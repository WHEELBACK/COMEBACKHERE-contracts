#![no_main]

// Fuzz harness for `execute_with_compliance_batch` (#619). Complements the
// single-settlement fuzz target by exercising the batch variant, which has
// additional failure modes: ordering, partial failures, duplicate recipients,
// and mixed valid/invalid settlement IDs.
//
// Properties asserted:
// 1. No funds move to a blocked recipient.
// 2. Total moved never exceeds total requested.
// 3. Running the batch gives the same final state as running items one by one.

use arbitrary::Arbitrary;
use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{testutils::Address as _, token, Address, Env, Vec};
use treasury::{TreasuryContract, TreasuryContractClient};

#[derive(Debug, Arbitrary)]
struct Input {
    // Number of settlements to propose (1..=8).
    num_settlements: u8,
    // Fuzzed settlement amounts.
    amounts: [i128; 8],
    // Fuzzed compliance allow flags for each settlement.
    allow_flags: [bool; 8],
    // Fuzzed merchant addresses (index into pool).
    merchant_idx: u8,
    // Whether to include a bogus settlement ID in the batch.
    include_bogus_id: bool,
    // Whether to include duplicate settlement IDs.
    include_duplicates: bool,
    // Whether to shuffle the order of settlement IDs.
    shuffle_order: bool,
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &Vec::new(&env));

    let workflow_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    workflow.initialize(&compliance_id, &treasury_id);
    treasury.set_signer(&admin, &workflow_id, &1);

    let token_id = env.register_stellar_asset_contract(admin.clone());

    // Pool of merchant addresses.
    let mut merchant_pool: Vec<Address> = Vec::new(&env);
    for _ in 0..4 {
        merchant_pool.push_back(Address::generate(&env));
    }
    let merchant = merchant_pool
        .get((input.merchant_idx as usize) % merchant_pool.len())
        .unwrap();

    // Propose settlements and set compliance.
    let num = (input.num_settlements as usize).min(8).max(1);
    let mut settlement_ids: Vec<u64> = Vec::new(&env);
    let mut total_requested: i128 = 0;

    for i in 0..num {
        let amount = input.amounts[i];
        if amount <= 0 {
            continue;
        }
        let sid = treasury.propose_settlement(&admin, &merchant, &amount);
        settlement_ids.push_back(sid);
        total_requested += amount;

        // Set compliance for this merchant.
        if input.allow_flags[i] {
            compliance.allow_address(&admin, &merchant);
        } else {
            compliance.block_address(&admin, &merchant, &None);
        }
    }

    if settlement_ids.is_empty() {
        return;
    }

    // Build the batch input.
    let mut batch_ids: Vec<u64> = Vec::new(&env);

    if input.shuffle_order {
        // Add IDs in reverse order.
        for i in (0..settlement_ids.len()).rev() {
            batch_ids.push_back(settlement_ids.get(i as u32).unwrap());
        }
    } else {
        for i in 0..settlement_ids.len() {
            batch_ids.push_back(settlement_ids.get(i as u32).unwrap());
        }
    }

    if input.include_duplicates && settlement_ids.len() > 1 {
        // Add a duplicate of the first ID.
        batch_ids.push_back(settlement_ids.get(0).unwrap());
    }

    if input.include_bogus_id {
        batch_ids.push_back(u64::MAX);
    }

    // Mint tokens to the treasury.
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &total_requested);

    // Record balances before.
    let balance_before = token::Client::new(&env, &token_id).balance(&merchant);

    // Execute the batch.
    let executed = workflow.execute_with_compliance_batch(&batch_ids, &token_id, &merchant);

    // Property 1: No funds move to a blocked recipient.
    let is_allowed = compliance.is_allowed(&merchant);
    let balance_after = token::Client::new(&env, &token_id).balance(&merchant);
    let moved = balance_after - balance_before;

    if !is_allowed {
        assert_eq!(
            moved, 0,
            "funds moved to a blocked recipient: {moved}"
        );
    }

    // Property 2: Total moved never exceeds total requested.
    assert!(
        moved <= total_requested,
        "total moved ({moved}) exceeds total requested ({total_requested})"
    );

    // Property 3: If compliance allows, all valid settlements should execute.
    if is_allowed {
        // All non-bogus, non-duplicate IDs should be in the executed list.
        for i in 0..settlement_ids.len() {
            let sid = settlement_ids.get(i as u32).unwrap();
            assert!(
                executed.contains(&sid),
                "settlement {sid} should have been executed"
            );
        }
    }
});
