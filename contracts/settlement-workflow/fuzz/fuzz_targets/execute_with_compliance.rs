#![no_main]

// Fuzz harness complementing settlement_workflow_test.rs. That test covers the
// two known branches (compliance pass / compliance fail) with hand-picked
// inputs; this harness throws adversarial Address + u64 combinations at
// `execute_with_compliance` to catch panics (overflow, unwrap-on-None, an
// unexpected input combination that lets a caller treat the wrong contract as
// authoritative for compliance/settlement, etc.) that logic-only assertions
// would miss.
//
// This contract sits at a trust boundary: `compliance_id` and `treasury_id` are
// passed per-call, so the parameters that most matter are (a) pointing both
// roles at the same contract instance (`compliance_id == treasury_id`), and (b)
// extreme `settlement_id` values (0, u64::MAX). The harness forces those cases
// and otherwise fuzzes the address selection across a pool that mixes the real
// registered contract IDs with unregistered generated addresses representing
// entities that were never onboarded.
//
// Keep iteration cost low (one workflow call per input) so this stays
// CI-runnable within a bounded time/iteration budget.

use arbitrary::Arbitrary;
use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use treasury::{TreasuryContract, TreasuryContractClient};

#[derive(Debug, Arbitrary)]
struct Input {
    // Indices into the address pool for the per-call contract/entity IDs.
    compliance_idx: u8,
    treasury_idx: u8,
    token_idx: u8,
    merchant_idx: u8,
    // Fuzzed settlement id, optionally forced to a boundary value below.
    settlement_id: u64,
    force_compliance_eq_treasury: bool,
    force_settlement_zero: bool,
    force_settlement_max: bool,
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    // Real, registered contract instances for compliance and treasury, plus a
    // handful of unregistered generated addresses standing in for adversarially
    // chosen contract IDs / merchant entities.
    let compliance_id = env.register_contract(None, ComplianceContract);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let workflow_id = env.register_contract(None, SettlementWorkflowContract);

    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &Vec::new(&env));

    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    // The workflow executes settlements as itself, so it must be an authorized
    // Treasury signer (mirrors the production setup in settlement_workflow_test.rs).
    treasury.set_signer(&admin, &workflow_id, &1);

    // Pool: index 0 = real compliance, index 1 = real treasury, rest unregistered.
    let mut pool: Vec<Address> = Vec::new(&env);
    pool.push_back(compliance_id.clone());
    pool.push_back(treasury_id.clone());
    for _ in 0..6 {
        pool.push_back(Address::generate(&env));
    }

    let pick = |idx: u8| -> Address {
        let i = (idx as usize) % pool.len();
        pool.get(i).unwrap()
    };

    let mut compliance_arg = pick(input.compliance_idx);
    let mut treasury_arg = pick(input.treasury_idx);
    if input.force_compliance_eq_treasury {
        treasury_arg = compliance_arg.clone();
    }
    let token_contract = pick(input.token_idx);
    let merchant = pick(input.merchant_idx);

    let settlement_id = if input.force_settlement_zero {
        0
    } else if input.force_settlement_max {
        u64::MAX
    } else {
        input.settlement_id
    };

    // Only the Result matters here: any Ok/Err is acceptable, a panic is not.
    let _ = workflow.try_execute_with_compliance(
        &compliance_arg,
        &treasury_arg,
        &settlement_id,
        &token_contract,
        &merchant,
    );
});
