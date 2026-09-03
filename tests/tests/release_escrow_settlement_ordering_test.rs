//! Isolated coverage for the happy-path cross-contract ordering: invoice's
//! `release_escrow` followed by treasury's `execute_settlement` executing
//! correctly against the resulting state.
//!
//! Issue #89 added isolated integration coverage for the refund/cancellation
//! ordering interaction with treasury. The symmetric happy-path ordering
//! (escrow release, then settlement execution) is almost certainly exercised
//! *incidentally* inside `invoice_treasury_integration_test.rs`'s
//! `end_to_end_invoice_to_settlement` test and issue #83's broader end-to-end
//! lifecycle test — but incidental coverage inside a long end-to-end test is a
//! weaker guarantee than a test whose entire purpose is pinning down this one
//! interaction: a break here can get lost among the many other assertions in
//! a long test, and an unrelated refactor elsewhere in that same flow could
//! make the long test pass for the wrong reasons even if this ordering breaks.
//!
//! This file exists solely to isolate that ordering guarantee so a regression
//! in it fails here, specifically, rather than only inside a larger flow.

use invoice::{InvoiceContract, InvoiceContractClient, InvoiceStatus, MaybeAddress, MaybeBytes};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

mod test_token {
    use soroban_sdk::{contract, contractimpl, Address, Env};

    #[contract]
    pub struct TestToken;

    #[contractimpl]
    impl TestToken {
        pub fn mint(env: Env, to: Address, amount: i128) {
            let key = ("bal", to.clone());
            let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            env.storage().persistent().set(&key, &(bal + amount));
        }

        pub fn balance(env: Env, of: Address) -> i128 {
            let key = ("bal", of);
            env.storage().persistent().get(&key).unwrap_or(0)
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            let from_key = ("bal", from.clone());
            let to_key = ("bal", to.clone());
            let from_bal: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
            let to_bal: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&from_key, &(from_bal - amount));
            env.storage().persistent().set(&to_key, &(to_bal + amount));
        }
    }
}

use test_token::{TestToken, TestTokenClient};

struct Fixture {
    env: Env,
    admin: Address,
    merchant: Address,
    payer: Address,
    invoice: InvoiceContractClient<'static>,
    treasury_id: Address,
    treasury: TreasuryContractClient<'static>,
    token: TestTokenClient<'static>,
    token_id: Address,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);

    let invoice_id = env.register_contract(None, InvoiceContract);
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    invoice.initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    let token_id = env.register_contract(None, TestToken);
    let token = TestTokenClient::new(&env, &token_id);

    Fixture {
        env,
        admin,
        merchant,
        payer,
        invoice,
        treasury_id,
        treasury,
        token,
        token_id,
    }
}

/// The one interaction this file exists to pin down: `release_escrow` moves
/// the invoice to `Released`, and a settlement for the same amount, proposed
/// and executed *after* that release, correctly pays the merchant out of
/// treasury-held funds. Nothing about `release_escrow` should block or alter
/// a subsequent, independent `execute_settlement` call.
#[test]
fn release_escrow_then_execute_settlement_happy_path_ordering() {
    let fx = setup();
    let amount = 10_000_000i128;

    let inv_id = fx.invoice.create_invoice(
        &fx.merchant,
        &amount,
        &(amount + 250_000),
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );

    fx.invoice.mark_paid(
        &fx.admin,
        &inv_id,
        &fx.payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
    assert_eq!(fx.invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);

    // Step 1 of the ordering under test: release escrow on the invoice side.
    fx.invoice.release_escrow(&fx.admin, &inv_id);
    assert_eq!(
        fx.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Released
    );

    // Step 2: fund treasury and settle *after* the release above, verifying
    // execute_settlement executes correctly against the post-release state.
    fx.token.mint(&fx.treasury_id, &amount);
    assert_eq!(fx.token.balance(&fx.treasury_id), amount);
    assert_eq!(fx.token.balance(&fx.merchant), 0);

    let settlement_id = fx
        .treasury
        .propose_settlement(&fx.admin, &fx.merchant, &amount);
    fx.treasury
        .execute_settlement(&fx.admin, &settlement_id, &fx.token_id);

    let settlement = fx.treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Executed);
    assert_eq!(fx.token.balance(&fx.merchant), amount);
    assert_eq!(fx.token.balance(&fx.treasury_id), 0);

    // The invoice's own status is untouched by the treasury-side settlement -
    // release_escrow and execute_settlement operate on independent state that
    // must agree on outcome without either mutating the other's records.
    assert_eq!(
        fx.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Released
    );
}

/// Same ordering, but the settlement is proposed *before* `release_escrow` is
/// called and only executed afterward - confirming that an in-flight
/// settlement proposal is unaffected by (and does not need to wait on) the
/// invoice-side release, and still executes correctly once both have
/// happened in this order.
#[test]
fn settlement_proposed_before_release_still_executes_correctly_after() {
    let fx = setup();
    // Must be >= USDC_FACTOR (1 USDC): create_invoice enforces require_usdc_precision.
    let amount = 10_000_000i128;

    let inv_id = fx.invoice.create_invoice(
        &fx.merchant,
        &amount,
        &(amount + 100_000),
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    fx.invoice.mark_paid(
        &fx.admin,
        &inv_id,
        &fx.payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );

    let settlement_id = fx
        .treasury
        .propose_settlement(&fx.admin, &fx.merchant, &amount);

    fx.invoice.release_escrow(&fx.admin, &inv_id);
    assert_eq!(
        fx.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Released
    );

    fx.token.mint(&fx.treasury_id, &amount);
    fx.treasury
        .execute_settlement(&fx.admin, &settlement_id, &fx.token_id);

    assert_eq!(
        fx.treasury.get_settlement(&settlement_id).status,
        SettlementStatus::Executed
    );
    assert_eq!(fx.token.balance(&fx.merchant), amount);
}
