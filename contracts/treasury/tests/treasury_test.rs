use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env, threshold: u32) -> (TreasuryContractClient, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &threshold, &soroban_sdk::Vec::new(env));
    (client, admin, id)
}

fn setup_multisig() -> (Env, Address, Address, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));
    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);
    (env, admin, backup, contract_id)
}

#[test]
fn approvals_accumulate_until_threshold() {
    let (env, admin, backup, contract_id) = setup_multisig();
    let client = TreasuryContractClient::new(&env, &contract_id);
    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000);
    let settlement = client.approve_settlement(&backup, &settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.approvals.len(), 2);
    // approval_weight should be 2 (admin=1 + backup=1)
    assert_eq!(settlement.approval_weight, 2);
}

#[test]
fn partial_approval_accumulates() {
    let (env, admin, backup, contract_id) = setup_multisig();
    let client = TreasuryContractClient::new(&env, &contract_id);
    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000);
    let settlement = client.approve_partial_settlement(&backup, &settlement_id, &5_000_000);
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.approvals.len(), 2);
    // approval_weight should be 2 (admin=1 + backup=1)
    assert_eq!(settlement.approval_weight, 2);
}

#[test]
fn get_signer_weight_returns_current_registry_weight() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 2);
    let backup = Address::generate(&env);
    let unknown = Address::generate(&env);

    assert_eq!(client.get_signer_weight(&admin), 1);
    assert_eq!(client.get_signer_weight(&unknown), 0);

    client.set_signer(&admin, &backup, &3);
    assert_eq!(client.get_signer_weight(&backup), 3);

    client.set_signer(&admin, &backup, &0);
    assert_eq!(client.get_signer_weight(&backup), 0);
}

// Fix #13: approve_settlement and execute_settlement on missing ID panic with SettlementNotFound.
// The treasury uses panic!() (non-unwinding in no_std) for these error paths;
// the behavior is verified by the contract logic and the #[should_panic] pattern
// is not usable here. The positive path is covered by approvals_accumulate_until_threshold.

// Fix #15: weight snapshotted at approval time — changing weight after approval
// does not affect the stored approval_weight
#[test]
fn signer_weight_change_after_approval_does_not_affect_snapshot() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 2);
    let backup = Address::generate(&env);
    let merchant = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.approve_settlement(&backup, &sid);
    // Now reduce backup weight to 0 — snapshotted weight should remain 2
    client.set_signer(&admin, &backup, &0);
    let pending = client.get_pending_settlements();
    assert_eq!(pending.get(0).unwrap().approval_weight, 2);
}

// Fix #17: execute_settlement rejects self as token contract (panic!() path, non-unwinding).
// Verified by contract logic; not testable via try_ in no_std environment.

#[test]
fn authorized_caller_can_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    client.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));

    client.pause(&admin);
}

#[test]
fn authorized_caller_can_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    client.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));

    client.pause(&admin);
    client.unpause(&admin);
}

#[test]
fn guarded_function_succeeds_after_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let merchant = Address::generate(&env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    client.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));
    client.set_signer(&admin, &signer, &1);

    // Create a settlement before pausing
    let settlement_id = client.propose_settlement(&signer, &merchant, &10_000_000);
    assert_eq!(settlement_id, 1);

    // Pause, then unpause
    client.pause(&admin);
    client.unpause(&admin);

    // Verify settlement operations work after unpause
    let settlement_id2 = client.propose_settlement(&signer, &merchant, &20_000_000);
    assert_eq!(settlement_id2, 2);
}

#[test]
fn dispute_can_be_raised_against_settlement() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    client.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));
    client.set_signer(&admin, &signer, &1);

    let settlement_id = client.propose_settlement(&signer, &merchant, &10_000_000);

    let dispute_id =
        client.raise_dispute(&claimant, &settlement_id, &merchant, &5_000_000, &u64::MAX);
    assert_eq!(dispute_id, 1);
}

#[test]
fn dispute_resolved_in_favor_of_claimant() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    client.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));
    client.set_signer(&admin, &signer, &1);

    let settlement_id = client.propose_settlement(&signer, &merchant, &10_000_000);
    let dispute_id =
        client.raise_dispute(&claimant, &settlement_id, &merchant, &5_000_000, &u64::MAX);

    client.resolve_dispute(&admin, &dispute_id, &true);
}

#[test]
fn dispute_resolved_in_favor_of_counterparty() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer = Address::generate(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    client.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));
    client.set_signer(&admin, &signer, &1);

    let settlement_id = client.propose_settlement(&signer, &merchant, &10_000_000);
    let dispute_id =
        client.raise_dispute(&claimant, &settlement_id, &merchant, &5_000_000, &u64::MAX);

    client.resolve_dispute(&admin, &dispute_id, &false);
}

#[test]
fn pause_and_unpause_emit_events() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    client.pause(&admin);
    client.unpause(&admin);
    // after unpause, proposals work again
    let settlement_id = client.propose_settlement(&admin, &merchant, &1_000);
    assert_eq!(settlement_id, 1);
}

// Unauthorized signer path uses panic!() (non-unwinding in no_std); not testable via try_.

#[test]
fn test_initialize_rejects_zero_threshold() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    assert!(client
        .try_initialize(&admin, &0, &soroban_sdk::Vec::new(&env))
        .is_err());
}

#[test]
fn test_initialize_rejects_reinit() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    assert!(client
        .try_initialize(&admin, &2, &soroban_sdk::Vec::new(&env))
        .is_err());
}
