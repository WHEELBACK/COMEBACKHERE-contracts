use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

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

fn setup() -> (
    Env,
    Address,
    Address,
    Address,
    InvoiceContractClient<'static>,
    Address,
    TreasuryContractClient<'static>,
    Address,
) {
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
    (
        env,
        admin,
        merchant,
        payer,
        invoice,
        treasury_id,
        treasury,
        token_id,
    )
}

#[test]
fn invoice_created_paid_released() {
    let (env, admin, merchant, payer, invoice, _treasury_id, _treasury, _token_id) = setup();

    let id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    let inv = invoice.get_invoice(&id);
    assert_eq!(inv.status, InvoiceStatus::Pending);

    invoice.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    let inv = invoice.get_invoice(&id);
    assert_eq!(inv.status, InvoiceStatus::Paid);
    assert_eq!(inv.payer, MaybeAddress::Some(payer.clone()));

    invoice.release_escrow(&admin, &id);
    let inv = invoice.get_invoice(&id);
    assert_eq!(inv.status, InvoiceStatus::Released);
}

#[test]
fn treasury_settlement_after_invoice_release() {
    let (env, admin, merchant, _payer, _invoice, treasury_id, treasury, token_id) = setup();

    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    let settlement = treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.merchant_address, merchant);
    assert_eq!(settlement.amount, 10_000_000);
    assert_eq!(settlement.approval_weight, 1);

    let token = TestTokenClient::new(&env, &token_id);
    token.mint(&treasury_id, &10_000_000);

    treasury.execute_settlement(&admin, &settlement_id, &token_id);
    let settled = treasury.get_settlement(&settlement_id);
    assert_eq!(settled.status, treasury::SettlementStatus::Executed);
    assert_eq!(token.balance(&merchant), 10_000_000);
}

#[test]
fn end_to_end_invoice_to_settlement() {
    let (env, admin, merchant, payer, invoice, treasury_id, treasury, token_id) = setup();

    let inv_id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );

    invoice.mark_paid(
        &admin,
        &inv_id,
        &payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);

    invoice.release_escrow(&admin, &inv_id);
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Released);

    let token = TestTokenClient::new(&env, &token_id);
    token.mint(&treasury_id, &10_000_000);
    assert_eq!(token.balance(&treasury_id), 10_000_000);
    assert_eq!(token.balance(&merchant), 0);

    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    treasury.execute_settlement(&admin, &settlement_id, &token_id);

    assert_eq!(token.balance(&treasury_id), 0);
    assert_eq!(token.balance(&merchant), 10_000_000);
}

// ── #280 refund workflow's interaction (or lack thereof) with treasury ─────────
//
// `request_refund`/`approve_refund` only flip the invoice's own status field;
// they take no `treasury_id` or token argument and never call into the
// treasury contract. This test documents that behaviour end-to-end: once a
// settlement has been executed and the merchant has been paid out, approving
// a refund on the invoice does NOT claw back the merchant's token balance or
// touch the settlement record. Any fund reversal must be performed
// separately (e.g. a manual treasury withdrawal/transfer), it is not an
// on-chain side effect of the refund approval today.
#[test]
fn approved_refund_does_not_adjust_treasury_balance_or_settlement() {
    let (env, admin, merchant, payer, invoice, treasury_id, treasury, token_id) = setup();

    let inv_id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    invoice.mark_paid(
        &admin,
        &inv_id,
        &payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);

    // Treasury settles the invoice: merchant is paid out on-chain.
    let token = TestTokenClient::new(&env, &token_id);
    token.mint(&treasury_id, &10_000_000);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    treasury.execute_settlement(&admin, &settlement_id, &token_id);
    assert_eq!(token.balance(&merchant), 10_000_000);
    assert_eq!(token.balance(&treasury_id), 0);

    // Payer disputes and the refund is approved purely at the invoice layer.
    invoice.request_refund(&payer, &inv_id);
    assert_eq!(
        invoice.get_invoice(&inv_id).status,
        InvoiceStatus::RefundRequested
    );
    invoice.approve_refund(&admin, &inv_id);
    assert_eq!(invoice.get_invoice(&inv_id).status, InvoiceStatus::Refunded);

    // The merchant's payout and the settlement record are untouched: no
    // on-chain fund movement is triggered by refund approval.
    assert_eq!(token.balance(&merchant), 10_000_000);
    assert_eq!(token.balance(&treasury_id), 0);
    let settlement = treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.status, treasury::SettlementStatus::Executed);
}

#[test]
fn request_refund_rejected_for_non_paid_invoice() {
    let (_env, _admin, merchant, payer, invoice, _treasury_id, _treasury, _token_id) = setup();

    let inv_id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    // Invoice is still Pending, never marked Paid.
    let result = invoice.try_request_refund(&payer, &inv_id);
    assert_eq!(result, Err(Ok(InvoiceError::NotPaid)));
}
