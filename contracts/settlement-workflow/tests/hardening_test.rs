//! Tests for invariant verification (#873), the modular error surface and its
//! client-facing messages (#874), and automatic TTL extension (#875).

use comebackhere_tests::fixtures::setup_with_workflow;
use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{
    errors, ttl, SettlementWorkflowContract, SettlementWorkflowContractClient, WorkflowError,
};
use soroban_sdk::{
    contract, contractimpl,
    testutils::{storage::Instance as _, Address as _, Ledger},
    vec, Address, Env, String,
};

#[contract]
struct InertTarget;

#[contractimpl]
impl InertTarget {
    pub fn pause(_env: Env, _admin: Address) {}
    pub fn unpause(_env: Env, _admin: Address) {}
}

fn instance_ttl(env: &Env, contract: &Address) -> u32 {
    env.as_contract(contract, || env.storage().instance().get_ttl())
}

// ── #873: invariants ────────────────────────────────────────────────────────

#[test]
fn a_correctly_deployed_workflow_satisfies_every_invariant() {
    let env = Env::default();
    let f = setup_with_workflow(&env, true);
    assert_eq!(f.workflow.try_verify_invariants(), Ok(Ok(())));
}

#[test]
fn pinning_one_contract_as_both_compliance_and_treasury_is_an_invariant_violation() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let compliance_id = env.register(ComplianceContract, ());
    ComplianceContractClient::new(&env, &compliance_id).initialize(&admin);

    let workflow_id = env.register(
        SettlementWorkflowContract,
        (admin.clone(), compliance_id.clone(), compliance_id.clone()),
    );
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);

    assert_eq!(
        workflow.try_verify_invariants(),
        Err(Ok(WorkflowError::InvariantViolation))
    );
    // The settlement path refuses to run against the misconfigured pair.
    let token = env.register_stellar_asset_contract(admin.clone());
    assert!(workflow
        .try_execute_with_compliance(&1, &token, &admin)
        .is_err());
    assert!(workflow
        .try_execute_with_compliance_batch(&vec![&env, 1u64], &token, &admin)
        .is_err());
}

#[test]
fn nominating_the_current_admin_is_rejected() {
    let env = Env::default();
    let f = setup_with_workflow(&env, true);
    assert!(f.workflow.try_transfer_admin(&f.admin, &f.admin).is_err());
    assert_eq!(f.workflow.try_verify_invariants(), Ok(Ok(())));
}

#[test]
fn a_second_emergency_sweep_cannot_reset_the_incident_timestamp() {
    let env = Env::default();
    let f = setup_with_workflow(&env, true);
    let target = env.register(InertTarget, ());
    f.workflow
        .initialize_emergency_pause(&f.admin, &vec![&env, target]);

    env.ledger().with_mut(|l| l.timestamp = 1_000);
    f.workflow.emergency_pause_all(&f.admin);
    env.ledger().with_mut(|l| l.timestamp = 2_000);
    assert_eq!(
        f.workflow.try_emergency_pause_all(&f.admin),
        Err(Ok(WorkflowError::EmergencyPauseActive))
    );
    assert_eq!(f.workflow.get_emergency_paused_at(), Some(1_000));
    assert_eq!(f.workflow.try_verify_invariants(), Ok(Ok(())));

    f.workflow.resume_all(&f.admin, &0);
    assert_eq!(f.workflow.try_verify_invariants(), Ok(Ok(())));
}

#[test]
fn only_the_workflow_admin_can_configure_the_emergency_pause_first() {
    let env = Env::default();
    let f = setup_with_workflow(&env, true);
    let outsider = Address::generate(&env);
    let target = env.register(InertTarget, ());
    assert_eq!(
        f.workflow
            .try_initialize_emergency_pause(&outsider, &vec![&env, target.clone()]),
        Err(Ok(WorkflowError::Unauthorized))
    );
    f.workflow
        .initialize_emergency_pause(&f.admin, &vec![&env, target.clone()]);
    assert_eq!(
        f.workflow
            .try_initialize_emergency_pause(&outsider, &vec![&env, target]),
        Err(Ok(WorkflowError::Unauthorized))
    );
    assert_eq!(f.workflow.get_emergency_pause_admin(), f.admin);
}

// ── #874: errors ────────────────────────────────────────────────────────────

#[test]
fn every_error_code_round_trips_and_has_a_message() {
    for (i, err) in WorkflowError::ALL.iter().enumerate() {
        let code = err.code();
        assert_eq!(code, i as u32 + 1, "codes must stay contiguous");
        assert_eq!(WorkflowError::from_code(code), Some(*err));
        assert!(!err.message().is_empty());
        assert!(!err.name().is_empty());
        assert_eq!(errors::message_for(code), Some(err.message()));
    }
    assert_eq!(WorkflowError::from_code(0), None);
    assert_eq!(
        errors::message_for(WorkflowError::ALL.len() as u32 + 1),
        None
    );
}

#[test]
fn error_messages_are_readable_on_chain() {
    let env = Env::default();
    let f = setup_with_workflow(&env, true);
    let code = WorkflowError::ResumeTooEarly.code();
    assert_eq!(
        f.workflow.get_error_message(&code),
        Some(String::from_str(
            &env,
            WorkflowError::ResumeTooEarly.message()
        ))
    );
    assert_eq!(f.workflow.get_error_message(&9_999), None);
}

#[test]
fn resume_too_early_is_a_typed_error() {
    let env = Env::default();
    let f = setup_with_workflow(&env, true);
    let target = env.register(InertTarget, ());
    f.workflow
        .initialize_emergency_pause(&f.admin, &vec![&env, target]);
    env.ledger().with_mut(|l| l.timestamp = 100);
    f.workflow.emergency_pause_all(&f.admin);
    assert_eq!(
        f.workflow.try_resume_all(&f.admin, &60),
        Err(Ok(WorkflowError::ResumeTooEarly))
    );
    env.ledger().with_mut(|l| l.timestamp = 160);
    f.workflow.resume_all(&f.admin, &60);
}

// ── #875: TTL ───────────────────────────────────────────────────────────────

#[test]
fn deployment_extends_the_instance_ttl() {
    let env = Env::default();
    let f = setup_with_workflow(&env, true);
    assert_eq!(
        instance_ttl(&env, &f.workflow_contract_id),
        ttl::INSTANCE_TTL_EXTEND_TO
    );
}

#[test]
fn state_changing_entrypoints_re_extend_a_decaying_ttl() {
    let env = Env::default();
    let f = setup_with_workflow(&env, true);
    let decay = ttl::INSTANCE_TTL_EXTEND_TO - ttl::INSTANCE_TTL_THRESHOLD + 1;
    env.ledger().with_mut(|l| l.sequence_number += decay);
    assert!(instance_ttl(&env, &f.workflow_contract_id) < ttl::INSTANCE_TTL_THRESHOLD);

    f.workflow.pause(&f.admin);
    assert_eq!(
        instance_ttl(&env, &f.workflow_contract_id),
        ttl::INSTANCE_TTL_EXTEND_TO
    );
}

#[test]
fn anyone_can_extend_an_idle_workflow() {
    let env = Env::default();
    let f = setup_with_workflow(&env, true);
    let decay = ttl::INSTANCE_TTL_EXTEND_TO - ttl::INSTANCE_TTL_THRESHOLD + 1;
    env.ledger().with_mut(|l| l.sequence_number += decay);

    f.workflow.extend_ttl();
    assert_eq!(
        instance_ttl(&env, &f.workflow_contract_id),
        ttl::INSTANCE_TTL_EXTEND_TO
    );
    // Configuration survives and is still readable.
    assert_eq!(f.workflow.get_admin(), f.admin);
    assert_eq!(f.workflow.get_treasury_id(), f.treasury_contract_id);
}
