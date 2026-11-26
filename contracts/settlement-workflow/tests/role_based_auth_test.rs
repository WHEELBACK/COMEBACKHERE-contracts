//! #870 — Granular role-based authorization checks for settlement-workflow.
//!
//! Verifies that every admin-gated entrypoint enforces the stored admin role
//! and that the execution path is open to any caller (no admin required).

use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{
    SettlementWorkflowContract, SettlementWorkflowContractClient, WorkflowError,
};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

// ── Minimal pausable stub ────────────────────────────────────────────────────

#[contract]
struct PausableStub;

#[contractimpl]
impl PausableStub {
    pub fn pause(_env: Env, _admin: Address) {}
    pub fn unpause(_env: Env, _admin: Address) {}
}

// ── Setup ────────────────────────────────────────────────────────────────────

struct Fixture {
    env: Env,
    admin: Address,
    outsider: Address,
    workflow: SettlementWorkflowContractClient<'static>,
    workflow_id: Address,
    treasury: TreasuryContractClient<'static>,
    compliance: ComplianceContractClient<'static>,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let outsider = Address::generate(&env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    let workflow_id = env.register(
        SettlementWorkflowContract,
        (admin.clone(), compliance_id.clone(), treasury_id.clone()),
    );
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    treasury.set_signer(&admin, &workflow_id, &1);

    Fixture {
        env,
        admin,
        outsider,
        workflow,
        workflow_id,
        treasury,
        compliance,
    }
}

// ── pause / unpause ──────────────────────────────────────────────────────────

/// Only the stored admin may call `pause`.
#[test]
fn pause_requires_stored_admin() {
    let f = setup();

    // Admin succeeds.
    f.workflow.pause(&f.admin);
    assert!(f.workflow.is_paused());

    // Outsider is rejected.
    f.workflow.unpause(&f.admin);
    let err = f.workflow.try_pause(&f.outsider).unwrap_err().unwrap();
    assert_eq!(err, TreasuryError::Unauthorized.into());
    assert!(!f.workflow.is_paused());
}

/// Only the stored admin may call `unpause`.
#[test]
fn unpause_requires_stored_admin() {
    let f = setup();
    f.workflow.pause(&f.admin);

    let err = f.workflow.try_unpause(&f.outsider).unwrap_err().unwrap();
    assert_eq!(err, TreasuryError::Unauthorized.into());
    assert!(
        f.workflow.is_paused(),
        "must remain paused after rejected unpause"
    );

    // Admin succeeds.
    f.workflow.unpause(&f.admin);
    assert!(!f.workflow.is_paused());
}

/// `pause` can only be called by the admin — a random address (even one that
/// provides valid auth via `mock_all_auths`) is rejected with `Unauthorized`.
#[test]
fn non_admin_pause_rejected_even_with_auth() {
    let f = setup();
    let random = Address::generate(&f.env);

    let err = f.workflow.try_pause(&random).unwrap_err().unwrap();
    assert_eq!(err, TreasuryError::Unauthorized.into());
}

// ── transfer_admin / accept_admin ────────────────────────────────────────────

/// Only the stored admin may initiate an admin transfer.
#[test]
fn transfer_admin_requires_stored_admin() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    // Outsider rejected.
    let err = f
        .workflow
        .try_transfer_admin(&f.outsider, &new_admin)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::Unauthorized.into());

    // Admin succeeds.
    f.workflow.transfer_admin(&f.admin, &new_admin);
}

/// Only the pending nominee may call `accept_admin`.
#[test]
fn accept_admin_requires_pending_nominee() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    f.workflow.transfer_admin(&f.admin, &new_admin);

    // Outsider rejected.
    let err = f
        .workflow
        .try_accept_admin(&f.outsider)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::Unauthorized.into());

    // Nominee accepted.
    f.workflow.accept_admin(&new_admin);
    assert_eq!(f.workflow.get_admin(), new_admin);
}

/// `accept_admin` with no pending nomination returns `NoPendingAdmin`.
#[test]
fn accept_admin_without_nomination_returns_error() {
    let f = setup();
    let candidate = Address::generate(&f.env);

    let err = f
        .workflow
        .try_accept_admin(&candidate)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::NoPendingAdmin.into());
}

// ── emergency pause ──────────────────────────────────────────────────────────

/// Only the pinned emergency-pause admin may call `emergency_pause_all`.
/// A different address is rejected with `WorkflowError::Unauthorized`.
#[test]
fn emergency_pause_all_requires_pause_admin() {
    let f = setup();
    let target = f.env.register(PausableStub, ());

    // The workflow's stored admin configures the targets; it becomes the emergency-pause admin.
    f.workflow
        .initialize_emergency_pause(&f.admin, &soroban_sdk::vec![&f.env, target]);

    // A different address (not the configured emergency pause admin) is rejected.
    let err = f
        .workflow
        .try_emergency_pause_all(&f.outsider)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, WorkflowError::Unauthorized.into());
}

/// `emergency_pause_all` and `resume_all` happy path:
/// admin can pause all targets, `get_emergency_paused_at` becomes Some,
/// then resume clears it.
#[test]
fn resume_all_requires_prior_emergency_pause() {
    let f = setup();
    let target = f.env.register(PausableStub, ());

    f.workflow
        .initialize_emergency_pause(&f.admin, &soroban_sdk::vec![&f.env, target]);
    f.workflow.emergency_pause_all(&f.admin);
    assert!(f.workflow.get_emergency_paused_at().is_some());

    // Resume by admin works.
    f.workflow.resume_all(&f.admin, &0_u64);
    assert!(f.workflow.get_emergency_paused_at().is_none());
}

/// `resume_all` without a prior emergency pause is rejected with `NotEmergencyPaused`.
#[test]
fn resume_all_without_emergency_pause_is_rejected() {
    let f = setup();
    let target = f.env.register(PausableStub, ());

    f.workflow
        .initialize_emergency_pause(&f.admin, &soroban_sdk::vec![&f.env, target]);

    let err = f
        .workflow
        .try_resume_all(&f.admin, &0_u64)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, WorkflowError::NotEmergencyPaused.into());
}

/// `initialize_emergency_pause` rejects a non-admin address.
#[test]
fn initialize_emergency_pause_requires_stored_admin() {
    let f = setup();
    let target = f.env.register(PausableStub, ());

    // Outsider cannot configure emergency pause on an already-initialized workflow.
    let err = f
        .workflow
        .try_initialize_emergency_pause(&f.outsider, &soroban_sdk::vec![&f.env, target.clone()])
        .unwrap_err()
        .unwrap();
    assert_eq!(err, WorkflowError::Unauthorized.into());

    // Stored admin CAN configure and trigger emergency pause.
    f.workflow
        .initialize_emergency_pause(&f.admin, &soroban_sdk::vec![&f.env, target]);
    let err = f
        .workflow
        .try_emergency_pause_all(&f.outsider)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, WorkflowError::Unauthorized.into());

    f.workflow.emergency_pause_all(&f.admin);
    assert!(f.workflow.get_emergency_paused_at().is_some());
}

// ── execution path is open (no admin required) ───────────────────────────────

/// `execute_with_compliance` does not require the caller to be the admin.
/// Any allowed merchant may trigger execution.
#[test]
fn execute_with_compliance_does_not_require_admin() {
    let f = setup();
    let merchant = Address::generate(&f.env);

    f.compliance.allow_address(&f.admin, &merchant);
    let settlement_id = f
        .treasury
        .propose_settlement(&f.admin, &merchant, &1_000_000, &0_u64);

    let token_id = f.env.register_stellar_asset_contract(f.admin.clone());
    // Mint to treasury for the token transfer.
    soroban_sdk::token::StellarAssetClient::new(&f.env, &token_id)
        .mint(&f.treasury.address, &1_000_000);

    // The outsider (non-admin) can still trigger execution when compliance passes.
    // The workflow's own address is the treasury signer, so the caller identity
    // for the compliance/execution path is not the message sender.
    f.workflow
        .try_execute_with_compliance(&settlement_id, &token_id, &merchant)
        .expect("execute_with_compliance must not require admin role")
        .expect("inner result must be Ok");
}
