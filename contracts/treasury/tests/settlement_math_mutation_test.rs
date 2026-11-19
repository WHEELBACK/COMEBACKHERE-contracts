use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

mod test_token {
    use soroban_sdk::{contract, contractimpl, Address, Env};

    #[contract]
    pub struct TestToken;

    #[contractimpl]
    impl TestToken {
        pub fn mint(env: Env, to: Address, amount: i128) {
            let key = ("bal", to.clone());
            let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            env.storage().persistent().set(&key, &(bal + amount));
        }

        pub fn balance(env: Env, of: Address) -> i128 {
            let key = ("bal", of);
            env.storage().persistent().get(&key).unwrap_or(0)
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            let from_key = ("bal", from.clone());
            let to_key = ("bal", to.clone());
            let from_bal: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
            let to_bal: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&from_key, &(from_bal - amount));
            env.storage().persistent().set(&to_key, &(to_bal + amount));
        }
    }
}

use test_token::{TestToken, TestTokenClient};

fn setup_treasury(env: &Env) -> (TreasuryContractClient, Address, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let merchant = Address::generate(env);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &treasury_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin, merchant, treasury_id)
}

#[test]
fn propose_settlement_records_amount_and_initial_approval_weight() {
    let env = Env::default();
    let (client, admin, merchant, _) = setup_treasury(&env);

    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let settlement = client.get_settlement(&settlement_id);

    assert_eq!(settlement.amount, 10_000_000);
    assert_eq!(settlement.approval_weight, 1);
    assert_eq!(settlement.approvals.len(), 1);
    assert_eq!(settlement.approvals.get(0).unwrap(), admin);
    assert_eq!(settlement.status, SettlementStatus::Pending);
}

#[test]
fn approve_partial_settlement_accepts_exact_remaining_amount() {
    let env = Env::default();
    let (client, admin, merchant, _) = setup_treasury(&env);
    let signer_two = Address::generate(&env);
    let signer_three = Address::generate(&env);
    client.set_signer(&admin, &signer_two, &1);
    client.set_signer(&admin, &signer_three, &1);

    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    client.approve_partial_settlement(&signer_two, &settlement_id, &5_000_000);
    let settlement = client.approve_partial_settlement(&signer_three, &settlement_id, &5_000_000);

    assert_eq!(settlement.approval_weight, 3);
    assert_eq!(settlement.approvals.len(), 3);
    assert!(settlement.approvals.contains(&signer_two));
    assert!(settlement.approvals.contains(&signer_three));
}

#[test]
fn approve_partial_settlement_rejects_cumulative_overflow() {
    let env = Env::default();
    let (client, admin, merchant, _) = setup_treasury(&env);
    let signer_two = Address::generate(&env);
    let signer_three = Address::generate(&env);
    client.set_signer(&admin, &signer_two, &1);
    client.set_signer(&admin, &signer_three, &1);

    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    client.approve_partial_settlement(&signer_two, &settlement_id, &6_000_000);

    // Cumulative approvals (6_000_000 + 5_000_000) exceed the settlement amount.
    let result = client.try_approve_partial_settlement(&signer_three, &settlement_id, &5_000_000);
    assert!(result.is_err());

    // State must be unchanged after the rejected approval.
    let settlement = client.get_settlement(&settlement_id);
    assert_eq!(settlement.approval_weight, 2);
    assert!(!settlement.approvals.contains(&signer_three));
}

#[test]
fn partially_execute_settlement_rejects_amount_above_remaining() {
    let env = Env::default();
    let (client, admin, merchant, treasury_id) = setup_treasury(&env);
    let token_id = env.register_contract(None, TestToken);
    let token_client = TestTokenClient::new(&env, &token_id);
    token_client.mint(&treasury_id, &10_000_000);

    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    client.partially_execute_settlement(&admin, &settlement_id, &4_000_000, &token_id);

    // Remaining is 6_000_000; requesting more must fail without panicking.
    let result =
        client.try_partially_execute_settlement(&admin, &settlement_id, &7_000_000, &token_id);
    assert!(result.is_err());

    let settlement = client.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(token_client.balance(&merchant), 4_000_000);
    assert_eq!(token_client.balance(&treasury_id), 6_000_000);
}

#[test]
fn partial_settlement_math_never_wraps_on_arbitrary_inputs() {
    let env = Env::default();
    let (client, admin, merchant, treasury_id) = setup_treasury(&env);
    let token_id = env.register_contract(None, TestToken);
    let token_client = TestTokenClient::new(&env, &token_id);
    token_client.mint(&treasury_id, &i128::MAX);

    let signer_two = Address::generate(&env);
    client.set_signer(&admin, &signer_two, &1);

    // Deterministic pseudo-random walk over a wide range of totals and partials,
    // including boundary values that would wrap with unchecked arithmetic.
    let totals: [i128; 6] = [1, 2, 1_000, 10_000_000, i128::MAX / 2, i128::MAX];
    let partials: [i128; 6] = [0, 1, 999, 5_000_000, i128::MAX / 2, i128::MAX];

    for &total in totals.iter() {
        for &partial in partials.iter() {
            let settlement_id = client.propose_settlement(&admin, &merchant, &total, &0_u64);

            // Approving more than the total must return a typed error, not panic.
            let approve_result =
                client.try_approve_partial_settlement(&signer_two, &settlement_id, &partial);
            if partial > total {
                assert!(approve_result.is_err());
            }

            // Partially executing more than the total must return a typed error.
            let execute_result =
                client.try_partially_execute_settlement(&admin, &settlement_id, &partial, &token_id);
            if partial > total {
                assert!(execute_result.is_err());
            }

            // The recorded settlement amount is never mutated by rejected calls.
            let settlement = client.get_settlement(&settlement_id);
            assert_eq!(settlement.amount, total);
        }
    }
}

#[test]
fn execute_settlement_pays_exact_amount_and_marks_executed() {
    let env = Env::default();
    let (client, admin, merchant, treasury_id) = setup_treasury(&env);
    let token_id = env.register_contract(None, TestToken);
    let token_client = TestTokenClient::new(&env, &token_id);
    token_client.mint(&treasury_id, &10_000_000);

    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    client.execute_settlement(&admin, &settlement_id, &token_id);

    let settlement = client.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Executed);
    assert_eq!(token_client.balance(&merchant), 10_000_000);
    assert_eq!(token_client.balance(&treasury_id), 0);
}
