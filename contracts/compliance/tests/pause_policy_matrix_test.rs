//! Single source of truth for the compliance contract's pause-blocking policy.
//!
//! `lib.rs`'s doc comments state, entrypoint by entrypoint, whether that
//! entrypoint is gated behind `require_not_paused`. That policy is scattered
//! across many individual doc comments, so nothing previously verified the
//! *whole picture* is internally consistent — a future change to a single
//! `require_not_paused` call site could silently contradict what the doc
//! comments still claim, and no single test would catch it.
//!
//! This file is that single test: one table, one row per admin-mutating
//! entrypoint, each asserted against the pause-gating behavior documented on
//! it in `lib.rs`. If an entrypoint's gating changes without this table (and
//! the doc comment it mirrors) being updated to match, this test fails.

use compliance::{ComplianceContract, ComplianceContractClient, ContractError};
use soroban_sdk::{testutils::Address as _, Address, Bytes, Env};

struct Ctx {
    env: Env,
    admin: Address,
    client: ComplianceContractClient<'static>,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    Ctx { env, admin, client }
}

/// Documented (per `lib.rs`) as permitted while paused — the emergency
/// remediation path, plus admin/role-management entrypoints that are
/// orthogonal to the allow/block mutations pause is meant to stop.
#[test]
fn entrypoints_documented_as_permitted_while_paused_are_not_blocked() {
    let Ctx { env, admin, client } = setup();
    let addr = Address::generate(&env);
    client.pause(&admin);

    // block_address: "permitted while paused (emergency policy)".
    client.block_address(&admin, &addr, &None);
    assert!(client.is_blocked(&addr));

    // block_address_until: "Permitted while paused (emergency policy)".
    let addr2 = Address::generate(&env);
    client.block_address_until(&admin, &addr2, &(env.ledger().timestamp() + 1000), &None);
    assert!(client.is_blocked(&addr2));

    // bulk_block_addresses: same emergency policy as block_address, batched.
    let addr3 = Address::generate(&env);
    let mut batch = soroban_sdk::Vec::new(&env);
    batch.push_back(addr3.clone());
    client.bulk_block_addresses(&admin, &batch);
    assert!(client.is_blocked(&addr3));

    // clear_address: "Permitted even while paused (emergency policy)".
    client.clear_address(&admin, &addr);
    assert!(!client.is_blocked(&addr));

    // Admin/role management: not gated by pause per their doc comments.
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    client.set_operator(&new_admin, &Address::generate(&env));

    let swept = client.sweep_expired(&new_admin);
    assert_eq!(swept, 0);
}

/// Documented as gated behind `require_not_paused` — every allow-side mutation
/// that grants or extends access. All of these must return
/// `ContractError::ContractPaused` while paused.
#[test]
fn entrypoints_documented_as_pause_gated_are_blocked() {
    let Ctx { env, admin, client } = setup();
    let addr = Address::generate(&env);
    client.pause(&admin);

    assert_eq!(
        client.try_allow_address(&admin, &addr),
        Err(Ok(ContractError::ContractPaused))
    );
    assert_eq!(
        client.try_allow_address_with_tier(&admin, &addr, &1),
        Err(Ok(ContractError::ContractPaused))
    );
    assert_eq!(
        client.try_allow_address_until(&admin, &addr, &(env.ledger().timestamp() + 1000)),
        Err(Ok(ContractError::ContractPaused))
    );
    let mut batch = soroban_sdk::Vec::new(&env);
    batch.push_back(addr.clone());
    assert_eq!(
        client.try_bulk_allow_addresses(&admin, &batch),
        Err(Ok(ContractError::ContractPaused))
    );
    assert_eq!(
        client.try_revoke_allow(&admin, &addr),
        Err(Ok(ContractError::ContractPaused))
    );

    // Confirm the pause is real and not a setup mistake: the same calls
    // succeed immediately after unpausing.
    client.unpause(&admin);
    client.allow_address(&admin, &addr);
    assert!(client.is_allowed(&addr));
}

/// `block_address`'s own reason-tracking sibling must honor the same
/// emergency-policy exemption even when combining both optional parameters
/// (`reason` and `unblock_at`).
#[test]
fn block_address_until_with_reason_bypasses_pause() {
    let Ctx { env, admin, client } = setup();
    let addr = Address::generate(&env);
    client.pause(&admin);

    let reason = Bytes::from_slice(&env, b"sanctions-match");
    client.block_address_until(
        &admin,
        &addr,
        &(env.ledger().timestamp() + 500),
        &Some(reason.clone()),
    );
    assert!(client.is_blocked(&addr));
    assert_eq!(client.get_block_reason(&addr), Some(reason));
}
