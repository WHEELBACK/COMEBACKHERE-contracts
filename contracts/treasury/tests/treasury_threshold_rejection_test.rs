use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env, threshold: u32) -> (TreasuryContractClient, Address, Address, Address) {
    let admin = Address::generate(env);
    let merchant = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &threshold, &soroban_sdk::Vec::new(env));
    let token_id = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(env, &token_id).mint(&contract_id, &100_000_000);
    (client, admin, merchant, token_id)
}

// Zero additional approvals: proposer's weight (1) < threshold (2) → ThresholdNotMet.
#[test]
#[should_panic(expected = "ThresholdNotMet")]
fn test_zero_additional_approvals_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, merchant, token_id) = setup(&env, 2);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    // approval_weight = 1 (only proposer), threshold = 2 → must panic
    client.execute_settlement(&admin, &sid, &token_id);
}

// Approvals below threshold: two signers each with weight 1, threshold 3 → ThresholdNotMet.
#[test]
#[should_panic(expected = "ThresholdNotMet")]
fn test_below_threshold_panics() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, merchant, token_id) = setup(&env, 3);
    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.approve_settlement(&backup, &sid);
    // approval_weight = 2 (admin + backup), threshold = 3 → must panic
    client.execute_settlement(&admin, &sid, &token_id);
}

// threshold - 1 approvals: propose first, then accumulate threshold-1 approvals.
#[test]
#[should_panic(expected = "ThresholdNotMet")]
fn test_threshold_minus_one_approvals_panics() {
    let env = Env::default();
    env.mock_all_auths();
    // threshold = 4; we will collect 3 approvals (= threshold - 1)
    let (client, admin, merchant, token_id) = setup(&env, 4);
    let signer_b = Address::generate(&env);
    let signer_c = Address::generate(&env);
    client.set_signer(&admin, &signer_b, &1);
    client.set_signer(&admin, &signer_c, &1);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.approve_settlement(&signer_b, &sid);
    client.approve_settlement(&signer_c, &sid);
    // approval_weight = 3 (admin + b + c), threshold = 4 → must panic
    client.execute_settlement(&admin, &sid, &token_id);
}

// Exactly at threshold: approval_weight == threshold → execution succeeds.
#[test]
fn test_exactly_at_threshold_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    // threshold = 2; admin proposes (weight 1) + backup approves (weight 1) = 2
    let (client, admin, merchant, token_id) = setup(&env, 2);
    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.approve_settlement(&backup, &sid);
    // approval_weight = 2 == threshold = 2 → must succeed
    client.execute_settlement(&admin, &sid, &token_id);
    let pending = client.get_pending_settlements();
    assert_eq!(
        pending.len(),
        0,
        "executed settlement still in pending list"
    );
}

// Threshold of 1: proposer alone satisfies it → immediate execution.
#[test]
fn test_threshold_one_proposer_satisfies_alone() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, merchant, token_id) = setup(&env, 1);
    let sid = client.propose_settlement(&admin, &merchant, &5_000_000);
    // approval_weight = 1 (admin), threshold = 1 → must execute cleanly
    client.execute_settlement(&admin, &sid, &token_id);
    let pending = client.get_pending_settlements();
    assert_eq!(pending.len(), 0);
}

// Unanimous threshold: all registered signers must approve.
#[test]
#[should_panic(expected = "ThresholdNotMet")]
fn test_unanimous_threshold_partial_approvals_panics() {
    let env = Env::default();
    env.mock_all_auths();
    // 3 signers, threshold = 3 (unanimous)
    let (client, admin, merchant, token_id) = setup(&env, 3);
    let signer_b = Address::generate(&env);
    client.set_signer(&admin, &signer_b, &1);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    // Only admin + signer_b approve (weight 2), signer_c absent → weight 2 < 3
    client.approve_settlement(&signer_b, &sid);
    client.execute_settlement(&admin, &sid, &token_id);
}

// Unanimous threshold: all three approve → execution succeeds.
#[test]
fn test_unanimous_threshold_all_approved_succeeds() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, merchant, token_id) = setup(&env, 3);
    let signer_b = Address::generate(&env);
    let signer_c = Address::generate(&env);
    client.set_signer(&admin, &signer_b, &1);
    client.set_signer(&admin, &signer_c, &1);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.approve_settlement(&signer_b, &sid);
    client.approve_settlement(&signer_c, &sid);
    // approval_weight = 3 == threshold = 3 → must succeed
    client.execute_settlement(&admin, &sid, &token_id);
    assert_eq!(
        client.get_pending_settlements().len(),
        0,
        "settlement still pending after unanimous approval"
    );
}

// Threshold with weighted signers: single high-weight signer satisfies alone.
#[test]
fn test_weighted_signer_satisfies_threshold_alone() {
    let env = Env::default();
    env.mock_all_auths();
    // threshold = 5; grant admin weight 5
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &5, &soroban_sdk::Vec::new(&env));
    // override admin weight to 5
    client.set_signer(&admin, &admin, &5);
    let token_id = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(&env, &token_id).mint(&contract_id, &10_000_000);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.execute_settlement(&admin, &sid, &token_id);
    let settlement = client.get_pending_settlements();
    assert_eq!(settlement.len(), 0);
}

// Settlement status remains Pending while below threshold.
#[test]
fn test_status_stays_pending_when_below_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, merchant, _token_id) = setup(&env, 3);
    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    let settlement = client.approve_settlement(&backup, &sid);
    // weight = 2, threshold = 3 — not yet executable
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.approvals.len(), 2);
}
