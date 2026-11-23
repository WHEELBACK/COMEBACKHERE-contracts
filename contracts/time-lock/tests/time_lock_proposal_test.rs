use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};
use time_lock::{TimeLockContract, TimeLockContractClient, TimeLockError, TimelockStatus};

fn setup() -> (Env, Address, Address, TimeLockContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let target = Address::generate(&env);
    let id = env.register_contract(None, TimeLockContract);
    let client = TimeLockContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, target, client)
}

#[test]
fn initialize_twice_fails_already_initialized() {
    let (_env, admin, _target, client) = setup();
    let err = client.try_initialize(&admin).unwrap_err().unwrap();
    assert_eq!(err, TimeLockError::AlreadyInitialized);
}

#[test]
fn execute_without_proposal_fails_not_found() {
    let (_env, admin, target, client) = setup();
    let err = client
        .try_execute_timelock(&admin, &target)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TimeLockError::TimelockNotFound);
    assert!(client.get_timelock(&target).is_none());
    assert!(!client.has_active_timelock(&target));
}

#[test]
fn zero_delay_rejected() {
    let (_env, admin, target, client) = setup();
    let err = client
        .try_propose_timelock(&admin, &target, &0)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TimeLockError::ZeroDelay);
    assert!(client.get_timelock(&target).is_none());
}

#[test]
fn non_admin_propose_fails_unauthorized() {
    let (env, _admin, target, client) = setup();
    let rando = Address::generate(&env);
    let err = client
        .try_propose_timelock(&rando, &target, &10)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TimeLockError::Unauthorized);
}

#[test]
fn propose_and_execute_after_delay() {
    let (env, admin, target, client) = setup();
    client.propose_timelock(&admin, &target, &10);
    let proposal = client.get_timelock(&target).unwrap();
    assert_eq!(proposal.target, target);
    assert_eq!(proposal.delay_secs, 10);
    assert_eq!(proposal.status, TimelockStatus::Active);
    assert!(client.has_active_timelock(&target));
    // Too early: the delay has not elapsed yet.
    let err = client
        .try_execute_timelock(&admin, &target)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TimeLockError::TooEarly);
    // Advance the ledger past the executable timestamp and execute.
    env.ledger().with_mut(|l| l.timestamp += 10);
    assert!(client.try_execute_timelock(&admin, &target).is_ok());
    let executed = client.get_timelock(&target).unwrap();
    assert_eq!(executed.status, TimelockStatus::Executed);
    // Executing again on a final state is rejected.
    let err = client
        .try_execute_timelock(&admin, &target)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TimeLockError::TimelockInactive);
}

#[test]
fn propose_on_active_target_fails() {
    let (_env, admin, target, client) = setup();
    client.propose_timelock(&admin, &target, &10);
    let err = client
        .try_propose_timelock(&admin, &target, &10)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TimeLockError::TimelockActive);
}

#[test]
fn cancel_blocks_execution_and_allows_repromote_after_finalization() {
    let (env, admin, target, client) = setup();
    client.propose_timelock(&admin, &target, &10);
    client.cancel_timelock(&admin, &target);
    assert_eq!(
        client.get_timelock(&target).unwrap().status,
        TimelockStatus::Cancelled
    );
    // Executing a cancelled timelock is rejected.
    let err = client
        .try_execute_timelock(&admin, &target)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TimeLockError::TimelockInactive);
    // Cancelling again is rejected too.
    let err = client
        .try_cancel_timelock(&admin, &target)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TimeLockError::TimelockInactive);
    // But proposing again after finalization is allowed.
    client.propose_timelock(&admin, &target, &10);
    assert_eq!(
        client.get_timelock(&target).unwrap().status,
        TimelockStatus::Active
    );
}
