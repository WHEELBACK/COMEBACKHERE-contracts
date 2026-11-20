//! #878: partial-settlement computations never underflow/overflow.
//!
//! `get_invoice_summary` derives `remaining_usdc = gross_usdc - paid_usdc`.
//! These tests pin that the derivation is checked (it reports `0` instead of
//! trapping) even for invoice records at the `i128` extremes, that
//! `paid + remaining` still reconstructs the gross for well-formed invoices,
//! and that a negative discount — which would push the discounted settlement
//! amount above the invoice amount — is rejected.

use invoice::{
    require_valid_discount, DataKey, Invoice, InvoiceContract, InvoiceContractClient,
    InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{testutils::Address as _, Address, Env};

const AMOUNT_USDC: i128 = 10_000_000;
const GROSS_USDC: i128 = 10_250_000;

struct Fixture {
    env: Env,
    contract_id: Address,
    admin: Address,
    merchant: Address,
    payer: Address,
    client: InvoiceContractClient<'static>,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let contract_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    Fixture {
        env,
        contract_id,
        admin,
        merchant,
        payer,
        client,
    }
}

fn create(f: &Fixture) -> u64 {
    f.client.create_invoice(
        &f.merchant,
        &AMOUNT_USDC,
        &GROSS_USDC,
        &3_600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    )
}

/// Overwrites the stored record's gross amount and status, simulating a
/// corrupted or extreme record the summary must still handle.
fn force(f: &Fixture, id: u64, gross: i128, status: InvoiceStatus) {
    let mut invoice: Invoice = f.client.get_invoice(&id);
    invoice.gross_usdc = gross;
    invoice.status = status;
    f.env.as_contract(&f.contract_id, || {
        f.env
            .storage()
            .persistent()
            .set(&DataKey::Invoice(id), &invoice);
    });
}

#[test]
fn summary_never_traps_at_i128_extremes() {
    let f = setup();
    let id = create(&f);
    let statuses = [
        InvoiceStatus::Pending,
        InvoiceStatus::Paid,
        InvoiceStatus::RefundRequested,
        InvoiceStatus::Released,
        InvoiceStatus::Refunded,
        InvoiceStatus::Expired,
        InvoiceStatus::Cancelled,
    ];
    for gross in [i128::MIN, i128::MIN + 1, -1, 0, 1, i128::MAX - 1, i128::MAX] {
        for status in statuses.iter() {
            force(&f, id, gross, status.clone());
            let summary = f.client.get_invoice_summary(&id);
            assert!(
                summary.remaining_usdc >= 0,
                "remaining must never go negative: gross={gross} status={status:?}"
            );
            assert!(summary.remaining_usdc <= gross.max(0));
        }
    }
}

#[test]
fn paid_plus_remaining_reconstructs_gross_through_settlement() {
    let f = setup();
    let id = create(&f);

    let pending = f.client.get_invoice_summary(&id);
    assert_eq!(pending.paid_usdc + pending.remaining_usdc, GROSS_USDC);
    assert_eq!(pending.remaining_usdc, GROSS_USDC);

    f.client.mark_paid(
        &f.admin,
        &id,
        &f.payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
    let paid = f.client.get_invoice_summary(&id);
    assert_eq!(paid.paid_usdc, GROSS_USDC);
    assert_eq!(paid.remaining_usdc, 0);
}

#[test]
fn negative_discount_is_rejected() {
    for discount in [-1, -AMOUNT_USDC, i128::MIN] {
        assert_eq!(
            require_valid_discount(discount, AMOUNT_USDC, 0, 10),
            Err(InvoiceError::InvalidAmount),
            "discount {discount}"
        );
    }
}

#[test]
fn discount_boundaries_are_unchanged() {
    assert_eq!(require_valid_discount(0, AMOUNT_USDC, 0, 10), Ok(()));
    assert_eq!(require_valid_discount(AMOUNT_USDC - 1, AMOUNT_USDC, 0, 10), Ok(()));
    assert_eq!(
        require_valid_discount(AMOUNT_USDC, AMOUNT_USDC, 0, 10),
        Err(InvoiceError::DiscountExceedsAmount)
    );
    assert_eq!(
        require_valid_discount(0, AMOUNT_USDC, 11, 10),
        Err(InvoiceError::DiscountDeadlineAfterExpiry)
    );
}
