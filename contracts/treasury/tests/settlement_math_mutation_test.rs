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

    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000);
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

    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.approve_partial_settlement(&signer_two, &settlement_id, &5_000_000);
    let settlement = client.approve_partial_settlement(&signer_three, &settlement_id, &5_000_000);

    assert_eq!(settlement.approval_weight, 3);
    assert_eq!(settlement.approvals.len(), 3);
    assert!(settlement.approvals.contains(&signer_two));
    assert!(settlement.approvals.contains(&signer_three));
}

#[test]
fn execute_settlement_pays_exact_amount_and_marks_executed() {
    let env = Env::default();
    let (client, admin, merchant, treasury_id) = setup_treasury(&env);
    let token_id = env.register_contract(None, TestToken);
    let token_client = TestTokenClient::new(&env, &token_id);
    token_client.mint(&treasury_id, &10_000_000);

    let settlement_id = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.execute_settlement(&admin, &settlement_id, &token_id);

    let settlement = client.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Executed);
    assert_eq!(token_client.balance(&merchant), 10_000_000);
    assert_eq!(token_client.balance(&treasury_id), 0);
}
