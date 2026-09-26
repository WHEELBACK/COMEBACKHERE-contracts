//! #70: payment state consistency before executing a cross-contract refund.
//!
//! `process_refund` is the invoice contract's refund path that actually moves
//! money, and it crosses a contract boundary into the invoice's `token_address`
//! contract. The tests here pin the two ways that can go wrong:
//!
//! - the invoice does not describe a completed payment, and
//! - the contract on the far side of the boundary fails.
//!
//! In both cases the requirement is the same: the refund must fail *safely* —
//! a typed error, and no state change that would claim the customer was paid.

use invoice::{
    verify_payment_state, Invoice, InvoiceContract, InvoiceContractClient, InvoiceError,
    InvoiceStatus, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// A SEP-41-shaped token whose `transfer` can be told to fail, standing in for
/// the Stellar Asset Contract (which signals failure by trapping, not by
/// returning an error value).
mod test_token {
    use soroban_sdk::{contract, contracterror, contractimpl, panic_with_error, Address, Env};

    #[contracterror]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u32)]
    pub enum TokenError {
        TransferFailed = 1,
    }

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

        /// Fails like a paused or drained SAC: the call traps, so the caller
        /// only learns about it through `try_`.
        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            let key = ("bal", from.clone());
            let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            if bal < amount {
                panic_with_error!(&env, TokenError::TransferFailed);
            }
            env.storage().persistent().set(&key, &(bal - amount));
            let to_key = ("bal", to);
            let to_bal: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
            env.storage().persistent().set(&to_key, &(to_bal + amount));
        }
    }
}

use test_token::{TestToken, TestTokenClient};

const AMOUNT: i128 = 10_000_000;
const GROSS: i128 = 10_250_000;

struct Fixture {
    env: Env,
    admin: Address,
    merchant: Address,
    payer: Address,
    invoice_id: Address,
    invoice: InvoiceContractClient<'static>,
    token_id: Address,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_id = env.register(TestToken, ());

    let invoice_id = env.register(InvoiceContract, ());
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    invoice.initialize(&admin);

    Fixture {
        env,
        admin,
        merchant,
        payer,
        invoice_id,
        invoice,
        token_id,
    }
}

/// Creates an invoice with `token_address` set, marks it paid by `payer`, and
/// leaves it in `RefundRequested`.
fn paid_and_disputed(f: &Fixture) -> u64 {
    let id = f.invoice.create_invoice(
        &f.merchant,
        &AMOUNT,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::Some(f.token_id.clone()),
    );
    f.invoice.mark_paid(
        &f.admin,
        &id,
        &f.payer,
        &MaybeBytes::None,
        &MaybeAddress::Some(f.token_id.clone()),
    );
    f.invoice.request_refund(&f.payer, &id);
    id
}

// ── payment state verification ───────────────────────────────────────────────

/// The happy path: a paid, disputed invoice with an intact payment record pays
/// the payer out and reaches `Refunded`.
#[test]
fn process_refund_pays_the_recorded_payer() {
    let f = setup();
    let id = paid_and_disputed(&f);

    // The invoice contract is the escrow: it holds what it refunds.
    let token = TestTokenClient::new(&f.env, &f.token_id);
    token.mint(&f.invoice_id, &AMOUNT);

    f.invoice.process_refund(&f.admin, &id);

    assert_eq!(f.invoice.get_invoice(&id).status, InvoiceStatus::Refunded);
    assert_eq!(token.balance(&f.payer), AMOUNT);
    assert_eq!(token.balance(&f.invoice_id), 0);
}

/// The payer the refund is paid to is the one recorded at `mark_paid`, never a
/// caller-supplied address.
#[test]
fn refund_recipient_is_the_recorded_payer() {
    let env = Env::default();
    let payer = Address::generate(&env);
    let invoice = Invoice {
        id: 1,
        merchant: Address::generate(&env),
        amount_usdc: AMOUNT,
        gross_usdc: GROSS,
        status: InvoiceStatus::RefundRequested,
        expires_at: 0,
        paid_at: Some(1),
        payer: MaybeAddress::Some(payer.clone()),
        metadata_hash: MaybeBytes::None,
        payment_link_hash: MaybeBytes::None,
        merchant_nonce: 0,
        token_address: MaybeAddress::None,
    };
    assert_eq!(invoice::refund_recipient(&invoice), Ok(payer));
}

/// An invoice whose payment was never timestamped, or whose payer was never
/// recorded, or whose amount pair is self-inconsistent, is not a completed
/// payment and must be refused. These states are unreachable through the
/// public entrypoints, which is exactly why the check is defence in depth.
#[test]
fn verify_payment_state_rejects_incomplete_payments() {
    let env = Env::default();
    let base = Invoice {
        id: 1,
        merchant: Address::generate(&env),
        amount_usdc: AMOUNT,
        gross_usdc: GROSS,
        status: InvoiceStatus::RefundRequested,
        expires_at: 0,
        paid_at: Some(42),
        payer: MaybeAddress::Some(Address::generate(&env)),
        metadata_hash: MaybeBytes::None,
        payment_link_hash: MaybeBytes::None,
        merchant_nonce: 0,
        token_address: MaybeAddress::None,
    };
    assert_eq!(verify_payment_state(&base), Ok(()));

    let mut untimed = base.clone();
    untimed.paid_at = None;
    assert_eq!(
        verify_payment_state(&untimed),
        Err(InvoiceError::PaymentStateInconsistent)
    );

    let mut payerless = base.clone();
    payerless.payer = MaybeAddress::None;
    assert_eq!(
        verify_payment_state(&payerless),
        Err(InvoiceError::PaymentStateInconsistent)
    );
    assert_eq!(
        invoice::refund_recipient(&payerless),
        Err(InvoiceError::PaymentStateInconsistent),
        "the recipient must not fall through to a default address"
    );

    let mut zero_amount = base.clone();
    zero_amount.amount_usdc = 0;
    assert_eq!(
        verify_payment_state(&zero_amount),
        Err(InvoiceError::PaymentStateInconsistent)
    );

    let mut gross_under_amount = base.clone();
    gross_under_amount.gross_usdc = AMOUNT - 1;
    assert_eq!(
        verify_payment_state(&gross_under_amount),
        Err(InvoiceError::PaymentStateInconsistent)
    );
}

// ── safe failure when the far side of the boundary fails ────────────────────

/// The payment/token contract call fails: the refund must revert with a typed
/// error and leave the invoice untouched, so it can be retried once the token
/// is healthy again. Marking it `Refunded` here would tell every indexer the
/// customer was paid when no tokens moved.
#[test]
fn payment_contract_call_failure_leaves_the_refund_retryable() {
    let f = setup();
    let id = paid_and_disputed(&f);

    // Escrow holds nothing, so the token rejects the transfer.
    let token = TestTokenClient::new(&f.env, &f.token_id);
    assert_eq!(token.balance(&f.invoice_id), 0);

    assert_eq!(
        f.invoice.try_process_refund(&f.admin, &id),
        Err(Ok(InvoiceError::RefundTransferFailed))
    );

    assert_eq!(
        f.invoice.get_invoice(&id).status,
        InvoiceStatus::RefundRequested,
        "a failed payout must not be recorded as a completed refund"
    );
    assert_eq!(token.balance(&f.payer), 0);

    // Retryable: once the escrow is funded, the same call succeeds.
    token.mint(&f.invoice_id, &AMOUNT);
    f.invoice.process_refund(&f.admin, &id);
    assert_eq!(f.invoice.get_invoice(&id).status, InvoiceStatus::Refunded);
    assert_eq!(token.balance(&f.payer), AMOUNT);
}

/// The address registered as `token_address` is not a token contract at all —
/// the invocation fails before any transfer is attempted, and the refund is
/// still refused.
#[test]
fn non_token_address_fails_the_refund_safely() {
    let f = setup();
    let impostor = Address::generate(&f.env);
    let id = f.invoice.create_invoice(
        &f.merchant,
        &AMOUNT,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::Some(impostor.clone()),
    );
    f.invoice.mark_paid(
        &f.admin,
        &id,
        &f.payer,
        &MaybeBytes::None,
        &MaybeAddress::Some(impostor.clone()),
    );
    f.invoice.request_refund(&f.payer, &id);

    assert_eq!(
        f.invoice.try_process_refund(&f.admin, &id),
        Err(Ok(InvoiceError::RefundTransferFailed))
    );
    assert_eq!(
        f.invoice.get_invoice(&id).status,
        InvoiceStatus::RefundRequested
    );
}

/// An invoice with no `token_address` has no contract to pay out through, and
/// is refused with a distinct error rather than being silently approved.
#[test]
fn refund_without_a_token_address_is_refused() {
    let f = setup();
    let id = f.invoice.create_invoice(
        &f.merchant,
        &AMOUNT,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    f.invoice.mark_paid(
        &f.admin,
        &id,
        &f.payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
    f.invoice.request_refund(&f.payer, &id);

    assert_eq!(
        f.invoice.try_process_refund(&f.admin, &id),
        Err(Ok(InvoiceError::RefundTokenNotSet))
    );
    assert_eq!(
        f.invoice.get_invoice(&id).status,
        InvoiceStatus::RefundRequested
    );
}

/// A paused payment contract must fail the refund safely, on both refund
/// entrypoints, and must not change the invoice.
#[test]
fn paused_payment_contract_fails_the_refund() {
    let f = setup();
    let id = paid_and_disputed(&f);
    f.invoice.pause(&f.admin);

    assert_eq!(
        f.invoice.try_process_refund(&f.admin, &id),
        Err(Ok(InvoiceError::ContractPaused))
    );
    assert_eq!(
        f.invoice.try_approve_refund(&f.admin, &id),
        Err(Ok(InvoiceError::ContractPaused))
    );
    assert_eq!(
        f.invoice.get_invoice(&id).status,
        InvoiceStatus::RefundRequested
    );

    f.invoice.unpause(&f.admin);
    f.invoice.approve_refund(&f.admin, &id);
    assert_eq!(f.invoice.get_invoice(&id).status, InvoiceStatus::Refunded);
}

// ── the approval path keeps the same guarantees ─────────────────────────────

/// A refund cannot be processed twice, and a refund on an invoice that was
/// never disputed is refused before any payout is attempted.
#[test]
fn refund_is_refused_for_an_undisputed_invoice_and_cannot_repeat() {
    let f = setup();
    let token = TestTokenClient::new(&f.env, &f.token_id);
    token.mint(&f.invoice_id, &(2 * AMOUNT));

    let id = f.invoice.create_invoice(
        &f.merchant,
        &AMOUNT,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::Some(f.token_id.clone()),
    );
    // Paid but never disputed.
    f.invoice.mark_paid(
        &f.admin,
        &id,
        &f.payer,
        &MaybeBytes::None,
        &MaybeAddress::Some(f.token_id.clone()),
    );
    assert_eq!(
        f.invoice.try_process_refund(&f.admin, &id),
        Err(Ok(InvoiceError::NotRefundRequested))
    );

    f.invoice.request_refund(&f.payer, &id);
    f.invoice.process_refund(&f.admin, &id);
    assert_eq!(f.invoice.get_invoice(&id).status, InvoiceStatus::Refunded);

    // Second attempt: no second payout, and the invoice is terminal.
    assert_eq!(
        f.invoice.try_process_refund(&f.admin, &id),
        Err(Ok(InvoiceError::NotRefundRequested))
    );
    assert_eq!(token.balance(&f.payer), AMOUNT);
}

/// Non-admins cannot trigger a payout.
#[test]
fn process_refund_is_admin_only() {
    let f = setup();
    let id = paid_and_disputed(&f);
    let stranger = Address::generate(&f.env);

    assert_eq!(
        f.invoice.try_process_refund(&stranger, &id),
        Err(Ok(InvoiceError::Unauthorized))
    );
    assert_eq!(
        f.invoice.get_invoice(&id).status,
        InvoiceStatus::RefundRequested
    );
}

/// The minimum USDC unit is still enforced on the refund path: an invoice
/// amount of zero is not a payment to reverse.
#[test]
fn zero_amount_invoice_is_not_a_completed_payment() {
    let env = Env::default();
    let invoice = Invoice {
        id: 1,
        merchant: Address::generate(&env),
        amount_usdc: 0,
        gross_usdc: 10_000_000,
        status: InvoiceStatus::RefundRequested,
        expires_at: 0,
        paid_at: Some(1),
        payer: MaybeAddress::Some(Address::generate(&env)),
        metadata_hash: MaybeBytes::None,
        payment_link_hash: MaybeBytes::None,
        merchant_nonce: 0,
        token_address: MaybeAddress::None,
    };
    assert_eq!(
        verify_payment_state(&invoice),
        Err(InvoiceError::PaymentStateInconsistent)
    );
}
