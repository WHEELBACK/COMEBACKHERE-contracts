use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient, SettlementStatus};

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

#[test]
fn propose_settlement_accepts_valid_amount() {
    let env = Env::default();
    env.mock_all_auths();
    
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury_client = TreasuryContractClient::new(&env, &treasury_id);
    treasury_client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    
    // Should succeed with valid amount
    let settlement_id = treasury_client.propose_settlement(&admin, &merchant, &10_000_000);
    assert!(settlement_id > 0);
    
    let settlement = treasury_client.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.amount, 10_000_000);
}

#[test]
fn approve_settlement_succeeds_with_valid_state() {
    let env = Env::default();
    env.mock_all_auths();
    
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury_client = TreasuryContractClient::new(&env, &treasury_id);
    treasury_client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    
    let settlement_id = treasury_client.propose_settlement(&admin, &merchant, &10_000_000);
    let settlement = treasury_client.approve_settlement(&admin, &settlement_id);
    
    assert_eq!(settlement.status, SettlementStatus::Pending);
    assert_eq!(settlement.approval_weight, 1);
}

#[test]
fn execute_settlement_succeeds_with_valid_approval() {
    let env = Env::default();
    env.mock_all_auths();
    
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury_client = TreasuryContractClient::new(&env, &treasury_id);
    treasury_client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    
    let token_id = env.register_contract(None, TestToken);
    let test_token_client = TestTokenClient::new(&env, &token_id);
    
    // Mint tokens to treasury
    test_token_client.mint(&treasury_id, &10_000_000);
    
    let settlement_id = treasury_client.propose_settlement(&admin, &merchant, &10_000_000);
    treasury_client.execute_settlement(&admin, &settlement_id, &token_id);
    
    let settlement = treasury_client.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Executed);
}
