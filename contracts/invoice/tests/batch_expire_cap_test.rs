//! Adversarial coverage for `batch_expire`'s `MAX_BATCH_EXPIRE` cap (issue #8/#18 precedent).
//!
//! `contracts/invoice/src/entrypoints/batch.rs::batch_expire` checks
//! `ids.len() > MAX_BATCH_EXPIRE` and returns `InvoiceError::BatchTooLarge`
//! **before** the per-ID loop begins, so nothing in `ids` should ever be
//! touched when the batch is oversized — regardless of whether individual IDs
//! in that oversized batch are valid, already-expired, already-cancelled, or
//! simply nonexistent.
//!
//! `contracts/invoice/tests/invoice_load_test.rs` already covers the
//! `MAX_BATCH_EXPIRE + 1` boundary with an all-valid-and-pending batch (see
//! `batch_expire_rejects_more_than_max_batch_size`) and confirms the
//! instruction budget at the cap (`batch_expire_at_cap_stays_under_instruction_budget`).
//! Neither of those exercises a *mixed* oversized batch, so they cannot catch a
//! regression where the size guard is checked correctly but a future refactor
//! reorders things so that some IDs get partially processed before the cap
//! check runs (which the current implementation does not do, but which isn't
//! pinned down by any existing test using only valid IDs). This file closes
//! that gap: it pads an otherwise-valid batch past `MAX_BATCH_EXPIRE` with
//! cheap-to-skip invalid IDs (nonexistent, already-cancelled, already-expired)
//! and asserts the whole call is rejected with zero side effects on any of the
//! valid entries mixed into the batch.

use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
    MAX_BATCH_EXPIRE,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Vec,
};

fn setup() -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

fn create_pending_invoice(
    env: &Env,
    client: &InvoiceContractClient<'static>,
    merchant: &Address,
    expires_in_seconds: u64,
) -> u64 {
    client.create_invoice(
        merchant,
        &10_000_000,
        &10_250_000,
        &expires_in_seconds,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    )
}

/// A batch of exactly `MAX_BATCH_EXPIRE + 1` IDs, deliberately mixing
/// already-expired-and-pending (valid, should expire), already-cancelled
/// (valid ID, wrong status), and nonexistent IDs (invalid, cheap to skip),
/// must be rejected wholesale with `BatchTooLarge` and must leave every valid
/// invoice's state completely untouched.
#[test]
fn oversized_adversarial_batch_is_rejected_before_any_processing() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000);

    // MAX_BATCH_EXPIRE - 2 genuinely pending invoices that would expire if the
    // call were allowed to proceed (expires_at = 1001, well before the
    // timestamp advance below).
    let valid_pending_count = MAX_BATCH_EXPIRE - 2;
    let mut valid_pending_ids: Vec<u64> = Vec::new(&env);
    for _ in 0..valid_pending_count {
        let id = create_pending_invoice(&env, &client, &merchant, 1);
        valid_pending_ids.push_back(id);
    }

    // One already-cancelled invoice: a real, existing invoice ID, but not in
    // Pending status, so batch_expire would skip it even if processed.
    let cancelled_id = create_pending_invoice(&env, &client, &merchant, 3600);
    client.cancel_invoice(&merchant, &cancelled_id);

    // One already-paid-and-released style terminal invoice stands in for
    // "valid ID, non-Pending status" alongside the cancelled one — reuse
    // cancellation for simplicity since the property under test (no status
    // change) is identical regardless of which terminal status is involved.
    let cancelled_id_2 = create_pending_invoice(&env, &client, &merchant, 3600);
    client.cancel_invoice(&merchant, &cancelled_id_2);

    // Nonexistent IDs padding the batch past the cap — cheap to skip, exactly
    // the kind of filler an adversarial caller would use to bypass a
    // processing-order-dependent guard.
    let mut ids: Vec<u64> = Vec::new(&env);
    for id in valid_pending_ids.iter() {
        ids.push_back(id);
    }
    ids.push_back(cancelled_id);
    ids.push_back(cancelled_id_2);
    // Nonexistent invoice IDs (well past any ID actually issued above).
    for nonexistent in 1_000_000u64..1_000_004u64 {
        ids.push_back(nonexistent);
    }

    // ids.len() == (MAX_BATCH_EXPIRE - 2) + 2 + 4 == MAX_BATCH_EXPIRE + 4,
    // safely over the MAX_BATCH_EXPIRE + 1 boundary this issue asks for.
    assert!(ids.len() > MAX_BATCH_EXPIRE);

    // Advance past every valid-pending invoice's expiry.
    env.ledger().with_mut(|l| l.timestamp = 2_000);

    let err = client.try_batch_expire(&admin, &ids).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::BatchTooLarge);

    // No partial processing: every genuinely-pending invoice that would have
    // expired if the call had been allowed to proceed must still be Pending.
    for id in valid_pending_ids.iter() {
        let invoice = client.get_invoice(&id);
        assert_eq!(
            invoice.status,
            InvoiceStatus::Pending,
            "invoice {id} must not have been expired by a rejected oversized batch"
        );
    }

    // The already-cancelled invoices must remain Cancelled, untouched.
    assert_eq!(client.get_invoice(&cancelled_id).status, InvoiceStatus::Cancelled);
    assert_eq!(client.get_invoice(&cancelled_id_2).status, InvoiceStatus::Cancelled);
}

/// Precisely `MAX_BATCH_EXPIRE + 1` IDs — the exact boundary named in the
/// issue — mixing one nonexistent ID in with otherwise-valid pending
/// invoices, must still be rejected outright.
#[test]
fn exactly_max_batch_expire_plus_one_with_one_invalid_id_is_rejected() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let mut ids: Vec<u64> = Vec::new(&env);
    for _ in 0..MAX_BATCH_EXPIRE {
        let id = create_pending_invoice(&env, &client, &merchant, 1);
        ids.push_back(id);
    }
    // Pad with a single nonexistent ID to reach MAX_BATCH_EXPIRE + 1 without
    // creating an extra real invoice.
    ids.push_back(9_999_999u64);
    assert_eq!(ids.len() as u32, MAX_BATCH_EXPIRE + 1);

    env.ledger().with_mut(|l| l.timestamp = 2_000);

    let err = client.try_batch_expire(&admin, &ids).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::BatchTooLarge);

    // None of the MAX_BATCH_EXPIRE valid pending invoices should have been
    // expired despite being individually valid and expirable.
    for i in 0..MAX_BATCH_EXPIRE {
        let id = ids.get(i).unwrap();
        let invoice = client.get_invoice(&id);
        assert_eq!(invoice.status, InvoiceStatus::Pending);
    }
}
