#![no_main]

// #872: coverage-guided counterpart of `tests/malicious_caller_fuzz_test.rs`.
//
// Each input is a sequence of (attacker, action, argument) steps thrown at a
// fully-configured workflow. Auth is mocked for every address, so the only
// defence is the workflow's own stored-admin checks. After every step the
// harness asserts that no attacker-reachable state moved and that
// `verify_invariants` (#873) still holds; any assertion failure or unexpected
// panic is a finding.

use arbitrary::Arbitrary;
use compliance::{ComplianceContract, ComplianceContractClient};
use libfuzzer_sys::fuzz_target;
use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, vec, Address, Env, Vec};
use treasury::{TreasuryContract, TreasuryContractClient};

#[contract]
struct InertTarget;

#[contractimpl]
impl InertTarget {
    pub fn pause(_env: Env, _admin: Address) {}
    pub fn unpause(_env: Env, _admin: Address) {}
}

#[derive(Debug, Arbitrary)]
struct Step {
    attacker_idx: u8,
    action: u8,
    arg: u64,
}

#[derive(Debug, Arbitrary)]
struct Input {
    steps: std::vec::Vec<Step>,
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let compliance_id = env.register(ComplianceContract, ());
    ComplianceContractClient::new(&env, &compliance_id).initialize(&admin);
    let treasury_id = env.register(TreasuryContract, ());
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &Vec::new(&env));

    let workflow_id = env.register(
        SettlementWorkflowContract,
        (admin.clone(), compliance_id.clone(), treasury_id.clone()),
    );
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    treasury.set_signer(&admin, &workflow_id, &1);
    let token = env.register_stellar_asset_contract(admin.clone());
    let target = env.register(InertTarget, ());
    workflow.initialize_emergency_pause(&admin, &vec![&env, target]);

    let mut attackers: std::vec::Vec<Address> = (0..4).map(|_| Address::generate(&env)).collect();
    attackers.extend([
        compliance_id.clone(),
        treasury_id.clone(),
        workflow_id.clone(),
        token.clone(),
    ]);

    for step in input.steps.iter().take(32) {
        let attacker = attackers[step.attacker_idx as usize % attackers.len()].clone();
        match step.action % 9 {
            0 => assert!(workflow.try_pause(&attacker).is_err()),
            1 => assert!(workflow.try_unpause(&attacker).is_err()),
            2 => assert!(workflow.try_transfer_admin(&attacker, &attacker).is_err()),
            3 => assert!(workflow.try_accept_admin(&attacker).is_err()),
            4 => assert!(workflow.try_emergency_pause_all(&attacker).is_err()),
            5 => assert!(workflow.try_resume_all(&attacker, &step.arg).is_err()),
            6 => assert!(workflow
                .try_initialize_emergency_pause(&attacker, &vec![&env, token.clone()])
                .is_err()),
            7 => assert!(workflow
                .try_execute_with_compliance(&step.arg, &token, &attacker)
                .is_err()),
            _ => {
                let ids = vec![&env, step.arg, step.arg.wrapping_add(1)];
                if let Ok(Ok(executed)) =
                    workflow.try_execute_with_compliance_batch(&ids, &token, &attacker)
                {
                    assert!(executed.is_empty());
                }
            }
        }
        assert_eq!(workflow.get_admin(), admin);
        assert!(!workflow.is_paused());
        assert_eq!(workflow.get_emergency_paused_at(), None);
        assert_eq!(workflow.get_emergency_pause_admin(), admin);
        assert_eq!(workflow.try_verify_invariants(), Ok(Ok(())));
    }
});
