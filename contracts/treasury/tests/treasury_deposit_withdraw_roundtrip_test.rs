use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env, Vec};
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

#[test]
fn batch_deposit_transfers_multiple_tokens_to_treasury() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let depositor = Address::generate(&env);
    let usdc_amount = 5_000_000i128;
    let eurc_amount = 7_000_000i128;

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury_client = TreasuryContractClient::new(&env, &treasury_id);
    treasury_client.initialize(&admin, &1);

    let usdc_id = env.register_contract(None, TestToken);
    let eurc_id = env.register_contract(None, TestToken);
    let usdc = TestTokenClient::new(&env, &usdc_id);
    let eurc = TestTokenClient::new(&env, &eurc_id);

    usdc.mint(&depositor, &usdc_amount);
    eurc.mint(&depositor, &eurc_amount);

    let mut deposits = Vec::new(&env);
    deposits.push_back((usdc_id.clone(), usdc_amount));
    deposits.push_back((eurc_id.clone(), eurc_amount));
    treasury_client.batch_deposit(&depositor, &deposits);

    assert_eq!(usdc.balance(&depositor), 0);
    assert_eq!(eurc.balance(&depositor), 0);
    assert_eq!(usdc.balance(&treasury_id), usdc_amount);
    assert_eq!(eurc.balance(&treasury_id), eurc_amount);
}

#[test]
#[should_panic(expected = "InvalidAmount")]
fn batch_deposit_rejects_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let depositor = Address::generate(&env);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury_client = TreasuryContractClient::new(&env, &treasury_id);
    treasury_client.initialize(&admin, &1);

    let token_id = env.register_contract(None, TestToken);
    let mut deposits = Vec::new(&env);
    deposits.push_back((token_id, 0));

    treasury_client.batch_deposit(&depositor, &deposits);
}
