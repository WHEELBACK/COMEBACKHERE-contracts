//! Tests for the role-manager contract.
//!
//! Coverage targets:
//!
//! * **Issue #932** — arithmetic underflow in partial settlement quota
//!   computations is prevented by `checked_sub`; `ArithmeticUnderflow` and
//!   `QuotaExceeded` are returned instead of wrapping.
//!
//! * **Issue #934** — `has_role` and `get_role` are O(1) single-key reads
//!   (structural property; asserted via correct return values without full
//!   list iteration).
//!
//! * **Issue #935** — fuzzing/malicious-caller scenarios: unauthorized grant,
//!   unauthorized revoke, quota drain by wrong address, re-grant idempotency,
//!   cross-role boundary, role revoke-and-reassign cycle.

use role_manager::{Role, RoleManagerContract, RoleManagerContractClient, RoleManagerError};
use soroban_sdk::{testutils::Address as _, Address, Env};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn setup(env: &Env) -> (RoleManagerContractClient, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register(RoleManagerContract, ());
    let client = RoleManagerContractClient::new(env, &contract_id);
    client.initialize(&admin);
    (client, admin)
}

// ── Initialization ─────────────────────────────────────────────────────────────

#[test]
fn double_initialize_returns_error() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let result = client.try_initialize(&admin);
    assert_eq!(result, Err(Ok(RoleManagerError::AlreadyInitialized)));
}

// ── Grant role ─────────────────────────────────────────────────────────────────

#[test]
fn admin_can_grant_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    client.grant_role(&admin, &target, &Role::Proposer);

    assert!(client.has_role(&target));
    assert_eq!(client.get_role(&target), Some(Role::Proposer));
    assert_eq!(client.role_count(), 1);
}

#[test]
fn grant_role_idempotent_does_not_increase_count() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    client.grant_role(&admin, &target, &Role::Proposer);
    assert_eq!(client.role_count(), 1);

    // Re-granting the same role must not increment the counter.
    client.grant_role(&admin, &target, &Role::Proposer);
    assert_eq!(client.role_count(), 1);
}

#[test]
fn grant_role_can_upgrade_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    client.grant_role(&admin, &target, &Role::Proposer);
    assert_eq!(client.get_role(&target), Some(Role::Proposer));

    // Upgrading to Executor — count must not change.
    client.grant_role(&admin, &target, &Role::Executor);
    assert_eq!(client.get_role(&target), Some(Role::Executor));
    assert_eq!(client.role_count(), 1);
}

/// **Issue #935 — malicious caller** — a non-admin address must not be able to
/// grant a role.
#[test]
fn non_admin_cannot_grant_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let impostor = Address::generate(&env);
    let target = Address::generate(&env);

    // The impostor is not the stored admin, so `require_admin` returns Unauthorized.
    let result = client.try_grant_role(&impostor, &target, &Role::Proposer);
    assert_eq!(result, Err(Ok(RoleManagerError::Unauthorized)));

    // Target must not have acquired a role.
    assert!(!client.has_role(&target));

    // Confirm that the legitimate admin's grant still works afterwards.
    client.grant_role(&admin, &target, &Role::Executor);
    assert!(client.has_role(&target));
}

// ── Revoke role ────────────────────────────────────────────────────────────────

#[test]
fn admin_can_revoke_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    client.grant_role(&admin, &target, &Role::Executor);
    assert_eq!(client.role_count(), 1);

    client.revoke_role(&admin, &target);

    assert!(!client.has_role(&target));
    assert_eq!(client.get_role(&target), None);
    assert_eq!(client.role_count(), 0);
}

/// **Issue #935 — malicious caller** — a non-admin must not revoke another
/// address's role.
#[test]
fn non_admin_cannot_revoke_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);
    let impostor = Address::generate(&env);

    client.grant_role(&admin, &target, &Role::Proposer);

    let result = client.try_revoke_role(&impostor, &target);
    assert_eq!(result, Err(Ok(RoleManagerError::Unauthorized)));

    // Target must still have its role.
    assert!(client.has_role(&target));
}

#[test]
fn revoking_nonexistent_role_returns_address_has_no_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    let result = client.try_revoke_role(&admin, &target);
    assert_eq!(result, Err(Ok(RoleManagerError::AddressHasNoRole)));
}

/// **Issue #932 — underflow guard** — revoking from a count of 0 must NOT wrap.
/// This exercises the `checked_sub` path in `revoke_role`.
#[test]
fn revoking_twice_returns_address_has_no_role_not_underflow() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    client.grant_role(&admin, &target, &Role::Observer);
    client.revoke_role(&admin, &target);
    assert_eq!(client.role_count(), 0);

    // Second revoke must return AddressHasNoRole (not underflow).
    let result = client.try_revoke_role(&admin, &target);
    assert_eq!(result, Err(Ok(RoleManagerError::AddressHasNoRole)));
    // Count must still be 0, not u32::MAX.
    assert_eq!(client.role_count(), 0);
}

// ── Quota management (issue #932 underflow) ───────────────────────────────────

#[test]
fn set_and_consume_quota_happy_path() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let member = Address::generate(&env);

    client.grant_role(&admin, &member, &Role::Executor);
    client.set_quota(&admin, &member, &1_000_000);

    assert_eq!(client.get_quota(&member), 1_000_000);

    let remaining = client.consume_quota(&member, &400_000);
    assert_eq!(remaining, 600_000);
    assert_eq!(client.get_quota(&member), 600_000);
}

/// **Issue #932** — consuming more than the quota must return `QuotaExceeded`,
/// not silently wrap the remaining value to a large positive number.
#[test]
fn consume_quota_exceeding_balance_returns_quota_exceeded() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let member = Address::generate(&env);

    client.grant_role(&admin, &member, &Role::Executor);
    client.set_quota(&admin, &member, &100);

    let result = client.try_consume_quota(&member, &101);
    assert_eq!(result, Err(Ok(RoleManagerError::QuotaExceeded)));

    // Quota must remain untouched.
    assert_eq!(client.get_quota(&member), 100);
}

/// Consuming the entire quota in one call succeeds and leaves 0 remaining.
#[test]
fn consume_entire_quota_leaves_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let member = Address::generate(&env);

    client.grant_role(&admin, &member, &Role::Proposer);
    client.set_quota(&admin, &member, &5_000_000);

    let remaining = client.consume_quota(&member, &5_000_000);
    assert_eq!(remaining, 0);
}

/// After fully consuming the quota, any further consume must fail with
/// `QuotaExceeded` (not underflow).
#[test]
fn consume_after_quota_exhausted_returns_quota_exceeded() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let member = Address::generate(&env);

    client.grant_role(&admin, &member, &Role::Executor);
    client.set_quota(&admin, &member, &1);

    client.consume_quota(&member, &1);

    let result = client.try_consume_quota(&member, &1);
    assert_eq!(result, Err(Ok(RoleManagerError::QuotaExceeded)));
    assert_eq!(client.get_quota(&member), 0);
}

/// **Issue #932 — boundary** — consuming exactly `i128::MAX` from a quota of
/// `i128::MAX` must succeed without overflow.
#[test]
fn consume_i128_max_quota_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let member = Address::generate(&env);

    client.grant_role(&admin, &member, &Role::Executor);
    client.set_quota(&admin, &member, &i128::MAX);

    let remaining = client.consume_quota(&member, &i128::MAX);
    assert_eq!(remaining, 0);
}

/// **Issue #935 — malicious caller** — an address with no role must not consume
/// any quota.
#[test]
fn address_without_role_cannot_consume_quota() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let member = Address::generate(&env);
    let stranger = Address::generate(&env);

    client.grant_role(&admin, &member, &Role::Executor);
    client.set_quota(&admin, &member, &1_000_000);

    // Stranger has no role, so consume_quota on its own behalf must fail.
    let result = client.try_consume_quota(&stranger, &1);
    assert_eq!(result, Err(Ok(RoleManagerError::AddressHasNoRole)));

    // Member's quota is untouched.
    assert_eq!(client.get_quota(&member), 1_000_000);
}

/// **Issue #935 — malicious caller** — a zero-amount consume must be rejected.
#[test]
fn consume_zero_amount_returns_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let member = Address::generate(&env);

    client.grant_role(&admin, &member, &Role::Proposer);
    client.set_quota(&admin, &member, &100);

    let result = client.try_consume_quota(&member, &0);
    assert_eq!(result, Err(Ok(RoleManagerError::InvalidAmount)));
}

/// Setting quota on an address without a role must fail.
#[test]
fn set_quota_for_unregistered_address_returns_address_has_no_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let stranger = Address::generate(&env);

    let result = client.try_set_quota(&admin, &stranger, &1_000);
    assert_eq!(result, Err(Ok(RoleManagerError::AddressHasNoRole)));
}

// ── Revoke-and-reassign cycle ─────────────────────────────────────────────────

/// Revoking a role clears the quota; re-granting and re-setting quota starts fresh.
#[test]
fn revoke_clears_quota_and_reassign_starts_fresh() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let member = Address::generate(&env);

    client.grant_role(&admin, &member, &Role::Executor);
    client.set_quota(&admin, &member, &5_000);
    client.revoke_role(&admin, &member);

    // After revocation quota should read 0.
    assert_eq!(client.get_quota(&member), 0);

    // Re-grant and set a fresh quota.
    client.grant_role(&admin, &member, &Role::Proposer);
    client.set_quota(&admin, &member, &2_000);
    assert_eq!(client.get_quota(&member), 2_000);
}

// ── Pause / unpause ───────────────────────────────────────────────────────────

#[test]
fn paused_contract_rejects_grant_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    client.pause(&admin);

    let result = client.try_grant_role(&admin, &target, &Role::Observer);
    assert_eq!(result, Err(Ok(RoleManagerError::ContractPaused)));
}

#[test]
fn paused_contract_rejects_revoke_role() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    client.grant_role(&admin, &target, &Role::Proposer);
    client.pause(&admin);

    let result = client.try_revoke_role(&admin, &target);
    assert_eq!(result, Err(Ok(RoleManagerError::ContractPaused)));
}

#[test]
fn paused_contract_rejects_consume_quota() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let member = Address::generate(&env);

    client.grant_role(&admin, &member, &Role::Executor);
    client.set_quota(&admin, &member, &100);
    client.pause(&admin);

    let result = client.try_consume_quota(&member, &10);
    assert_eq!(result, Err(Ok(RoleManagerError::ContractPaused)));
}

#[test]
fn unpausing_allows_operations_to_resume() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    client.pause(&admin);
    client.unpause(&admin);

    client.grant_role(&admin, &target, &Role::RoleAdmin);
    assert!(client.has_role(&target));
}

// ── has_specific_role (O(1) check, issue #934) ────────────────────────────────

#[test]
fn has_specific_role_returns_correct_boolean() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    client.grant_role(&admin, &target, &Role::RoleAdmin);

    assert!(client.has_specific_role(&target, &Role::RoleAdmin));
    assert!(!client.has_specific_role(&target, &Role::Proposer));
    assert!(!client.has_specific_role(&target, &Role::Executor));
    assert!(!client.has_specific_role(&target, &Role::Observer));
}

// ── list_role_members (bounded list, issue #934) ──────────────────────────────

#[test]
fn list_role_members_reflects_grants_and_revocations() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    client.grant_role(&admin, &a, &Role::Proposer);
    client.grant_role(&admin, &b, &Role::Executor);
    client.grant_role(&admin, &c, &Role::Observer);

    let members = client.list_role_members();
    assert_eq!(members.len(), 3);

    client.revoke_role(&admin, &b);
    let members_after = client.list_role_members();
    assert_eq!(members_after.len(), 2);
    assert!(!members_after.contains(&b));
}

// ── role_granted_at ───────────────────────────────────────────────────────────

#[test]
fn role_granted_at_records_timestamp() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    let ts_before = env.ledger().timestamp();
    client.grant_role(&admin, &target, &Role::Proposer);

    let ts = client.role_granted_at(&target);
    assert!(ts.is_some());
    assert!(ts.unwrap() >= ts_before);
}

#[test]
fn role_granted_at_cleared_on_revoke() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let target = Address::generate(&env);

    client.grant_role(&admin, &target, &Role::Proposer);
    client.revoke_role(&admin, &target);

    assert_eq!(client.role_granted_at(&target), None);
}

// ── Fuzz-style: many addresses ────────────────────────────────────────────────

/// Grant 20 distinct addresses different roles and verify correct lookup for all.
#[test]
fn grant_many_addresses_all_query_correctly() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let roles = [
        Role::Proposer,
        Role::Executor,
        Role::Observer,
        Role::RoleAdmin,
    ];
    let mut addresses = soroban_sdk::Vec::new(&env);

    for i in 0..20usize {
        let addr = Address::generate(&env);
        let role = roles[i % roles.len()].clone();
        client.grant_role(&admin, &addr, &role);
        addresses.push_back(addr);
    }

    assert_eq!(client.role_count(), 20);

    for addr in addresses.iter() {
        assert!(client.has_role(&addr));
        assert!(client.get_role(&addr).is_some());
    }
}

/// Grant and immediately revoke 10 addresses; counter must return to 0.
#[test]
fn grant_and_revoke_many_counter_returns_to_zero() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let mut addrs: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
    for _ in 0..10 {
        let addr = Address::generate(&env);
        client.grant_role(&admin, &addr, &Role::Proposer);
        addrs.push_back(addr);
    }
    assert_eq!(client.role_count(), 10);

    for addr in addrs.iter() {
        client.revoke_role(&admin, &addr);
    }
    assert_eq!(client.role_count(), 0);
}

/// Sequential partial quota consumption that totals to exactly the quota must
/// leave 0 and no underflow.
#[test]
fn sequential_quota_consumption_sums_exactly_to_quota() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);
    let member = Address::generate(&env);

    let total_quota = 1_000i128;
    client.grant_role(&admin, &member, &Role::Executor);
    client.set_quota(&admin, &member, &total_quota);

    // Consume 10 at a time, 100 iterations.
    for i in 0..100 {
        let remaining = client.consume_quota(&member, &10);
        assert_eq!(remaining, total_quota - 10 * (i as i128 + 1));
    }

    assert_eq!(client.get_quota(&member), 0);

    // One more consume must fail with QuotaExceeded, not underflow.
    let result = client.try_consume_quota(&member, &1);
    assert_eq!(result, Err(Ok(RoleManagerError::QuotaExceeded)));
}
