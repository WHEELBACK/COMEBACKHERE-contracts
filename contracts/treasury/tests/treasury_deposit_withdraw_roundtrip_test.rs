use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
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
fn deposit_withdraw_roundtrip() {
    let env = Env::default();
    env.mock_all_auths();
    
    let admin = Address::generate(&env);
    let depositor = Address::generate(&env);
    let amount = 5_000_000i128;
    
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury_client = TreasuryContractClient::new(&env, &treasury_id);
    treasury_client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    
    let token_id = env.register_contract(None, TestToken);
    let test_token_client = TestTokenClient::new(&env, &token_id);
    
    // Mint tokens to depositor and treasury
    test_token_client.mint(&depositor, &amount);
    test_token_client.mint(&treasury_id, &amount);
    
    // Verify initial balances
    assert_eq!(test_token_client.balance(&depositor), amount);
    assert_eq!(test_token_client.balance(&treasury_id), amount);
    
    // Deposit
    treasury_client.deposit(&depositor, &token_id, &amount);
    
    // Verify balances after deposit
    assert_eq!(test_token_client.balance(&depositor), 0);
    assert_eq!(test_token_client.balance(&treasury_id), amount * 2);
    
    // Withdraw
    treasury_client.withdraw(&depositor, &token_id, &amount);
    
    // Verify balances after withdraw
    assert_eq!(test_token_client.balance(&depositor), amount);
    assert_eq!(test_token_client.balance(&treasury_id), amount);
}
