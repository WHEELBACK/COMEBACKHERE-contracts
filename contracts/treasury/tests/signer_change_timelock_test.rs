//! Tests for the timelocked signer/threshold-configuration change flow (#447).
//!
//! Covers:
//! - A queued change cannot be executed before its delay elapses.
//! - A queued change can be cancelled by the admin before it takes effect.
//! - A cancelled change cannot be executed.
//! - An already-executed change cannot be cancelled.
//! - Only admin can propose / cancel.
//! - `execute_signer_change` correctly applies each `SignerChangeKind`.
//! - `get_signer_change` returns `None` for an unknown id.

use soroban_sdk::{testutils::Address as _, testutils::Ledger as _, Address, Env, Vec};
use treasury::{
    SignerChangeKind, SignerChangeStatus, TreasuryContract, TreasuryContractClient, TreasuryError,
};

/// Minimum delay between proposal and execution.
const TIMELOCK_SECS: u64 = 24 * 60 * 60;

fn setup(env: &Env) -> (TreasuryContractClient<'_>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &Vec::new(env));
    (client, admin)
}

// ── propose / read ────────────────────────────────────────────────────────────

#[test]
fn propose_returns_incrementing_ids() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let signer = Address::generate(&env);

    let id1 = client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(signer.clone(), 2));
    let id2 = client.propose_signer_change(&admin, &SignerChangeKind::RemoveSigner(signer));
    assert_eq!(id1, 1);
    assert_eq!(id2, 2);
}

#[test]
fn get_signer_change_returns_none_for_unknown_id() {
    let env = Env::default();
    let (client, _admin) = setup(&env);

    let result = client.get_signer_change(&999u64);
    assert!(result.is_none());
}

#[test]
fn proposal_is_initially_pending_with_correct_executable_at() {
    let env = Env::default();
    env.ledger().set_timestamp(0);
    let (client, admin) = setup(&env);
    let signer = Address::generate(&env);

    let cid = client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(signer, 3));
    let proposal = client.get_signer_change(&cid).expect("proposal must exist");

    assert_eq!(proposal.status, SignerChangeStatus::Pending);
    assert_eq!(proposal.executable_at, proposal.proposed_at + TIMELOCK_SECS);
}

// ── cannot execute before delay ───────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #38)")]
fn execute_before_delay_is_rejected() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let signer = Address::generate(&env);

    let cid = client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(signer, 2));
    // Advance time but not past the full delay.
    env.ledger().set_timestamp(1_000 + TIMELOCK_SECS - 1);
    client.execute_signer_change(&admin, &cid);
}

#[test]
fn execute_exactly_at_delay_boundary_succeeds() {
    let env = Env::default();
    env.ledger().set_timestamp(1_000);
    let (client, admin) = setup(&env);
    let signer = Address::generate(&env);

    let cid = client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(signer.clone(), 5));
    env.ledger().set_timestamp(1_000 + TIMELOCK_SECS);
    let proposal = client.execute_signer_change(&admin, &cid);

    assert_eq!(proposal.status, SignerChangeStatus::Executed);
    assert_eq!(client.get_signer_weight(&signer), 5);
}

// ── cancel ────────────────────────────────────────────────────────────────────

#[test]
fn admin_can_cancel_pending_change() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let signer = Address::generate(&env);

    let cid = client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(signer, 2));
    let proposal = client.cancel_signer_change(&admin, &cid);

    assert_eq!(proposal.status, SignerChangeStatus::Cancelled);
}

#[test]
#[should_panic(expected = "Error(Contract, #40)")]
fn cancelled_change_cannot_be_executed() {
    let env = Env::default();
    env.ledger().set_timestamp(0);
    let (client, admin) = setup(&env);
    let signer = Address::generate(&env);

    let cid = client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(signer, 2));
    client.cancel_signer_change(&admin, &cid);

    // Even after the delay has elapsed the cancelled proposal cannot be executed.
    env.ledger().set_timestamp(TIMELOCK_SECS + 1);
    client.execute_signer_change(&admin, &cid);
}

#[test]
#[should_panic(expected = "Error(Contract, #40)")]
fn executed_change_cannot_be_cancelled() {
    let env = Env::default();
    env.ledger().set_timestamp(0);
    let (client, admin) = setup(&env);
    let signer = Address::generate(&env);

    let cid = client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(signer, 2));
    env.ledger().set_timestamp(TIMELOCK_SECS);
    client.execute_signer_change(&admin, &cid);

    // Attempt to cancel an already-executed proposal must fail.
    client.cancel_signer_change(&admin, &cid);
}

#[test]
#[should_panic(expected = "Error(Contract, #40)")]
fn executed_change_cannot_be_executed_again() {
    let env = Env::default();
    env.ledger().set_timestamp(0);
    let (client, admin) = setup(&env);
    let signer = Address::generate(&env);

    let cid = client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(signer, 2));
    env.ledger().set_timestamp(TIMELOCK_SECS);
    client.execute_signer_change(&admin, &cid);
    client.execute_signer_change(&admin, &cid);
}

// ── authorization ─────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn non_admin_cannot_propose_change() {
    let env = Env::default();
    let (client, _admin) = setup(&env);
    let attacker = Address::generate(&env);
    let target = Address::generate(&env);

    client.propose_signer_change(&attacker, &SignerChangeKind::SetSigner(target, 1));
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn non_admin_cannot_cancel_change() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let attacker = Address::generate(&env);
    let signer = Address::generate(&env);

    let cid = client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(signer, 1));
    client.cancel_signer_change(&attacker, &cid);
}

#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn non_admin_cannot_execute_change() {
    let env = Env::default();
    env.ledger().set_timestamp(0);
    let (client, admin) = setup(&env);
    let attacker = Address::generate(&env);
    let signer = Address::generate(&env);

    let cid = client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(signer, 1));
    env.ledger().set_timestamp(TIMELOCK_SECS + 1);
    client.execute_signer_change(&attacker, &cid);
}

// ── unknown id ────────────────────────────────────────────────────────────────

#[test]
#[should_panic(expected = "Error(Contract, #39)")]
fn execute_unknown_id_panics() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    client.execute_signer_change(&admin, &999u64);
}

#[test]
#[should_panic(expected = "Error(Contract, #39)")]
fn cancel_unknown_id_panics() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    client.cancel_signer_change(&admin, &999u64);
}

// ── correct application of each SignerChangeKind ─────────────────────────────

#[test]
fn set_signer_change_adds_signer_weight() {
    let env = Env::default();
    env.ledger().set_timestamp(0);
    let (client, admin) = setup(&env);
    let new_signer = Address::generate(&env);

    assert_eq!(client.get_signer_weight(&new_signer), 0);

    let cid =
        client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(new_signer.clone(), 3));
    env.ledger().set_timestamp(TIMELOCK_SECS);
    client.execute_signer_change(&admin, &cid);

    assert_eq!(client.get_signer_weight(&new_signer), 3);
}

#[test]
fn set_signer_change_with_zero_weight_deactivates_signer() {
    let env = Env::default();
    env.ledger().set_timestamp(0);
    let (client, admin) = setup(&env);
    let signer = Address::generate(&env);

    // First activate the signer via the immediate path.
    client.set_signer(&admin, &signer, &4);
    assert_eq!(client.get_signer_weight(&signer), 4);

    // Then queue a timelocked removal via SetSigner weight=0.
    let cid = client.propose_signer_change(&admin, &SignerChangeKind::SetSigner(signer.clone(), 0));
    env.ledger().set_timestamp(TIMELOCK_SECS);
    client.execute_signer_change(&admin, &cid);

    assert_eq!(client.get_signer_weight(&signer), 0);
}

#[test]
fn remove_signer_change_removes_signer() {
    let env = Env::default();
    env.ledger().set_timestamp(0);
    let (client, admin) = setup(&env);
    let signer = Address::generate(&env);

    client.set_signer(&admin, &signer, &2);
    assert_eq!(client.get_signer_weight(&signer), 2);

    let cid = client.propose_signer_change(&admin, &SignerChangeKind::RemoveSigner(signer.clone()));
    env.ledger().set_timestamp(TIMELOCK_SECS);
    client.execute_signer_change(&admin, &cid);

    assert_eq!(client.get_signer_weight(&signer), 0);
}

#[test]
fn update_threshold_change_applies_new_threshold() {
    let env = Env::default();
    env.ledger().set_timestamp(0);
    // Initialize with threshold=1 and an extra signer so total weight is 3.
    let admin = Address::generate(&env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    let extra = Address::generate(&env);
    let signers: soroban_sdk::Vec<(Address, u32)> = {
        let mut v = soroban_sdk::Vec::new(&env);
        v.push_back((extra, 2u32));
        v
    };
    env.mock_all_auths();
    client.initialize(&admin, &1, &signers);

    // admin weight=1 + extra weight=2 = total 3; propose threshold=3.
    let cid = client.propose_signer_change(&admin, &SignerChangeKind::UpdateThreshold(3));
    env.ledger().set_timestamp(TIMELOCK_SECS);
    client.execute_signer_change(&admin, &cid);

    // The settlement below with only admin approving (weight=1) should NOT execute
    // because the threshold is now 3.
    let merchant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &1_000);
    let settlement = client.approve_settlement(&admin, &sid);
    // approval_weight is 1 (only admin), threshold is 3 → not yet executed.
    assert_eq!(settlement.approval_weight, 1);
    assert_eq!(settlement.status, treasury::SettlementStatus::Pending);
}

#[test]
#[should_panic(expected = "Error(Contract, #2)")]
fn update_threshold_change_rejects_zero() {
    let env = Env::default();
    env.ledger().set_timestamp(0);
    let (client, admin) = setup(&env);

    let cid = client.propose_signer_change(&admin, &SignerChangeKind::UpdateThreshold(0));
    env.ledger().set_timestamp(TIMELOCK_SECS);
    client.execute_signer_change(&admin, &cid);
}

#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn update_threshold_change_rejects_unreachable_threshold() {
    let env = Env::default();
    env.ledger().set_timestamp(0);
    let (client, admin) = setup(&env);

    // Total weight is just admin's 1; threshold 10 is unreachable.
    let cid = client.propose_signer_change(&admin, &SignerChangeKind::UpdateThreshold(10));
    env.ledger().set_timestamp(TIMELOCK_SECS);
    client.execute_signer_change(&admin, &cid);
}
