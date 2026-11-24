//! Tests for `force_cancel_settlement` authorization (#584).
//!
//! `force_cancel_settlement` bypasses the normal signer flow and is restricted
//! to the admin only. Because it is powerful, its access control is tested
//! explicitly here:
//!
//! * Non-admin addresses are rejected.
//! * Registered signers (non-admin) are rejected.
//! * The settlement proposer (if not admin) is rejected.
//! * A successful admin call changes the settlement status to `Cancelled`.
//! * A successful admin call emits the `settlement_force_cancelled` event.
//! * Attempting to force-cancel an already-terminal settlement
//!   (Cancelled / OnHold-then-force-cancelled again) fails with `ForceCancelNotAllowed`.

use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, Symbol, TryFromVal, Vec,
};
use treasury::{SettlementHoldReason, SettlementStatus, TreasuryContract, TreasuryContractClient};

/// Role identifiers mirroring the contract's role-based access control.
/// `Admin` is the only role permitted to force-cancel settlements; `Operator`
/// and unregistered callers must be rejected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    Admin,
    Operator,
    Unauthorized,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup_treasury(env: &Env, threshold: u32) -> (TreasuryContractClient<'_>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &threshold, &Vec::new(env));
    (client, admin)
}

/// Proposes a settlement as `admin` and returns its id.
fn propose(client: &TreasuryContractClient<'_>, env: &Env, admin: &Address) -> u64 {
    let merchant = Address::generate(env);
    client.propose_settlement(admin, &merchant, &10_000_000, &0_u64)
}

// ---------------------------------------------------------------------------
// Rejection tests
// ---------------------------------------------------------------------------

/// A random address that was never registered as admin or signer must be
/// rejected. `force_cancel_settlement` panics via `require_admin` →
/// `panic_with_error!(Unauthorized)` — error code 9.
#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn non_admin_cannot_force_cancel_settlement() {
    let env = Env::default();
    let (client, admin) = setup_treasury(&env, 1);
    let settlement_id = propose(&client, &env, &admin);

    let non_admin = Address::generate(&env);
    client.force_cancel_settlement(&non_admin, &settlement_id);
}

/// A registered signer with weight > 0 that is not the admin must be rejected.
/// Signers have no special privilege for force-cancel.
#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn registered_signer_cannot_force_cancel_settlement() {
    let env = Env::default();
    let (client, admin) = setup_treasury(&env, 2);

    let signer = Address::generate(&env);
    client.set_signer(&admin, &signer, &1);

    let settlement_id = propose(&client, &env, &admin);

    // The signer can approve settlements but must not force-cancel.
    client.force_cancel_settlement(&signer, &settlement_id);
}

/// The address that proposed the settlement (if not admin) must also be rejected.
/// The proposer has no special force-cancel rights beyond a normal signer.
#[test]
#[should_panic(expected = "Error(Contract, #9)")]
fn proposer_cannot_force_cancel_settlement() {
    let env = Env::default();
    let (client, admin) = setup_treasury(&env, 2);

    // Register a second signer who will be the proposer.
    let proposer = Address::generate(&env);
    client.set_signer(&admin, &proposer, &1);

    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&proposer, &merchant, &5_000_000, &0_u64);

    // The proposer must not be able to force-cancel their own settlement.
    client.force_cancel_settlement(&proposer, &settlement_id);
}

// ---------------------------------------------------------------------------
// Success tests
// ---------------------------------------------------------------------------

/// Admin can force-cancel a `Pending` settlement; the status must change to
/// `Cancelled` and the `settlement_force_cancelled` event must be emitted.
#[test]
fn admin_can_force_cancel_pending_settlement_and_event_is_emitted() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_treasury(&env, 2);
    let settlement_id = propose(&client, &env, &admin);

    // Confirm starting state.
    let before = client.get_settlement(&settlement_id);
    assert_eq!(before.status, SettlementStatus::Pending);

    client.force_cancel_settlement(&admin, &settlement_id);
    // Read events now: `all()` only covers the most recent invocation, so the
    // `get_settlement` call below would otherwise replace them.
    let events = env.events().all();

    // Status must now be Cancelled.
    let after = client.get_settlement(&settlement_id);
    assert_eq!(after.status, SettlementStatus::Cancelled);

    // The `settlement_force_cancelled` event must have been emitted.
    let target = Symbol::new(&env, "settlement_force_cancelled");
    let emitted = events.iter().any(|(_, topics, _)| {
        topics
            .get(0)
            .and_then(|v| Symbol::try_from_val(&env, &v).ok())
            .map(|s| s == target)
            .unwrap_or(false)
    });
    assert!(emitted, "settlement_force_cancelled event was not emitted");
}

/// Admin can force-cancel an `OnHold` settlement (e.g. one stuck because the
/// dispute or hold cannot be resolved through normal means).
#[test]
fn admin_can_force_cancel_onhold_settlement_and_event_is_emitted() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_treasury(&env, 2);
    let settlement_id = propose(&client, &env, &admin);

    // Place the settlement on hold.
    client.hold_settlement(
        &admin,
        &settlement_id,
        &SettlementHoldReason::AdminHold,
        &None,
    );

    let held = client.get_settlement(&settlement_id);
    assert_eq!(held.status, SettlementStatus::OnHold);

    client.force_cancel_settlement(&admin, &settlement_id);
    // Read events now: `all()` only covers the most recent invocation, so the
    // `get_settlement` call below would otherwise replace them.
    let events = env.events().all();

    let after = client.get_settlement(&settlement_id);
    assert_eq!(after.status, SettlementStatus::Cancelled);

    // Verify the event was emitted.
    let target = Symbol::new(&env, "settlement_force_cancelled");
    let emitted = events.iter().any(|(_, topics, _)| {
        topics
            .get(0)
            .and_then(|v| Symbol::try_from_val(&env, &v).ok())
            .map(|s| s == target)
            .unwrap_or(false)
    });
    assert!(
        emitted,
        "settlement_force_cancelled event was not emitted for OnHold settlement"
    );
}

// ---------------------------------------------------------------------------
// Terminal-state rejection tests
// ---------------------------------------------------------------------------

/// Attempting to force-cancel an already `Cancelled` settlement must fail with
/// `ForceCancelNotAllowed` (error code 37).
#[test]
#[should_panic(expected = "Error(Contract, #37)")]
fn cannot_force_cancel_already_cancelled_settlement() {
    let env = Env::default();
    let (client, admin) = setup_treasury(&env, 1);
    let settlement_id = propose(&client, &env, &admin);

    // Cancel normally via admin batch_cancel_settlements.
    let mut ids = Vec::new(&env);
    ids.push_back(settlement_id);
    client.batch_cancel_settlements(&admin, &ids);

    // A second force-cancel on a Cancelled settlement must fail.
    client.force_cancel_settlement(&admin, &settlement_id);
}

/// Attempting to force-cancel a non-existent settlement must panic with
/// `SettlementNotFound` (error code 3).
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn cannot_force_cancel_nonexistent_settlement() {
    let env = Env::default();
    let (client, admin) = setup_treasury(&env, 1);

    client.force_cancel_settlement(&admin, &9999);
}

// ---------------------------------------------------------------------------
// Granular role-based authorization tests (#969)
// ---------------------------------------------------------------------------

/// Registers `caller` with the given role and returns its address.
/// `Operator` is modelled as a registered signer with weight > 0; `Admin`
/// returns the existing admin; `Unauthorized` returns a fresh unregistered
/// address.
fn caller_for_role(
    env: &Env,
    client: &TreasuryContractClient<'_>,
    admin: &Address,
    role: Role,
) -> Address {
    match role {
        Role::Admin => admin.clone(),
        Role::Operator => {
            let operator = Address::generate(env);
            client.set_signer(admin, &operator, &1);
            operator
        }
        Role::Unauthorized => Address::generate(env),
    }
}

/// Only the `Admin` role may force-cancel; `Operator` and unauthorized callers
/// must be rejected with `Unauthorized` (error code 9) and must not be able to
/// modify the settlement status.
#[test]
fn only_admin_role_can_force_cancel() {
    for role in [Role::Admin, Role::Operator, Role::Unauthorized] {
        let env = Env::default();
        env.mock_all_auths();
        let (client, admin) = setup_treasury(&env, 2);
        let settlement_id = propose(&client, &env, &admin);
        let caller = caller_for_role(&env, &client, &admin, role);

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.force_cancel_settlement(&caller, &settlement_id);
        }));

        match role {
            Role::Admin => {
                assert!(result.is_ok(), "admin force-cancel must succeed");
                let after = client.get_settlement(&settlement_id);
                assert_eq!(after.status, SettlementStatus::Cancelled);
            }
            Role::Operator | Role::Unauthorized => {
                assert!(
                    result.is_err(),
                    "{role:?} must not be able to force-cancel a settlement"
                );
                // The settlement must remain untouched by the rejected caller.
                let after = client.get_settlement(&settlement_id);
                assert_eq!(
                    after.status,
                    SettlementStatus::Pending,
                    "{role:?} must not modify settlement status"
                );
            }
        }
    }
}

/// An `Operator` (registered signer) cannot force-cancel an `OnHold`
/// settlement, and the hold state must be preserved after the rejected call.
#[test]
fn operator_cannot_force_cancel_onhold_settlement() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_treasury(&env, 2);
    let settlement_id = propose(&client, &env, &admin);

    client.hold_settlement(
        &admin,
        &settlement_id,
        &SettlementHoldReason::AdminHold,
        &None,
    );

    let operator = caller_for_role(&env, &client, &admin, Role::Operator);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.force_cancel_settlement(&operator, &settlement_id);
    }));
    assert!(result.is_err(), "operator must not force-cancel");

    let after = client.get_settlement(&settlement_id);
    assert_eq!(
        after.status,
        SettlementStatus::OnHold,
        "rejected operator must not modify settlement status"
    );
}

/// The admin path still succeeds after an operator attempt is rejected,
/// confirming authorization checks do not corrupt contract state.
#[test]
fn admin_can_force_cancel_after_operator_rejected() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup_treasury(&env, 2);
    let settlement_id = propose(&client, &env, &admin);

    let operator = caller_for_role(&env, &client, &admin, Role::Operator);
    let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        client.force_cancel_settlement(&operator, &settlement_id);
    }));
    assert!(rejected.is_err(), "operator must be rejected");

    // Admin path must still work.
    client.force_cancel_settlement(&admin, &settlement_id);
    let after = client.get_settlement(&settlement_id);
    assert_eq!(after.status, SettlementStatus::Cancelled);
}
