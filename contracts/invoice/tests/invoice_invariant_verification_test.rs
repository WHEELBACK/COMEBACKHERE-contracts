//! #882: strict invariant verification for stored invoices.

use invoice::{
    verify_invoice_invariants, Invoice, InvoiceContract, InvoiceContractClient, InvoiceError,
    InvoiceStatus, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{testutils::Address as _, Address, Env};

const AMOUNT: i128 = 10_000_000;
const GROSS: i128 = 10_250_000;

fn base(env: &Env, status: InvoiceStatus) -> Invoice {
    let paid = !matches!(
        status,
        InvoiceStatus::Pending | InvoiceStatus::Expired | InvoiceStatus::Cancelled
    );
    Invoice {
        id: 1,
        merchant: Address::generate(env),
        amount_usdc: AMOUNT,
        gross_usdc: GROSS,
        status,
        expires_at: 100,
        paid_at: if paid { Some(10) } else { None },
        payer: if paid {
            MaybeAddress::Some(Address::generate(env))
        } else {
            MaybeAddress::None
        },
        metadata_hash: MaybeBytes::None,
        payment_link_hash: MaybeBytes::None,
        merchant_nonce: 0,
        token_address: MaybeAddress::None,
    }
}

const ALL: [InvoiceStatus; 7] = [
    InvoiceStatus::Pending,
    InvoiceStatus::Paid,
    InvoiceStatus::Expired,
    InvoiceStatus::Cancelled,
    InvoiceStatus::RefundRequested,
    InvoiceStatus::Released,
    InvoiceStatus::Refunded,
];

#[test]
fn well_formed_invoices_pass_in_every_status() {
    let env = Env::default();
    for status in ALL {
        assert_eq!(
            verify_invoice_invariants(&base(&env, status.clone())),
            Ok(()),
            "{status:?}"
        );
    }
}

#[test]
fn amount_pair_must_be_consistent() {
    let env = Env::default();
    for (amount, gross) in [
        (0, GROSS),
        (-1, GROSS),
        (AMOUNT, AMOUNT - 1),
        (i128::MIN, 0),
    ] {
        let mut inv = base(&env, InvoiceStatus::Pending);
        inv.amount_usdc = amount;
        inv.gross_usdc = gross;
        assert_eq!(
            verify_invoice_invariants(&inv),
            Err(InvoiceError::InvariantViolation),
            "amount={amount} gross={gross}"
        );
    }
}

#[test]
fn unpaid_statuses_must_not_carry_payment_fields() {
    let env = Env::default();
    for status in [
        InvoiceStatus::Pending,
        InvoiceStatus::Expired,
        InvoiceStatus::Cancelled,
    ] {
        let mut with_payer = base(&env, status.clone());
        with_payer.payer = MaybeAddress::Some(Address::generate(&env));
        assert_eq!(
            verify_invoice_invariants(&with_payer),
            Err(InvoiceError::InvariantViolation)
        );

        let mut with_paid_at = base(&env, status);
        with_paid_at.paid_at = Some(1);
        assert_eq!(
            verify_invoice_invariants(&with_paid_at),
            Err(InvoiceError::InvariantViolation)
        );
    }
}

#[test]
fn paid_statuses_must_carry_payment_fields() {
    let env = Env::default();
    for status in [
        InvoiceStatus::Paid,
        InvoiceStatus::RefundRequested,
        InvoiceStatus::Released,
        InvoiceStatus::Refunded,
    ] {
        let mut no_payer = base(&env, status.clone());
        no_payer.payer = MaybeAddress::None;
        assert_eq!(
            verify_invoice_invariants(&no_payer),
            Err(InvoiceError::InvariantViolation)
        );

        let mut no_paid_at = base(&env, status);
        no_paid_at.paid_at = None;
        assert_eq!(
            verify_invoice_invariants(&no_paid_at),
            Err(InvoiceError::InvariantViolation)
        );
    }
}

#[test]
fn payer_can_never_be_the_merchant() {
    let env = Env::default();
    let mut inv = base(&env, InvoiceStatus::Paid);
    inv.payer = MaybeAddress::Some(inv.merchant.clone());
    assert_eq!(
        verify_invoice_invariants(&inv),
        Err(InvoiceError::InvariantViolation)
    );
}

/// Every record the public lifecycle produces satisfies the invariants.
#[test]
fn full_lifecycle_records_satisfy_invariants() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let contract_id = env.register(InvoiceContract, ());
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let id = client.create_invoice(
        &merchant,
        &AMOUNT,
        &GROSS,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    assert_eq!(verify_invoice_invariants(&client.get_invoice(&id)), Ok(()));
    client.mark_paid(&admin, &id, &payer, &MaybeBytes::None, &MaybeAddress::None);
    assert_eq!(verify_invoice_invariants(&client.get_invoice(&id)), Ok(()));
    client.request_refund(&payer, &id);
    assert_eq!(verify_invoice_invariants(&client.get_invoice(&id)), Ok(()));
    client.approve_refund(&admin, &id);
    assert_eq!(verify_invoice_invariants(&client.get_invoice(&id)), Ok(()));
}
