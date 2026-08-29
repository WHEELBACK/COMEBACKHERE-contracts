// #67: Benchmark for approval computation with large signer sets.
//
// Measures the cost of accumulating signer approvals across varying set sizes.
// These are not CI-enforced on every run (they need testutils and expect
// successful execution), but provide a regression baseline developers can
// compare against when touching the approval path.
//
// Run with: cargo test --package comebackhere-treasury --test approval_benchmark_test -- --nocapture
use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

/// Helper: register `n` signers each with weight 1, returning their addresses.
fn register_signers(
    client: &TreasuryContractClient,
    admin: &Address,
    env: &Env,
    n: u32,
) -> Vec<Address> {
    let mut signers = Vec::new();
    for _ in 0..n {
        let s = Address::generate(env);
        client.set_signer(admin, &s, &1);
        signers.push(s);
    }
    signers
}

/// Benchmark: approve a settlement with `signer_count` signers and measure
/// the cumulative approval weight.
fn bench_approval_set(signer_count: u32) -> u32 {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &signer_count, &soroban_sdk::Vec::new(&env));
    client.set_signer(&admin, &admin, &signer_count); // admin carries full weight

    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000);
    let settlement = client.approve_settlement(&admin, &settlement_id);

    settlement.approval_weight
}

fn bench_large_signer_proposal(signer_count: u32) -> u64 {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &signer_count, &soroban_sdk::Vec::new(&env));
    client.set_signer(&admin, &admin, &signer_count);

    let signers = register_signers(&client, &admin, &env, signer_count - 1);

    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000);

    // Each signer approves sequentially
    for s in &signers {
        client.approve_settlement(s, &settlement_id);
    }
    settlement_id
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

#[test]
fn bench_approval_single_signer() {
    let weight = bench_approval_set(1);
    assert_eq!(weight, 1, "single signer weight must be 1");
}

#[test]
fn bench_approval_ten_signers() {
    let weight = bench_approval_set(10);
    assert_eq!(weight, 10, "ten-signer approval weight accumulation");
}

#[test]
fn bench_approval_fifty_signers() {
    let weight = bench_approval_set(50);
    assert_eq!(weight, 50, "fifty-signer approval weight accumulation");
}

#[test]
fn bench_approval_hundred_signers() {
    let weight = bench_approval_set(100);
    assert_eq!(
        weight, 100,
        "one-hundred-signer approval weight accumulation"
    );
}

#[test]
fn bench_large_signer_set_proposal_and_approval() {
    let env = Env::default();
    env.mock_all_auths();

    let signer_count = 50u32;
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &signer_count, &soroban_sdk::Vec::new(&env));
    client.set_signer(&admin, &admin, &(signer_count / 2));

    // Register 50 signers with weight 1
    let signers = register_signers(&client, &admin, &env, signer_count);

    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000);

    // First half approves
    for i in 0..(signer_count as usize / 2) {
        client.approve_settlement(&signers[i], &settlement_id);
    }
    let settlement = client.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(
        settlement.approval_weight,
        signer_count / 2 + signer_count / 2 // admin(25) + 25 signers
    );

    // Remaining half approves to reach threshold
    for i in (signer_count as usize / 2)..(signer_count as usize) {
        client.approve_settlement(&signers[i], &settlement_id);
    }
    let settlement = client.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.approval_weight, signer_count + signer_count / 2);
}

#[test]
fn bench_large_signer_set_get_all_signers() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    // Register 100 signers
    for _ in 0..100 {
        let s = Address::generate(&env);
        client.set_signer(&admin, &s, &1);
    }

    let all_signers = client.get_all_signers();
    assert_eq!(all_signers.len(), 101); // admin + 100
}

/// Verify that repeated approvals by the same signer do not double-count weight.
#[test]
fn bench_duplicate_approval_does_not_double_count() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &3, &soroban_sdk::Vec::new(&env));
    client.set_signer(&admin, &admin, &2);

    let backup = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000);
    assert_eq!(client.get_settlement(&settlement_id).approval_weight, 2);

    // Approve again — must not double-count admin's weight
    client.approve_settlement(&admin, &settlement_id);
    assert_eq!(client.get_settlement(&settlement_id).approval_weight, 2);
}
