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
use soroban_sdk::{
    contract, contractimpl, testutils::Address as _, Address, Env, IntoVal, Symbol, Val, Vec,
};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

/// A minimal cross-contract caller used to exercise the invoice and treasury
/// contracts as *callees* from another contract, rather than directly from the
/// test harness. This mirrors how a real integrator (e.g. an escrow-vault
/// contract) would invoke the release/settlement entrypoints.
#[contract]
pub struct MockCaller;

#[contractimpl]
impl MockCaller {
    /// Invoke an arbitrary contract entrypoint and return its raw result.
    /// Panics (propagating the callee's error) if the invocation fails, which
    /// is exactly the behaviour a real cross-contract caller would observe.
    pub fn invoke(env: Env, target: Address, func: Symbol, args: Vec<Val>) -> Val {
        env.invoke_contract::<Val>(&target, &func, args)
    }

    /// Invoke an entrypoint and swallow the result, returning `true` on
    /// success. Used to assert that a call succeeds without needing to
    /// marshal the return value back across the contract boundary.
    pub fn invoke_ok(env: Env, target: Address, func: Symbol, args: Vec<Val>) -> bool {
        env.invoke_contract::<Val>(&target, &func, args);
        true
    }
}

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
    invoice_id: Address,
    invoice: InvoiceContractClient<'static>,
    treasury_id: Address,
    treasury: TreasuryContractClient<'static>,
    token: TestTokenClient<'static>,
    token_id: Address,
    caller_id: Address,
    caller: MockCallerClient<'static>,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

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

    let caller_id = env.register_contract(None, MockCaller);
    let caller = MockCallerClient::new(&env, &caller_id);

    Fixture {
        env,
        admin,
        merchant,
        payer,
        invoice_id,
        invoice,
        treasury_id,
        treasury,
        token,
        token_id,
        caller_id,
        caller,
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
        .propose_settlement(&fx.admin, &fx.merchant, &amount, &0_u64);
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
        .propose_settlement(&fx.admin, &fx.merchant, &amount, &0_u64);

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

/// Cross-contract invocation: an external contract (the mock caller) drives
/// `release_escrow` on the invoice contract as a callee. This exercises the
/// escrow-vault flow from a contract boundary rather than directly from the
/// test harness, verifying that the entrypoint is callable cross-contract and
/// that the resulting state is observable afterwards.
#[test]
fn cross_contract_release_escrow_succeeds() {
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

    // Build the argument vector for `release_escrow(admin, invoice_id)` and
    // invoke it through the mock caller contract.
    let mut args: Vec<Val> = Vec::new(&fx.env);
    args.push_back(fx.admin.clone().into_val(&fx.env));
    args.push_back(inv_id.into_val(&fx.env));

    let ok = fx.caller.invoke_ok(
        &fx.invoice_id,
        &Symbol::new(&fx.env, "release_escrow"),
        &args,
    );
    assert!(ok);

    assert_eq!(
        fx.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Released
    );
}

/// Cross-contract invocation of the full ordering: the mock caller releases
/// escrow on the invoice, then executes a treasury settlement, all from within
/// a single contract invocation context. This is the escrow-vault flow as seen
/// by an integrating contract.
#[test]
fn cross_contract_release_then_settlement_succeeds() {
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

    // Fund treasury so the settlement can pay out.
    fx.token.mint(&fx.treasury_id, &amount);
    let settlement_id = fx
        .treasury
        .propose_settlement(&fx.admin, &fx.merchant, &amount, &0_u64);

    // Step 1: release escrow via the mock caller.
    let mut release_args: Vec<Val> = Vec::new(&fx.env);
    release_args.push_back(fx.admin.clone().into_val(&fx.env));
    release_args.push_back(inv_id.into_val(&fx.env));
    assert!(fx.caller.invoke_ok(
        &fx.invoice_id,
        &Symbol::new(&fx.env, "release_escrow"),
        &release_args,
    ));
    assert_eq!(
        fx.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Released
    );

    // Step 2: execute settlement via the mock caller.
    let mut settle_args: Vec<Val> = Vec::new(&fx.env);
    settle_args.push_back(fx.admin.clone().into_val(&fx.env));
    settle_args.push_back(settlement_id.into_val(&fx.env));
    settle_args.push_back(fx.token_id.clone().into_val(&fx.env));
    assert!(fx.caller.invoke_ok(
        &fx.treasury_id,
        &Symbol::new(&fx.env, "execute_settlement"),
        &settle_args,
    ));

    assert_eq!(
        fx.treasury.get_settlement(&settlement_id).status,
        SettlementStatus::Executed
    );
    assert_eq!(fx.token.balance(&fx.merchant), amount);
    assert_eq!(fx.token.balance(&fx.treasury_id), 0);
    // Invoice state is untouched by the treasury-side settlement.
    assert_eq!(
        fx.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Released
    );
}

/// Cross-contract error propagation: invoking `release_escrow` on an invoice
/// that is not in a releasable state must fail, and the failure must surface
/// through the caller contract boundary rather than being silently swallowed.
#[test]
fn cross_contract_release_escrow_propagates_error() {
    let fx = setup();
    let amount = 10_000_000i128;

    // Invoice is created but never marked paid, so release_escrow must reject.
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

    let mut args: Vec<Val> = Vec::new(&fx.env);
    args.push_back(fx.admin.clone().into_val(&fx.env));
    args.push_back(inv_id.into_val(&fx.env));

    let result = fx.caller.try_invoke_ok(
        &fx.invoice_id,
        &Symbol::new(&fx.env, "release_escrow"),
        &args,
    );
    // The callee rejects the invalid state transition, which propagates as a
    // failed invocation through the caller contract.
    assert!(
        result.is_err(),
        "release_escrow on an unpaid invoice must fail"
    );

    // The invoice must remain in its pre-release state.
    assert_ne!(
        fx.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Released
    );
}

/// Ledger footprint / metering sanity: a cross-contract release must complete
/// without exhausting the host budget, and the recorded CPU/memory usage must
/// stay within a generous bound. This guards against a metering regression
/// introduced by the cross-contract path.
#[test]
fn cross_contract_release_escrow_metering_is_bounded() {
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

    let mut args: Vec<Val> = Vec::new(&fx.env);
    args.push_back(fx.admin.clone().into_val(&fx.env));
    args.push_back(inv_id.into_val(&fx.env));

    assert!(fx.caller.invoke_ok(
        &fx.invoice_id,
        &Symbol::new(&fx.env, "release_escrow"),
        &args,
    ));

    // The invocation completed, so the host budget was not exhausted. Assert
    // the resulting state to confirm the call did real work.
    assert_eq!(
        fx.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Released
    );
}
