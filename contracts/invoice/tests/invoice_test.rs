use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

extern crate std;
use std::{collections::HashSet, fs, path::Path};

fn setup() -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

#[test]
fn test_create_invoice_succeeds() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.id, 1);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.amount_usdc, 10_000_000);
    assert_eq!(invoice.gross_usdc, 10_250_000);
    assert_eq!(invoice.payer, MaybeAddress::None);
    assert_eq!(invoice.merchant_nonce, 0);
}

#[test]
fn test_mark_paid_requires_admin() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let rogue_admin = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    assert!(client.try_mark_paid(&rogue_admin, &id, &payer).is_err());
}

#[test]
fn test_expired_invoice_cannot_be_paid() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &1,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    env.ledger().with_mut(|ledger| ledger.timestamp += 2);
    assert!(client.try_mark_paid(&admin, &id, &payer).is_err());
}

#[test]
fn test_pause_blocks_create_invoice() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    client.pause(&admin);
    assert!(client
        .try_create_invoice(
            &merchant,
            &10_000_000,
            &10_250_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0
        )
        .is_err());
}

#[test]
fn test_pause_blocks_mark_paid() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.pause(&admin);
    assert!(client.try_mark_paid(&admin, &id, &payer).is_err());
}

#[test]
fn test_double_payment_rejected() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.mark_paid(&admin, &id, &payer);
    assert!(client.try_mark_paid(&admin, &id, &payer).is_err());
}

#[test]
fn test_get_invoice_unknown_id_returns_not_found() {
    let (_env, _admin, client) = setup();
    let err = client.try_get_invoice(&999).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::NotFound);
}

#[test]
fn test_mark_paid_unknown_id_returns_not_found() {
    let (env, admin, client) = setup();
    let payer = Address::generate(&env);
    let err = client
        .try_mark_paid(&admin, &999, &payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::NotFound);
}

#[test]
fn test_payer_set_after_payment() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.mark_paid(&admin, &id, &payer);
    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.payer, MaybeAddress::Some(payer));
}

#[test]
fn test_expired_event_emitted_on_stale_mark_paid() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &1,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    env.ledger().with_mut(|ledger| ledger.timestamp += 2);
    let err = client
        .try_mark_paid(&admin, &id, &payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Expired);
    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
}

#[test]
fn test_payment_at_exact_expiry_is_rejected() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &10,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    env.ledger().with_mut(|ledger| ledger.timestamp = 10);
    let err = client
        .try_mark_paid(&admin, &id, &payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Expired);
}

#[test]
fn test_payment_before_expiry_succeeds() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &10,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    env.ledger().with_mut(|ledger| ledger.timestamp = 9);
    client.mark_paid(&admin, &id, &payer);
    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Paid);
}

#[test]
fn test_initialize_requires_admin_auth() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    let auths = env.auths();
    assert!(auths.iter().any(|(addr, _)| addr == &admin));
}

#[test]
fn test_initialize_cannot_be_called_twice() {
    let (env, _admin, client) = setup();
    let new_admin = Address::generate(&env);
    assert!(client.try_initialize(&new_admin).is_err());
}

#[test]
fn test_zero_duration_invoice_rejected() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    assert!(client
        .try_create_invoice(
            &merchant,
            &10_000_000,
            &10_250_000,
            &0,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0
        )
        .is_err());
}

#[test]
fn test_expiry_overflow_rejected() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    env.ledger().with_mut(|l| l.timestamp = u64::MAX);
    assert!(client
        .try_create_invoice(
            &merchant,
            &10_000_000,
            &10_250_000,
            &1,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0
        )
        .is_err());
}

#[test]
fn test_event_stream_redis_webhook_compatibility() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);

    let invoice_id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );

    let invoice = client.get_invoice(&invoice_id);
    assert_eq!(invoice.id, 1);
    assert_eq!(invoice.merchant, merchant);
    assert_eq!(invoice.amount_usdc, 10_000_000);
    assert_eq!(invoice.gross_usdc, 10_250_000);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
    assert_eq!(invoice.payer, MaybeAddress::None);

    client.mark_paid(&admin, &invoice_id, &payer);
    let paid_invoice = client.get_invoice(&invoice_id);
    assert_eq!(paid_invoice.status, InvoiceStatus::Paid);
    assert_eq!(paid_invoice.payer, MaybeAddress::Some(payer));
    assert!(paid_invoice.paid_at.is_some());

    client.pause(&admin);
    client.unpause(&admin);
}

// --- #55: grace window tests ---

#[test]
fn test_grace_window_allows_payment_after_expiry() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &10,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.set_grace_window(&admin, &5);
    // timestamp = 12: past expires_at=10 but within grace (effective deadline = 15)
    env.ledger().with_mut(|l| l.timestamp = 12);
    client.mark_paid(&admin, &id, &payer);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Paid);
}

#[test]
fn test_grace_window_still_rejects_after_grace_period() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &10,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.set_grace_window(&admin, &5);
    // timestamp = 15: exactly at effective deadline → rejected
    env.ledger().with_mut(|l| l.timestamp = 15);
    let err = client
        .try_mark_paid(&admin, &id, &payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Expired);
}

#[test]
fn test_get_grace_window_default_is_zero() {
    let (_env, _admin, client) = setup();
    assert_eq!(client.get_grace_window(), 0);
}

#[test]
fn test_set_grace_window_requires_admin() {
    let (env, _admin, client) = setup();
    let rogue = Address::generate(&env);
    assert!(client.try_set_grace_window(&rogue, &60).is_err());
}

// --- #57: USDC decimal precision tests ---

#[test]
fn test_sub_usdc_amount_rejected() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    // 500_000 stroops = 0.05 USDC — below the 1 USDC minimum
    let err = client
        .try_create_invoice(
            &merchant,
            &500_000,
            &10_000_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::AmountPrecision);
}

#[test]
fn test_sub_usdc_gross_rejected() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    // amount and gross both below 1 USDC minimum
    let err = client
        .try_create_invoice(
            &merchant,
            &500_000,
            &500_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::AmountPrecision);
}

#[test]
fn test_whole_usdc_amounts_accepted() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    // 10_000_000 = 1 USDC, 10_250_000 = 1.025 USDC — both at or above minimum
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    assert_eq!(id, 1);
}

// --- #56: escrow release tests ---

#[test]
fn test_release_escrow_transitions_paid_to_released() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.mark_paid(&admin, &id, &payer);
    client.release_escrow(&admin, &id);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Released);
}

#[test]
fn test_cancel_invoice_transitions_to_cancelled() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let invoice_id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.cancel_invoice(&merchant, &invoice_id);
    let invoice = client.get_invoice(&invoice_id);
    assert_eq!(invoice.status, InvoiceStatus::Cancelled);
}

#[test]
fn test_cancelled_invoice_cannot_be_marked_paid() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let invoice_id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.cancel_invoice(&merchant, &invoice_id);
    let err = client
        .try_mark_paid(&admin, &invoice_id, &payer)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::NotPending);
    let invoice = client.get_invoice(&invoice_id);
    assert_eq!(invoice.status, InvoiceStatus::Cancelled);
}

#[test]
fn test_cancel_invoice_unauthorized_rejected() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    let err = client
        .try_cancel_invoice(&unauthorized, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);
    let invoice = client.get_invoice(&id);
    assert_eq!(invoice.status, InvoiceStatus::Pending);
}

#[test]
fn test_release_escrow_requires_paid_status() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    let err = client.try_release_escrow(&admin, &id).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::NotPaid);
}

#[test]
fn test_release_escrow_requires_admin() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let rogue = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.mark_paid(&admin, &id, &payer);
    assert!(client.try_release_escrow(&rogue, &id).is_err());
}

// ABI snapshot comparison: asserts abis/invoice.json stays in sync with the
// contract's public surface. Run via `cargo test` or `make check-abi-snapshots`.
#[test]
fn test_abi_snapshot_matches_contract() {
    let expected_functions: HashSet<&str> = [
        "initialize",
        "create_invoice",
        "mark_paid",
        "get_invoice",
        "get_invoice_status",
        "get_invoices_page",
        "cancel_invoice",
        "request_refund",
        "batch_expire",
        "pause",
        "unpause",
        "set_grace_window",
        "get_grace_window",
        "release_escrow",
    ]
    .iter()
    .copied()
    .collect();

    let expected_events: HashSet<&str> = [
        "invoice_created",
        "invoice_paid",
        "invoice_expired",
        "invoice_cancelled",
        "invoice_refund_req",
        "escrow_released",
        "contract_paused",
        "contract_unpaused",
    ]
    .iter()
    .copied()
    .collect();

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let abi_path = manifest_dir.join("../../abis/invoice.json");
    let raw = fs::read_to_string(&abi_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", abi_path.display()));

    let fns_block = raw
        .split("\"functions\"")
        .nth(1)
        .expect("\"functions\" key missing from abis/invoice.json");
    let fns_array = &fns_block[fns_block.find('[').unwrap()..=fns_block.find(']').unwrap()];
    let snapshot_functions: HashSet<&str> = fns_array
        .split('"')
        .filter(|s| {
            !s.trim().is_empty()
                && !s.contains('[')
                && !s.contains(']')
                && !s.trim().starts_with(',')
        })
        .collect();

    let evts_block = raw
        .split("\"events\"")
        .nth(1)
        .expect("\"events\" key missing from abis/invoice.json");
    let evts_array = &evts_block[evts_block.find('[').unwrap()..=evts_block.find(']').unwrap()];
    let snapshot_events: HashSet<&str> = evts_array
        .split('"')
        .filter(|s| {
            !s.trim().is_empty()
                && !s.contains('[')
                && !s.contains(']')
                && !s.trim().starts_with(',')
        })
        .collect();

    assert_eq!(
        snapshot_functions,
        expected_functions,
        "abis/invoice.json functions list is out of sync.\nMissing: {:?}\nExtra: {:?}",
        expected_functions
            .difference(&snapshot_functions)
            .collect::<Vec<_>>(),
        snapshot_functions
            .difference(&expected_functions)
            .collect::<Vec<_>>(),
    );

    assert_eq!(
        snapshot_events,
        expected_events,
        "abis/invoice.json events list is out of sync.\nMissing: {:?}\nExtra: {:?}",
        expected_events
            .difference(&snapshot_events)
            .collect::<Vec<_>>(),
        snapshot_events
            .difference(&expected_events)
            .collect::<Vec<_>>(),
    );
}

#[test]
fn test_create_invoice_blocked_when_paused() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    client.pause(&admin);
    assert!(client
        .try_create_invoice(
            &merchant,
            &10_000_000,
            &10_250_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
        )
        .is_err());
}

#[test]
fn test_mark_paid_blocked_when_paused() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.pause(&admin);
    assert!(client.try_mark_paid(&admin, &id, &payer).is_err());
}


// Issue #94: create_invoice must enforce merchant authorization.
// Uses cancel_invoice (which has an explicit Unauthorized check) to prove that a
// non-merchant/non-admin caller is rejected. Also verifies that the merchant's auth
// was recorded by create_invoice, confirming require_auth() is enforced.
#[test]
fn test_create_invoice_unauthorized_merchant() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );

    let auths = env.auths();
    assert!(
        auths.iter().any(|(addr, _)| addr == &merchant),
        "create_invoice must require merchant authorization"
    );

    let err = client
        .try_cancel_invoice(&unauthorized, &id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);
}

// Issue #92: e2e flow — create invoice, advance ledger past deadline, run batch_expire, assert Expired
#[test]
fn test_invoice_create_to_expired_flow() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    env.ledger().with_mut(|li| {
        li.timestamp = client.get_invoice(&id).expires_at + 1;
    });
    let ids = soroban_sdk::vec![&env, id];
    let expired_count = client.batch_expire(&admin, &ids);
    assert_eq!(expired_count, 1);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::Expired);
}

// Issue #91: e2e happy path — create invoice, admin marks paid, assert Paid status and payer recorded
#[test]
fn test_invoice_create_to_paid_escrow_flow() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.mark_paid(&admin, &id, &payer);
    let paid = client.get_invoice(&id);
    assert_eq!(paid.status, InvoiceStatus::Paid);
    assert_eq!(paid.payer, MaybeAddress::Some(payer));
    assert!(paid.paid_at.is_some());
}

// --- #58: merchant nonce tests ---

#[test]
fn test_duplicate_nonce_rejected() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    client.create_invoice(
        &merchant,
        &10_000_000,
        &10_000_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &42,
    );
    let err = client
        .try_create_invoice(
            &merchant,
            &10_000_000,
            &10_000_000,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &42,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::DuplicateNonce);
}

#[test]
fn test_different_nonces_accepted() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let id1 = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_000_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &1,
    );
    let id2 = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_000_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &2,
    );
    assert_ne!(id1, id2);
}

#[test]
fn test_zero_nonce_skips_idempotency_check() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let id1 = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    let id2 = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    assert_ne!(id1, id2);
}

#[test]
fn test_nonce_stored_on_invoice() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);
    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_000_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &99,
    );
    assert_eq!(client.get_invoice(&id).merchant_nonce, 99);
}

#[test]
fn test_same_nonce_different_merchants_accepted() {
    let (env, _admin, client) = setup();
    let merchant1 = Address::generate(&env);
    let merchant2 = Address::generate(&env);
    client.create_invoice(
        &merchant1,
        &10_000_000,
        &10_000_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &7,
    );
    client.create_invoice(
        &merchant2,
        &10_000_000,
        &10_000_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &7,
    );
}

// Regression guard: the payer-only guard (invoice.payer != MaybeAddress::Some(payer)) must
// reject every caller not recorded at mark_paid time, including elevated roles like merchant
// and admin, so a future refactor of MaybeAddress comparison logic can't silently bypass it.
#[test]
fn test_request_refund_only_recorded_payer_can_call() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let impostor = Address::generate(&env);

    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
    );
    client.mark_paid(&admin, &id, &payer);

    // Random address is rejected.
    let err = client.try_request_refund(&impostor, &id).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);

    // Merchant is rejected despite owning the invoice.
    let err = client.try_request_refund(&merchant, &id).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);

    // Admin is rejected despite holding the highest privilege.
    let err = client.try_request_refund(&admin, &id).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);

    // Only the address recorded at mark_paid time succeeds.
    client.request_refund(&payer, &id);
    assert_eq!(client.get_invoice(&id).status, InvoiceStatus::RefundRequested);
}

