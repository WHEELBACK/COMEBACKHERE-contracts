//! Budget test for `get_pending_metrics` (#743).
//!
//! `get_pending_metrics` aggregates over every pending settlement and token.
//! This test sets up 100 pending settlements across 10 tokens and records the
//! CPU instructions and memory consumed by the call via `env.budget()`.
//!
//! The measured numbers are recorded in `docs/economic-parameters.md`.

use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, Vec};
use treasury::{MaybeAddress, TreasuryContract, TreasuryContractClient};

const NUM_TOKENS: u32 = 10;
const NUM_SETTLEMENTS: u32 = 100;

fn setup(env: &Env) -> (TreasuryContractClient<'_>, Vec<Address>) {
    env.mock_all_auths();

    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);

    let admin = Address::generate(env);
    let signer = Address::generate(env);

    let mut signers = Vec::new(env);
    signers.push_back((signer.clone(), 1u32));

    client.initialize(&admin, &1u32, &signers);

    // Allowlist NUM_TOKENS distinct tokens.
    let mut tokens = Vec::new(env);
    for _ in 0..NUM_TOKENS {
        let token = Address::generate(env);
        client.add_allowed_token(&admin, &token);
        tokens.push_back(token);
    }

    // Propose NUM_SETTLEMENTS pending settlements spread across the tokens.
    for i in 0..NUM_SETTLEMENTS {
        let token = tokens.get(i % NUM_TOKENS).unwrap();
        let recipient = Address::generate(env);
        client.propose_settlement_with_token(
            &signer,
            &recipient,
            &1_000i128,
            &MaybeAddress::Some(token),
        );
    }

    (client, tokens)
}

#[test]
fn pending_metrics_budget_many_tokens_and_settlements() {
    let env = Env::default();
    let (client, _tokens) = setup(&env);

    // Reset the budget so we only measure the `get_pending_metrics` call.
    env.budget().reset_unlimited();

    let metrics = client.get_pending_metrics();

    let cpu = env.budget().cpu_instruction_cost();
    let mem = env.budget().memory_bytes_cost();

    // Sanity: the aggregation must have observed every pending settlement.
    let pending_count: u64 = metrics.iter().map(|(_, count, _)| count).sum();
    assert_eq!(pending_count, NUM_SETTLEMENTS as u64);
    assert_eq!(metrics.len(), NUM_TOKENS);

    // Record the measured numbers for the PR / docs cross-reference.
    std::println!(
        "get_pending_metrics: {} settlements across {} tokens -> cpu={} mem={}",
        NUM_SETTLEMENTS, NUM_TOKENS, cpu, mem
    );
}
