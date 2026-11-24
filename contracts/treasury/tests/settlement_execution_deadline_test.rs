/// Tests for issue #590: execution deadline on settlements.
///
/// Verifies that:
/// - A settlement with no deadline (0) executes normally regardless of time.
/// - A settlement with a future deadline executes successfully before the deadline.
/// - A settlement is rejected with `ExecutionDeadlineExceeded` when executed after
///   the deadline, even if approvals are complete.
/// - Execution at exactly the deadline timestamp is allowed.
/// - A deadline of 0 is never treated as "expired".
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Ledger as _},
    Address, Env,
};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

// --- Minimal token stub for executing settlements ---

#[derive(Clone)]
#[contract]
pub struct TestToken;

#[contractimpl]
impl TestToken {
    pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {}
    pub fn balance(_env: Env, _id: Address) -> i128 {
        i128::MAX
    }
}

// --- Helper setup ---

fn setup(env: &Env) -> (TreasuryContractClient<'_>, Address, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let merchant = Address::generate(env);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &treasury_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    let token_id = env.register_contract(None, TestToken);
    (client, admin, merchant, token_id)
}

// --- Tests ---

/// A settlement proposed with deadline = 0 (no deadline) executes without error
/// even after significant ledger time has passed.
#[test]
fn execute_settlement_succeeds_with_no_deadline() {
    let env = Env::default();
    let (client, admin, merchant, token_id) = setup(&env);

    // Propose with no deadline (0).
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);

    // Advance time well past any plausible deadline.
    env.ledger().with_mut(|l| l.timestamp = 999_999_999);

    // Should still execute successfully because deadline == 0 means "no deadline".
    client.execute_settlement(&admin, &sid, &token_id);
}

/// A settlement with a future deadline executes successfully when called before
/// the deadline.
#[test]
fn execute_settlement_succeeds_before_deadline() {
    let env = Env::default();
    let (client, admin, merchant, token_id) = setup(&env);

    // Current timestamp is 0; set deadline 1 hour in the future.
    let deadline: u64 = 3_600;
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &deadline);

    // Execute at t=1_800, which is before the deadline.
    env.ledger().with_mut(|l| l.timestamp = 1_800);

    client.execute_settlement(&admin, &sid, &token_id);
}

/// `execute_settlement` returns `ExecutionDeadlineExceeded` when called after
/// the deadline, even though approvals are complete.
#[test]
fn execute_settlement_fails_after_deadline() {
    let env = Env::default();
    let (client, admin, merchant, token_id) = setup(&env);

    // Deadline is t=1_000.
    let deadline: u64 = 1_000;
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &deadline);

    // Execute at t=1_001, one second past the deadline.
    env.ledger().with_mut(|l| l.timestamp = 1_001);

    let result = client.try_execute_settlement(&admin, &sid, &token_id);
    assert_eq!(result, Err(Ok(TreasuryError::ExecutionDeadlineExceeded)));
}

/// Execution at exactly the deadline timestamp is permitted (boundary is inclusive).
#[test]
fn execute_settlement_succeeds_at_exact_deadline() {
    let env = Env::default();
    let (client, admin, merchant, token_id) = setup(&env);

    let deadline: u64 = 500;
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &deadline);

    // Execute at exactly t=500 — should succeed.
    env.ledger().with_mut(|l| l.timestamp = 500);

    client.execute_settlement(&admin, &sid, &token_id);
}

/// One second after the deadline must be rejected.
#[test]
fn execute_settlement_fails_one_second_after_deadline() {
    let env = Env::default();
    let (client, admin, merchant, token_id) = setup(&env);

    let deadline: u64 = 500;
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &deadline);

    env.ledger().with_mut(|l| l.timestamp = 501);

    let result = client.try_execute_settlement(&admin, &sid, &token_id);
    assert_eq!(result, Err(Ok(TreasuryError::ExecutionDeadlineExceeded)));
}

/// Verify `get_settlement` exposes the stored `execution_deadline` value.
#[test]
fn propose_settlement_stores_execution_deadline() {
    let env = Env::default();
    let (client, admin, merchant, _token_id) = setup(&env);

    let deadline: u64 = 7_200;
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &deadline);

    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.execution_deadline, deadline);
}

/// A settlement with deadline = 0 has `execution_deadline == 0` in storage.
#[test]
fn propose_settlement_with_no_deadline_stores_zero() {
    let env = Env::default();
    let (client, admin, merchant, _token_id) = setup(&env);

    let sid = client.propose_settlement(&admin, &merchant, &5_000_000, &0_u64);

    let settlement = client.get_settlement(&sid);
    assert_eq!(settlement.execution_deadline, 0);
}
