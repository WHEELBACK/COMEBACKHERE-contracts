//! #885: zero-balance edge cases must not panic the contract.
//!
//! When a refund nets to zero (e.g. a 100% processing-fee policy) there is
//! nothing to pay out, so the refund must complete even if the escrow balance
//! is zero or the token contract rejects zero-value transfers. Negative
//! payouts must surface as a typed error rather than a trap.

use invoice::{
    transfer_net_refund, InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus,
    MaybeAddress, MaybeBytes,
};
use soroban_sdk::{testutils::Address as _, Address, Env};

/// A token that traps on a zero-value transfer and on any transfer the sender
/// cannot cover, the strictest behaviour a SEP-41 token may have.
mod strict_token {
    use soroban_sdk::{contract, contracterror, contractimpl, panic_with_error, Address, Env};

    #[contracterror]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u32)]
    pub enum TokenError {
        ZeroAmount = 1,
        InsufficientBalance = 2,
    }

    #[contract]
    pub struct StrictToken;

    #[contractimpl]
    impl StrictToken {
        pub fn mint(env: Env, to: Address, amount: i128) {
            let bal: i128 = env.storage().persistent().get(&to).unwrap_or(0);
            env.storage().persistent().set(&to, &(bal + amount));
        }

        pub fn balance(env: Env, of: Address) -> i128 {
            env.storage().persistent().get(&of).unwrap_or(0)
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            if amount == 0 {
                panic_with_error!(&env, TokenError::ZeroAmount);
            }
            let bal: i128 = env.storage().persistent().get(&from).unwrap_or(0);
            if bal < amount {
                panic_with_error!(&env, TokenError::InsufficientBalance);
            }
            env.storage().persistent().set(&from, &(bal - amount));
            let to_bal: i128 = env.storage().persistent().get(&to).unwrap_or(0);
            env.storage().persistent().set(&to, &(to_bal + amount));
        }
    }
}

use strict_token::{StrictToken, StrictTokenClient};

const AMOUNT: i128 = 10_000_000;
const GROSS: i128 = 10_250_000;

struct Fixture {
    env: Env,
    admin: Address,
    payer: Address,
    invoice_id: Address,
    invoice: InvoiceContractClient<'static>,
    token: StrictTokenClient<'static>,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let token_id = env.register(StrictToken, ());
    let invoice_id = env.register(InvoiceContract, ());
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    invoice.initialize(&admin);
    let token = StrictTokenClient::new(&env, &token_id);
    Fixture {
        env,
        admin,
        payer,
        invoice_id,
        invoice,
        token,
    }
}

fn paid_and_disputed(f: &Fixture) -> u64 {
    let merchant = Address::generate(&f.env);
    let id = f.invoice.create_invoice(
        &merchant,
        &AMOUNT,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::Some(f.token.address.clone()),
    );
    f.invoice.mark_paid(
        &f.admin,
        &id,
        &f.payer,
        &MaybeBytes::None,
        &MaybeAddress::Some(f.token.address.clone()),
    );
    f.invoice.request_refund(&f.payer, &id);
    id
}

/// Escrow is empty and the net payout is zero: the refund completes without
/// touching the token contract, which would otherwise trap on both counts.
#[test]
fn zero_net_refund_with_empty_escrow_completes() {
    let f = setup();
    let id = paid_and_disputed(&f);
    assert_eq!(f.token.balance(&f.invoice_id), 0);

    let applied = f.invoice.process_refund(&f.admin, &id, &10_000);

    assert_eq!(applied.net_amount, 0);
    assert_eq!(f.invoice.get_invoice(&id).status, InvoiceStatus::Refunded);
    assert_eq!(f.token.balance(&f.payer), 0);
    assert_eq!(f.token.balance(&f.invoice_id), 0);
}

/// Escrow is empty but a positive payout is owed: this is a real failure and
/// must come back as a typed error with the invoice left retryable.
#[test]
fn positive_refund_with_empty_escrow_fails_safely() {
    let f = setup();
    let id = paid_and_disputed(&f);

    let err = f
        .invoice
        .try_process_refund(&f.admin, &id, &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::RefundTransferFailed);
    assert_eq!(
        f.invoice.get_invoice(&id).status,
        InvoiceStatus::RefundRequested
    );
    assert!(f.invoice.try_get_refund_breakdown(&id).is_err());
}

/// The same zero-net refund succeeds once the escrow is funded, and leaves the
/// escrow untouched.
#[test]
fn zero_net_refund_with_funded_escrow_moves_nothing() {
    let f = setup();
    let id = paid_and_disputed(&f);
    f.token.mint(&f.invoice_id, &AMOUNT);

    f.invoice.process_refund(&f.admin, &id, &10_000);

    assert_eq!(f.token.balance(&f.invoice_id), AMOUNT);
    assert_eq!(f.token.balance(&f.payer), 0);
}

/// A zero or negative amount never reaches the token contract.
#[test]
fn transfer_net_refund_handles_zero_and_negative_amounts() {
    let env = Env::default();
    let token = Address::generate(&env);
    let recipient = Address::generate(&env);

    assert_eq!(transfer_net_refund(&env, &token, &recipient, 0), Ok(()));
    assert_eq!(
        transfer_net_refund(&env, &token, &recipient, -1),
        Err(InvoiceError::InvalidAmount)
    );
    assert_eq!(
        transfer_net_refund(&env, &token, &recipient, i128::MIN),
        Err(InvoiceError::InvalidAmount)
    );
}

/// A freshly created invoice reports a zero paid balance without panicking.
#[test]
fn pending_invoice_summary_reports_zero_paid() {
    let f = setup();
    let merchant = Address::generate(&f.env);
    let id = f.invoice.create_invoice(
        &merchant,
        &AMOUNT,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    let summary = f.invoice.get_invoice_summary(&id);
    assert_eq!(summary.paid_usdc, 0);
    assert_eq!(summary.remaining_usdc, GROSS);
}
