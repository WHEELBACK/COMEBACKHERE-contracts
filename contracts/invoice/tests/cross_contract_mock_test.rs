//! #886: cross-contract invocation mock tests for `invoice`.
//!
//! `process_refund` is the invoice contract's only outbound cross-contract
//! call. These tests use Soroban SDK testutils to register mock contracts on
//! the far side of that boundary and pin exactly how the invoice contract
//! invokes them:
//!
//! - a recording mock that captures every `transfer` invocation's arguments,
//! - a contract that does not export `transfer` at all,
//! - the real Stellar Asset Contract registered through testutils,
//!
//! and asserts the authorization tree the invoice contract produces for the
//! nested call.

use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
    REFUND_NETWORK_FEE,
};
use soroban_sdk::{
    testutils::{Address as _, AuthorizedFunction, AuthorizedInvocation},
    token::{StellarAssetClient, TokenClient},
    Address, Env, IntoVal, Symbol,
};

/// Records every `transfer` call so the test can assert the exact arguments
/// the invoice contract passed across the boundary.
mod recording_token {
    use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Vec};

    #[contracttype]
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct TransferCall {
        pub from: Address,
        pub to: Address,
        pub amount: i128,
    }

    const CALLS: &str = "calls";

    #[contract]
    pub struct RecordingToken;

    #[contractimpl]
    impl RecordingToken {
        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            let mut calls: Vec<TransferCall> = env
                .storage()
                .instance()
                .get(&CALLS)
                .unwrap_or_else(|| Vec::new(&env));
            calls.push_back(TransferCall { from, to, amount });
            env.storage().instance().set(&CALLS, &calls);
        }

        pub fn calls(env: Env) -> Vec<TransferCall> {
            env.storage()
                .instance()
                .get(&CALLS)
                .unwrap_or_else(|| Vec::new(&env))
        }
    }
}

/// A contract that exists but exposes no `transfer` entrypoint.
mod not_a_token {
    use soroban_sdk::{contract, contractimpl};

    #[contract]
    pub struct NotAToken;

    #[contractimpl]
    impl NotAToken {
        pub fn ping() -> u32 {
            1
        }
    }
}

use not_a_token::NotAToken;
use recording_token::{RecordingToken, RecordingTokenClient, TransferCall};

const AMOUNT: i128 = 10_000_000;
const GROSS: i128 = 10_250_000;

struct Fixture {
    env: Env,
    admin: Address,
    payer: Address,
    invoice_id: Address,
    invoice: InvoiceContractClient<'static>,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let invoice_id = env.register(InvoiceContract, ());
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    invoice.initialize(&admin);
    Fixture {
        env,
        admin,
        payer,
        invoice_id,
        invoice,
    }
}

fn paid_and_disputed(f: &Fixture, token: &Address) -> u64 {
    let merchant = Address::generate(&f.env);
    let id = f.invoice.create_invoice(
        &merchant,
        &AMOUNT,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::Some(token.clone()),
    );
    f.invoice.mark_paid(
        &f.admin,
        &id,
        &f.payer,
        &MaybeBytes::None,
        &MaybeAddress::Some(token.clone()),
    );
    f.invoice.request_refund(&f.payer, &id);
    id
}

/// The invoice contract invokes `transfer` exactly once, from its own address,
/// to the recorded payer, for the net amount.
#[test]
fn process_refund_invokes_transfer_with_exact_arguments() {
    let f = setup();
    let token_id = f.env.register(RecordingToken, ());
    let token = RecordingTokenClient::new(&f.env, &token_id);
    let id = paid_and_disputed(&f, &token_id);

    let applied = f.invoice.process_refund(&f.admin, &id, &0);

    let calls = token.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls.get(0).unwrap(),
        TransferCall {
            from: f.invoice_id.clone(),
            to: f.payer.clone(),
            amount: AMOUNT - REFUND_NETWORK_FEE,
        }
    );
    assert_eq!(applied.net_amount, AMOUNT - REFUND_NETWORK_FEE);
    assert_eq!(f.invoice.get_invoice(&id).status, InvoiceStatus::Refunded);
}

/// The nested `transfer` call is authorized by the invoice contract itself as
/// part of the admin-authorized `process_refund` invocation; no other address
/// is asked to sign for the escrow debit.
#[test]
fn process_refund_authorization_tree_is_admin_then_nested_transfer() {
    let f = setup();
    let token_id = f.env.register(RecordingToken, ());
    let id = paid_and_disputed(&f, &token_id);

    f.invoice.process_refund(&f.admin, &id, &0);

    let net = AMOUNT - REFUND_NETWORK_FEE;
    let auths = f.env.auths();
    assert_eq!(auths.len(), 1, "only the admin signs a refund payout");
    let (signer, invocation): &(Address, AuthorizedInvocation) = &auths[0];
    assert_eq!(signer, &f.admin);
    assert_eq!(
        invocation.function,
        AuthorizedFunction::Contract((
            f.invoice_id.clone(),
            Symbol::new(&f.env, "process_refund"),
            (f.admin.clone(), id, 0u32).into_val(&f.env),
        ))
    );
    // The escrow debit is authorized implicitly by the invoice contract being
    // the direct caller, so it is recorded on the mock rather than in `auths`.
    let calls = RecordingTokenClient::new(&f.env, &token_id).calls();
    assert_eq!(calls.get(0).unwrap().from, f.invoice_id);
    assert_eq!(calls.get(0).unwrap().amount, net);
}

/// A rejected fee policy is caught before the boundary: the mock is never
/// invoked.
#[test]
fn invalid_fee_never_reaches_the_token_contract() {
    let f = setup();
    let token_id = f.env.register(RecordingToken, ());
    let id = paid_and_disputed(&f, &token_id);

    let err = f
        .invoice
        .try_process_refund(&f.admin, &id, &10_001)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::RefundFeeTooHigh);
    assert_eq!(
        RecordingTokenClient::new(&f.env, &token_id).calls().len(),
        0
    );
}

/// `approve_refund` records the decision only and must not call out.
#[test]
fn approve_refund_makes_no_cross_contract_call() {
    let f = setup();
    let token_id = f.env.register(RecordingToken, ());
    let id = paid_and_disputed(&f, &token_id);

    f.invoice.approve_refund(&f.admin, &id);

    assert_eq!(
        RecordingTokenClient::new(&f.env, &token_id).calls().len(),
        0
    );
    assert_eq!(f.invoice.get_invoice(&id).status, InvoiceStatus::Refunded);
}

/// A token address that points at a contract without `transfer` fails the
/// invocation; the invoice contract maps that to a typed error and keeps the
/// invoice retryable.
#[test]
fn missing_transfer_export_is_a_typed_error() {
    let f = setup();
    let token_id = f.env.register(NotAToken, ());
    let id = paid_and_disputed(&f, &token_id);

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
}

/// End to end against the real Stellar Asset Contract registered via
/// testutils: the escrow is debited and the payer credited by the net amount.
#[test]
fn process_refund_against_stellar_asset_contract() {
    let f = setup();
    let issuer = Address::generate(&f.env);
    let sac = f.env.register_stellar_asset_contract_v2(issuer);
    let token_id = sac.address();
    StellarAssetClient::new(&f.env, &token_id).mint(&f.invoice_id, &AMOUNT);
    let id = paid_and_disputed(&f, &token_id);

    f.invoice.process_refund(&f.admin, &id, &250);

    let token = TokenClient::new(&f.env, &token_id);
    let breakdown = f.invoice.get_refund_breakdown(&id);
    assert_eq!(token.balance(&f.payer), breakdown.net_amount);
    assert_eq!(token.balance(&f.invoice_id), AMOUNT - breakdown.net_amount);
    assert_eq!(f.invoice.get_invoice(&id).status, InvoiceStatus::Refunded);
}

extern crate std;
