//! Regression coverage for the signer-rotation weight time-of-check-to-time-of-use
//! gap: `approve_signer_rotation` used to read `old_signer`'s *current* weight at
//! the moment the approval threshold was met, rather than the weight `old_signer`
//! had when the rotation was proposed. That meant a `set_signer`/`remove_signer`
//! call landing between `propose_signer_rotation` and the approval that finally
//! crosses the threshold could change - or zero out - the weight `new_signer`
//! ends up with, purely as a side effect of unrelated administrative traffic
//! racing the rotation.
//!
//! The fix snapshots `old_signer`'s weight into
//! `SignerRotationProposal::captured_old_weight` at proposal time and uses that
//! snapshot at execution time. These tests pin that behavior down explicitly.

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use treasury::{RotationStatus, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env) -> (TreasuryContractClient<'static>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &2, &Vec::new(env));
    (client, admin)
}

/// Baseline: with no concurrent weight change, new_signer receives exactly the
/// weight old_signer had at proposal time.
#[test]
fn new_signer_receives_weight_captured_at_proposal_time() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let other_signer = Address::generate(&env);
    client.set_signer(&admin, &old_signer, &5);
    client.set_signer(&admin, &other_signer, &5);

    let rid = client.propose_signer_rotation(&old_signer, &old_signer, &new_signer);
    let proposal = client.approve_signer_rotation(&other_signer, &rid);

    assert_eq!(proposal.status, RotationStatus::Executed);
    assert_eq!(proposal.captured_old_weight, 5);
    assert_eq!(client.get_signer_weight(&new_signer), 5);
}

/// old_signer's weight is *reduced* by a separate `set_signer` call after the
/// rotation is proposed but before the second approval crosses the threshold.
/// new_signer must still receive the weight captured at proposal time (5), not
/// the reduced weight (1) that old_signer holds when execution actually happens.
#[test]
fn concurrent_weight_reduction_does_not_affect_executed_rotation() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let other_signer = Address::generate(&env);
    client.set_signer(&admin, &old_signer, &5);
    client.set_signer(&admin, &other_signer, &5);

    let rid = client.propose_signer_rotation(&old_signer, &old_signer, &new_signer);

    // A separate admin action races the pending rotation: old_signer's weight
    // is slashed to 1 before the rotation executes.
    client.set_signer(&admin, &old_signer, &1);

    let proposal = client.approve_signer_rotation(&other_signer, &rid);

    assert_eq!(proposal.status, RotationStatus::Executed);
    assert_eq!(
        proposal.captured_old_weight, 5,
        "captured weight must reflect proposal time, not execution time"
    );
    assert_eq!(
        client.get_signer_weight(&new_signer),
        5,
        "new_signer must receive the weight old_signer had when the rotation was proposed"
    );
}

/// old_signer is fully *removed* (weight -> implicitly 0, entry deleted) by a
/// separate transaction while the rotation is pending. new_signer must still
/// receive the non-zero weight captured at proposal time, not zero.
#[test]
fn old_signer_removed_before_execution_does_not_zero_new_signer_weight() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let other_signer = Address::generate(&env);
    client.set_signer(&admin, &old_signer, &5);
    client.set_signer(&admin, &other_signer, &5);

    let rid = client.propose_signer_rotation(&old_signer, &old_signer, &new_signer);

    // old_signer is removed outright before the rotation executes.
    client.remove_signer(&admin, &old_signer);
    assert_eq!(client.get_signer_weight(&old_signer), 0);

    let proposal = client.approve_signer_rotation(&other_signer, &rid);

    assert_eq!(proposal.status, RotationStatus::Executed);
    assert_eq!(proposal.captured_old_weight, 5);
    assert_eq!(
        client.get_signer_weight(&new_signer),
        5,
        "removing old_signer mid-flight must not zero out new_signer's rotated-in weight"
    );
}
