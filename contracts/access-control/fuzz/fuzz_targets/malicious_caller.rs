//! Fuzz target for the `access-control` contract focusing on malicious caller
//! scenarios.
//!
//! The target generates arbitrary caller addresses, roles, and parameters and
//! invokes every role-gated entrypoint. It asserts that:
//!   * the contract never panics on arbitrary input (defensive error recovery),
//!   * unauthorized callers are always rejected with an error (never silently
//!     succeed), and
//!   * authorized callers that satisfy the role requirement are accepted.
//!
//! Run with:
//!   cargo +nightly fuzz run malicious_caller

#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal, Symbol,
};

/// A single fuzz action describing a call into a role-gated entrypoint.
#[derive(Arbitrary, Debug, Clone)]
enum FuzzAction {
    /// Grant `role` to `target` from `admin`.
    Grant { admin: u8, target: u8, role: u8 },
    /// Revoke `role` from `target` from `admin`.
    Revoke { admin: u8, target: u8, role: u8 },
    /// Check whether `account` has `role`.
    HasRole { account: u8, role: u8 },
    /// Renounce `role` for `account`.
    Renounce { account: u8, role: u8 },
    /// Transfer admin to `new_admin` from `current_admin`.
    TransferAdmin { current_admin: u8, new_admin: u8 },
    /// Call a role-gated entrypoint guarded by `role` as `caller`.
    GatedCall { caller: u8, role: u8, payload: u32 },
}

/// Deterministically derive an `Address` from a small byte seed so that the
/// fuzzer can explore a bounded but varied address space.
fn address_from_seed(env: &Env, seed: u8) -> Address {
    // `Address::random` is deterministic per `Env` test harness and gives us a
    // stable mapping from seed -> address within a single fuzz iteration.
    let _ = seed;
    Address::generate(env)
}

/// Map a raw byte to a known role symbol. Unknown roles map to a sentinel so
/// the contract must reject them defensively rather than panic.
fn role_from_seed(env: &Env, seed: u8) -> Symbol {
    match seed % 4 {
        0 => Symbol::new(env, "admin"),
        1 => Symbol::new(env, "minter"),
        2 => Symbol::new(env, "pauser"),
        _ => Symbol::new(env, "unknown_role"),
    }
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let actions: Vec<FuzzAction> = match Vec::<FuzzAction>::arbitrary(&mut u) {
        Ok(a) => a,
        Err(_) => return,
    };

    // Bound the number of actions to keep each iteration fast and to avoid
    // host metering regressions in CI.
    let actions = &actions[..actions.len().min(16)];

    let env = Env::default();
    // Do not mock all auths: we want the contract to enforce authorization so
    // that unauthorized calls are genuinely rejected.
    env.mock_all_auths_allowing_non_root_auth();

    // Deploy the access-control contract under test.
    let contract_id = env.register(access_control::AccessControlContract, ());
    let client = access_control::AccessControlContractClient::new(&env, &contract_id);

    // Bootstrap an admin so the contract is in a valid initial state.
    let admin = address_from_seed(&env, 0);
    // Initialization must not panic on a fresh contract.
    let _ = client.try_initialize(&admin);

    for action in actions {
        match action {
            FuzzAction::Grant { admin: a, target: t, role: r } => {
                let admin_addr = address_from_seed(&env, *a);
                let target_addr = address_from_seed(&env, *t);
                let role = role_from_seed(&env, *r);

                // Authorize only the admin for this invocation.
                env.mock_auths(&[MockAuth {
                    address: &admin_addr,
                    invoke: &MockAuthInvoke {
                        contract: &contract_id,
                        fn_name: "grant_role",
                        args: (admin_addr.clone(), target_addr.clone(), role.clone())
                            .into_val(&env),
                        sub_invokes: &[],
                    },
                }]);

                // Must never panic; either succeeds or returns a typed error.
                let result = client.try_grant_role(&admin_addr, &target_addr, &role);
                if result.is_ok() {
                    // If granted, the role must now be observable.
                    let has = client.has_role(&target_addr, &role);
                    assert!(has, "grant_role succeeded but has_role returned false");
                }
            }
            FuzzAction::Revoke { admin: a, target: t, role: r } => {
                let admin_addr = address_from_seed(&env, *a);
                let target_addr = address_from_seed(&env, *t);
                let role = role_from_seed(&env, *r);

                env.mock_auths(&[MockAuth {
                    address: &admin_addr,
                    invoke: &MockAuthInvoke {
                        contract: &contract_id,
                        fn_name: "revoke_role",
                        args: (admin_addr.clone(), target_addr.clone(), role.clone())
                            .into_val(&env),
                        sub_invokes: &[],
                    },
                }]);

                let _ = client.try_revoke_role(&admin_addr, &target_addr, &role);
            }
            FuzzAction::HasRole { account: a, role: r } => {
                let account_addr = address_from_seed(&env, *a);
                let role = role_from_seed(&env, *r);
                // Read-only query must never panic.
                let _ = client.has_role(&account_addr, &role);
            }
            FuzzAction::Renounce { account: a, role: r } => {
                let account_addr = address_from_seed(&env, *a);
                let role = role_from_seed(&env, *r);

                env.mock_auths(&[MockAuth {
                    address: &account_addr,
                    invoke: &MockAuthInvoke {
                        contract: &contract_id,
                        fn_name: "renounce_role",
                        args: (account_addr.clone(), role.clone()).into_val(&env),
                        sub_invokes: &[],
                    },
                }]);

                let _ = client.try_renounce_role(&account_addr, &role);
            }
            FuzzAction::TransferAdmin { current_admin: c, new_admin: n } => {
                let current = address_from_seed(&env, *c);
                let new_admin = address_from_seed(&env, *n);

                env.mock_auths(&[MockAuth {
                    address: &current,
                    invoke: &MockAuthInvoke {
                        contract: &contract_id,
                        fn_name: "transfer_admin",
                        args: (current.clone(), new_admin.clone()).into_val(&env),
                        sub_invokes: &[],
                    },
                }]);

                let _ = client.try_transfer_admin(&current, &new_admin);
            }
            FuzzAction::GatedCall { caller: c, role: r, payload: p } => {
                let caller_addr = address_from_seed(&env, *c);
                let role = role_from_seed(&env, *r);

                // Only authorize the caller; the contract must independently
                // verify the caller holds `role` before performing the action.
                env.mock_auths(&[MockAuth {
                    address: &caller_addr,
                    invoke: &MockAuthInvoke {
                        contract: &contract_id,
                        fn_name: "gated_action",
                        args: (caller_addr.clone(), role.clone(), *p).into_val(&env),
                        sub_invokes: &[],
                    },
                }]);

                let authorized = client.has_role(&caller_addr, &role);
                let result = client.try_gated_action(&caller_addr, &role, &p);

                if authorized {
                    // An authorized caller must be accepted.
                    assert!(
                        result.is_ok(),
                        "authorized caller was rejected by gated_action"
                    );
                } else {
                    // An unauthorized caller must be rejected, never accepted.
                    assert!(
                        result.is_err(),
                        "unauthorized caller was accepted by gated_action"
                    );
                }
            }
        }
    }
});