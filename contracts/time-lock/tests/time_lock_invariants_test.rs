//! Issue #945: strict invariant verification for the time-lock contract.
//!
//! These tests corrupt storage directly (simulating a bad upgrade or a
//! hostile write) and assert that every state transition rejects the bad
//! record with a typed error instead of panicking or moving funds under an
//! invalid identity. Boundary matrices act as the contract's fuzz suite.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};
use time_lock::{
    DataKey, LockStatus, Role, TimeLock, TimeLockContract, TimeLockContractClient, TimeLockError,
    TimelockProposal, TimelockStatus,
};

fn setup<'a>(env: &'a Env) -> (TimeLockContractClient<'a>, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let cid = env.register(TimeLockContract, ());
    let client = TimeLockContractClient::new(env, &cid);
    client.initialize(&admin);
    (client, admin, cid)
}

fn advance_to(env: &Env, timestamp: u64) {
    env.ledger().with_mut(|li| li.timestamp = timestamp);
}

fn valid_lock(id: u64, beneficiary: &Address, depositor: &Address) -> TimeLock {
    TimeLock {
        id,
        depositor: depositor.clone(),
        beneficiary: beneficiary.clone(),
        amount: 1_000,
        release_at: 100,
        status: LockStatus::Active,
    }
}

fn valid_proposal(target: &Address) -> TimelockProposal {
    TimelockProposal {
        target: target.clone(),
        delay_secs: 10,
        proposed_at: 100,
        executable_at: 110,
        status: TimelockStatus::Active,
    }
}

// ---------------------------------------------------------------------------
// Healthy state
// ---------------------------------------------------------------------------

#[test]
fn verify_invariants_holds_across_the_whole_lifecycle() {
    let env = Env::default();
    let (client, admin, _cid) = setup(&env);
    let beneficiary = Address::generate(&env);

    assert_eq!(client.try_verify_invariants(), Ok(Ok(())));

    let lock_id = client.create_lock(&admin, &beneficiary, &1_000, &10);
    assert_eq!(client.try_verify_invariants(), Ok(Ok(())));

    advance_to(&env, 200);
    client.release(&beneficiary, &lock_id);
    assert_eq!(client.try_verify_invariants(), Ok(Ok(())));

    client.propose_timelock(&admin, &beneficiary, &10);
    assert_eq!(client.try_verify_invariants(), Ok(Ok(())));
    advance_to(&env, 300);
    client.execute_timelock(&admin, &beneficiary);
    assert_eq!(client.try_verify_invariants(), Ok(Ok(())));
}

// ---------------------------------------------------------------------------
// Instance-level invariants
// ---------------------------------------------------------------------------

#[test]
fn missing_lock_counter_is_reported_instead_of_restarting_at_zero() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let beneficiary = Address::generate(&env);

    env.as_contract(&cid, || {
        env.storage().instance().remove(&DataKey::LockCount);
    });

    assert_eq!(
        client.try_verify_invariants(),
        Err(Ok(TimeLockError::StateInconsistency))
    );
    assert_eq!(
        client.try_create_lock(&admin, &beneficiary, &1_000, &10),
        Err(Ok(TimeLockError::StateInconsistency))
    );
}

#[test]
fn lock_counter_overflow_returns_typed_error() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let beneficiary = Address::generate(&env);

    env.as_contract(&cid, || {
        env.storage().instance().set(&DataKey::LockCount, &u64::MAX);
    });

    assert_eq!(
        client.try_create_lock(&admin, &beneficiary, &1_000, &10),
        Err(Ok(TimeLockError::LockCountOverflow))
    );
}

#[test]
fn counter_desync_cannot_overwrite_an_existing_lock() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let beneficiary = Address::generate(&env);
    let lock_id = client.create_lock(&admin, &beneficiary, &1_000, &10);

    // Roll the counter back without touching the lock record.
    env.as_contract(&cid, || {
        env.storage().instance().set(&DataKey::LockCount, &0u64);
    });

    assert_eq!(
        client.try_create_lock(&admin, &beneficiary, &9_999, &10),
        Err(Ok(TimeLockError::StateInconsistency))
    );
    assert_eq!(client.get_lock(&lock_id).amount, 1_000);
}

#[test]
fn demoting_the_stored_admin_is_rejected() {
    let env = Env::default();
    let (client, admin, _cid) = setup(&env);
    let beneficiary = Address::generate(&env);

    for role in [Role::None, Role::Operator] {
        assert_eq!(
            client.try_set_role(&admin, &admin, &role),
            Err(Ok(TimeLockError::AdminRoleLoss))
        );
    }
    assert_eq!(client.get_role(&admin), Role::Admin);
    // The contract is still operable after the rejected demotions.
    assert!(client.try_create_lock(&admin, &beneficiary, &1, &1).is_ok());
}

#[test]
fn corrupted_admin_role_blocks_admin_operations() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);

    env.as_contract(&cid, || {
        env.storage()
            .instance()
            .set(&DataKey::Role(admin.clone()), &Role::None);
    });

    assert_eq!(
        client.try_verify_invariants(),
        Err(Ok(TimeLockError::StateInconsistency))
    );
    assert_eq!(
        client.try_pause(&admin),
        Err(Ok(TimeLockError::StateInconsistency))
    );
}

// ---------------------------------------------------------------------------
// Lock-record invariants
// ---------------------------------------------------------------------------

#[test]
fn malformed_lock_records_are_rejected_by_every_transition() {
    // Build the malformed field combinations once; each case gets its own env
    // so Soroban address handles never cross environment boundaries.
    let template_env = Env::default();
    let depositor = Address::generate(&template_env);
    let beneficiary = Address::generate(&template_env);

    let mut cases = std::vec![
        valid_lock(99, &beneficiary, &depositor), // mismatched ID
        valid_lock(0, &beneficiary, &depositor),  // zero ID
    ];
    let mut zero_amount = valid_lock(1, &beneficiary, &depositor);
    zero_amount.amount = 0;
    cases.push(zero_amount);
    let mut negative_amount = valid_lock(1, &beneficiary, &depositor);
    negative_amount.amount = -1;
    cases.push(negative_amount);
    let mut zero_release = valid_lock(1, &beneficiary, &depositor);
    zero_release.release_at = 0;
    cases.push(zero_release);

    for corrupt in cases {
        let case_env = Env::default();
        let (client, admin, cid) = setup(&case_env);
        // Rebuild the record with addresses from this env.
        let case_depositor = Address::generate(&case_env);
        let case_beneficiary = Address::generate(&case_env);
        let record = TimeLock {
            id: corrupt.id,
            depositor: case_depositor,
            beneficiary: case_beneficiary.clone(),
            amount: corrupt.amount,
            release_at: corrupt.release_at,
            status: LockStatus::Active,
        };
        case_env.as_contract(&cid, || {
            case_env
                .storage()
                .persistent()
                .set(&DataKey::Lock(1), &record);
        });

        assert_eq!(
            client.try_get_lock(&1),
            Err(Ok(TimeLockError::StateInconsistency)),
            "get_lock must reject id={} amount={} release_at={}",
            record.id,
            record.amount,
            record.release_at
        );
        assert_eq!(
            client.try_release(&case_beneficiary, &1),
            Err(Ok(TimeLockError::StateInconsistency))
        );
        assert_eq!(
            client.try_cancel_lock(&admin, &1),
            Err(Ok(TimeLockError::StateInconsistency))
        );
    }
}

#[test]
fn fuzz_id_mismatches_never_panic() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let depositor = Address::generate(&env);
    let beneficiary = Address::generate(&env);

    for stored_id in [0u64, 2, 42, u64::MAX] {
        for key_id in [1u64, 999, u64::MAX - 1] {
            let record = valid_lock(stored_id, &beneficiary, &depositor);
            env.as_contract(&cid, || {
                env.storage()
                    .persistent()
                    .set(&DataKey::Lock(key_id), &record);
            });
            let get_result = client.try_get_lock(&key_id);
            assert!(
                get_result.is_err(),
                "mismatched id {stored_id} stored under {key_id} must be rejected"
            );
            assert!(client.try_cancel_lock(&admin, &key_id).is_err());
        }
    }
}

// ---------------------------------------------------------------------------
// Proposal invariants
// ---------------------------------------------------------------------------

#[test]
fn corrupt_proposal_is_rejected_by_lifecycle_and_queries() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let target = Address::generate(&env);
    let wrong_target = Address::generate(&env);
    advance_to(&env, 500);

    let mut wrong_target_proposal = valid_proposal(&target);
    wrong_target_proposal.target = wrong_target;
    env.as_contract(&cid, || {
        env.storage().instance().set(
            &DataKey::TimelockProposal(target.clone()),
            &wrong_target_proposal,
        );
    });

    assert_eq!(
        client.try_execute_timelock(&admin, &target),
        Err(Ok(TimeLockError::StateInconsistency))
    );
    assert_eq!(
        client.try_cancel_timelock(&admin, &target),
        Err(Ok(TimeLockError::StateInconsistency))
    );
    assert!(client.get_timelock(&target).is_none());
    assert!(!client.has_active_timelock(&target));
}

#[test]
fn corrupt_proposal_timing_cannot_authorize_early_execution() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let target = Address::generate(&env);
    advance_to(&env, 105);

    let mut early_proposal = valid_proposal(&target);
    // proposed_at + delay_secs == 110, but the stored executable_at says 101.
    early_proposal.executable_at = 101;
    env.as_contract(&cid, || {
        env.storage()
            .instance()
            .set(&DataKey::TimelockProposal(target.clone()), &early_proposal);
    });

    assert_eq!(
        client.try_execute_timelock(&admin, &target),
        Err(Ok(TimeLockError::StateInconsistency))
    );
}

#[test]
fn fuzz_proposal_variants_are_all_rejected() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let target = Address::generate(&env);
    let other = Address::generate(&env);
    advance_to(&env, 1_000);

    let mut zero_delay = valid_proposal(&target);
    zero_delay.delay_secs = 0;
    zero_delay.executable_at = zero_delay.proposed_at;
    let mut bad_timing = valid_proposal(&target);
    bad_timing.executable_at = u64::MAX;
    let mut wrong_target = valid_proposal(&target);
    wrong_target.target = other;

    for proposal in [zero_delay, bad_timing, wrong_target] {
        env.as_contract(&cid, || {
            env.storage()
                .instance()
                .set(&DataKey::TimelockProposal(target.clone()), &proposal);
        });
        assert_eq!(
            client.try_execute_timelock(&admin, &target),
            Err(Ok(TimeLockError::StateInconsistency))
        );
        assert_eq!(
            client.try_cancel_timelock(&admin, &target),
            Err(Ok(TimeLockError::StateInconsistency))
        );
    }
}

#[test]
fn proposing_over_a_corrupt_proposal_fails_with_state_inconsistency() {
    let env = Env::default();
    let (client, admin, cid) = setup(&env);
    let target = Address::generate(&env);
    let mut corrupt = valid_proposal(&target);
    corrupt.executable_at = corrupt.proposed_at; // inconsistent with delay

    env.as_contract(&cid, || {
        env.storage()
            .instance()
            .set(&DataKey::TimelockProposal(target.clone()), &corrupt);
    });

    assert_eq!(
        client.try_propose_timelock(&admin, &target, &10),
        Err(Ok(TimeLockError::StateInconsistency))
    );
}
