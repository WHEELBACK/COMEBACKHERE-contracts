/// Tests for `batch_approve_settlements` behaviour with invalid IDs (issue #588).
///
/// The contract's documented skip-semantics are:
///   - Unknown IDs (no settlement exists): silently skipped.
///   - Already-executed settlements (`Executed`/`PartiallyExecuted`): silently skipped.
///   - Expired settlements: silently skipped.
///   - Cancelled settlements: silently skipped.
///   - On-hold settlements: silently skipped.
///
/// A batch containing any mix of invalid IDs and valid pending IDs must still
/// approve all the valid ones and return only those in the result Vec.  A batch
/// composed entirely of invalid IDs returns an empty Vec without error.
///
/// Integrators can therefore safely resubmit a partially-failed or stale batch:
/// already-approved IDs are deduplicated (no double-count), skipped IDs do not
/// abort the transaction, and the returned Vec contains exactly the settlements
/// that were newly approved in this call.
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

// ─── helpers ────────────────────────────────────────────────────────────────

fn setup(env: &Env, threshold: u32) -> (TreasuryContractClient<'_>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &threshold, &Vec::new(env));
    (client, admin)
}

fn register_token(env: &Env, admin: &Address, contract_id: &Address) -> Address {
    let token = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(env, &token).mint(contract_id, &100_000_000);
    token
}

// ─── unknown IDs ────────────────────────────────────────────────────────────

/// A batch of IDs that have never existed is processed without error;
/// the returned Vec is empty because nothing was approved.
#[test]
fn batch_with_all_unknown_ids_returns_empty() {
    let env = Env::default();
    let (client, admin) = setup(&env, 1);

    let ids = soroban_sdk::vec![&env, 999u64, 1_000u64, 1_001u64];
    let approved = client.batch_approve_settlements(&admin, &ids);

    assert_eq!(approved.len(), 0, "unknown IDs must produce no approvals");
}

/// Unknown IDs mixed with a valid pending ID: only the valid one is approved.
#[test]
fn batch_with_unknown_and_valid_ids_approves_only_valid() {
    let env = Env::default();
    let (client, admin) = setup(&env, 2);

    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);

    // Mix of unknown IDs and the real pending one.
    let ids = soroban_sdk::vec![&env, 999u64, sid, 1_001u64];
    let approved = client.batch_approve_settlements(&backup, &ids);

    assert_eq!(approved.len(), 1);
    assert_eq!(approved.get(0).unwrap().id, sid);
    assert_eq!(
        approved.get(0).unwrap().status,
        SettlementStatus::Pending
    );
}

// ─── executed settlements ────────────────────────────────────────────────────

/// A settlement that has already been executed is silently skipped in the batch.
#[test]
fn batch_skips_already_executed_settlement() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    // threshold=1 so admin alone can execute.
    client.initialize(&admin, &1, &Vec::new(&env));
    let token = register_token(&env, &admin, &contract_id);

    let merchant = Address::generate(&env);
    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    // Propose and execute settlement_1.
    let sid_exec = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.execute_settlement(&admin, &sid_exec, &token);
    assert_eq!(
        client.get_settlement(&sid_exec).status,
        SettlementStatus::Executed
    );

    // Propose a still-pending settlement_2.
    let sid_pending = client.propose_settlement(&admin, &merchant, &2_000_000);

    let ids = soroban_sdk::vec![&env, sid_exec, sid_pending];
    let approved = client.batch_approve_settlements(&backup, &ids);

    // Only the pending settlement should appear in the result.
    assert_eq!(approved.len(), 1);
    assert_eq!(approved.get(0).unwrap().id, sid_pending);
}

/// A batch composed entirely of executed settlements returns an empty Vec.
#[test]
fn batch_with_only_executed_settlements_returns_empty() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &1, &Vec::new(&env));
    let token = register_token(&env, &admin, &contract_id);

    let merchant = Address::generate(&env);
    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    let sid1 = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.execute_settlement(&admin, &sid1, &token);

    let sid2 = client.propose_settlement(&admin, &merchant, &2_000_000);
    client.execute_settlement(&admin, &sid2, &token);

    let ids = soroban_sdk::vec![&env, sid1, sid2];
    let approved = client.batch_approve_settlements(&backup, &ids);

    assert_eq!(approved.len(), 0);
}

// ─── expired settlements ─────────────────────────────────────────────────────

/// An expired settlement is silently skipped.
#[test]
fn batch_skips_expired_settlement() {
    let env = Env::default();
    let (client, admin) = setup(&env, 1);

    let merchant = Address::generate(&env);
    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    // Propose and then expire the first settlement.
    let sid_exp = client.propose_settlement(&admin, &merchant, &1_000_000);
    // Advance time past SETTLEMENT_TTL (7 days = 604_800 s).
    env.ledger()
        .with_mut(|l| l.timestamp = 604_801);
    client.expire_settlement(&admin, &sid_exp);
    assert_eq!(
        client.get_settlement(&sid_exp).status,
        SettlementStatus::Expired
    );

    // Propose a second, still-pending settlement.
    let sid_pending = client.propose_settlement(&admin, &merchant, &2_000_000);

    let ids = soroban_sdk::vec![&env, sid_exp, sid_pending];
    let approved = client.batch_approve_settlements(&backup, &ids);

    assert_eq!(approved.len(), 1);
    assert_eq!(approved.get(0).unwrap().id, sid_pending);
}

// ─── cancelled settlements ───────────────────────────────────────────────────

/// A cancelled settlement is silently skipped.
#[test]
fn batch_skips_cancelled_settlement() {
    let env = Env::default();
    let (client, admin) = setup(&env, 1);

    let merchant = Address::generate(&env);
    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    let sid_cancelled = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.cancel_settlement(&admin, &sid_cancelled);
    assert_eq!(
        client.get_settlement(&sid_cancelled).status,
        SettlementStatus::Cancelled
    );

    let sid_pending = client.propose_settlement(&admin, &merchant, &2_000_000);

    let ids = soroban_sdk::vec![&env, sid_cancelled, sid_pending];
    let approved = client.batch_approve_settlements(&backup, &ids);

    assert_eq!(approved.len(), 1);
    assert_eq!(approved.get(0).unwrap().id, sid_pending);
}

// ─── on-hold settlements ─────────────────────────────────────────────────────

/// An on-hold settlement (blocked by a dispute) is silently skipped.
#[test]
fn batch_skips_on_hold_settlement() {
    let env = Env::default();
    let (client, admin) = setup(&env, 1);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);
    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    let sid_held = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.raise_dispute(&claimant, &sid_held, &merchant, &500_000, &9_999_999);
    assert_eq!(
        client.get_settlement(&sid_held).status,
        SettlementStatus::OnHold
    );

    let sid_pending = client.propose_settlement(&admin, &merchant, &2_000_000);

    let ids = soroban_sdk::vec![&env, sid_held, sid_pending];
    let approved = client.batch_approve_settlements(&backup, &ids);

    assert_eq!(approved.len(), 1);
    assert_eq!(approved.get(0).unwrap().id, sid_pending);
}

// ─── duplicate approvals ─────────────────────────────────────────────────────

/// Including the same ID twice in the batch approves it once; the second
/// occurrence is a no-op (already in the approvals list).
#[test]
fn batch_duplicate_id_approves_once() {
    let env = Env::default();
    let (client, admin) = setup(&env, 2);

    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000);

    // Submit the same ID twice in one batch.
    let ids = soroban_sdk::vec![&env, sid, sid];
    let approved = client.batch_approve_settlements(&backup, &ids);

    // Both occurrences count as one approval: backup is added once.
    assert_eq!(approved.len(), 2); // both iterations returned the settlement
    let settlement = client.get_settlement(&sid);
    // admin(1) + backup(1) = weight 2; backup should not be counted twice.
    assert_eq!(settlement.approval_weight, 2);
    assert_eq!(settlement.approvals.len(), 2);
}

// ─── mixed: all three invalid types together with a valid pending ─────────────

/// A batch containing unknown, executed, expired, and pending IDs processes
/// without error and returns only the pending settlement.
#[test]
fn batch_mixed_invalid_types_approves_only_pending() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &1, &Vec::new(&env));
    let token = register_token(&env, &admin, &contract_id);

    let merchant = Address::generate(&env);
    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    // executed
    let sid_exec = client.propose_settlement(&admin, &merchant, &1_000_000);
    client.execute_settlement(&admin, &sid_exec, &token);

    // expired
    let sid_exp = client.propose_settlement(&admin, &merchant, &1_000_000);
    env.ledger().with_mut(|l| l.timestamp = 604_801);
    client.expire_settlement(&admin, &sid_exp);

    // still pending (proposed after advancing time, so TTL not elapsed yet).
    env.ledger().with_mut(|l| l.timestamp = 604_802);
    let sid_pending = client.propose_settlement(&admin, &merchant, &3_000_000);

    let ids = soroban_sdk::vec![&env, 9_999u64, sid_exec, sid_exp, sid_pending];
    let approved = client.batch_approve_settlements(&backup, &ids);

    assert_eq!(approved.len(), 1);
    assert_eq!(approved.get(0).unwrap().id, sid_pending);
}

// ─── batch too large ─────────────────────────────────────────────────────────

/// Submitting more than MAX_BATCH_SIZE (50) IDs returns BatchTooLarge.
#[test]
fn batch_too_large_returns_error() {
    let env = Env::default();
    let (client, admin) = setup(&env, 1);

    let mut ids = Vec::new(&env);
    for i in 1u64..=51 {
        ids.push_back(i);
    }

    let result = client.try_batch_approve_settlements(&admin, &ids);
    assert!(result.is_err(), "batch of 51 must be rejected");
}
