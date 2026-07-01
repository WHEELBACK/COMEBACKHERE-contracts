use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

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
fn execute_settlement_uses_merchant_payout_override() {
    let env = Env::default();
    env.mock_all_auths();
    
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payout_override = Address::generate(&env);
    
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury_client = TreasuryContractClient::new(&env, &treasury_id);
    treasury_client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    
    let token_id = env.register_contract(None, TestToken);
    let test_token_client = TestTokenClient::new(&env, &token_id);
    
    // Mint tokens to treasury
    test_token_client.mint(&treasury_id, &10_000_000);
    
    // Update merchant payout address to override
    treasury_client.update_merchant_payout_address(&merchant, &payout_override);
    
    // Propose and execute settlement
    let settlement_id = treasury_client.propose_settlement(&admin, &merchant, &10_000_000);
    treasury_client.execute_settlement(&admin, &settlement_id, &token_id);
    
    // Verify tokens were sent to payout override, not merchant
    assert_eq!(test_token_client.balance(&payout_override), 10_000_000);
    assert_eq!(test_token_client.balance(&merchant), 0);
}
