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

/// A malicious token that re-enters the treasury during transfer to change the
/// merchant payout address, attempting to redirect funds mid-flight.
mod malicious_token {
    use soroban_sdk::{contract, contractimpl, Address, Env};
    use treasury::TreasuryContractClient;

    #[contract]
    pub struct MaliciousToken;

    #[contractimpl]
    impl MaliciousToken {
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
            // Re-enter the treasury to change the merchant's payout address
            let treasury_addr: Address = env
                .storage()
                .instance()
                .get(&("treasury_addr",))
                .unwrap();
            let treasury_client = TreasuryContractClient::new(&env, &treasury_addr);
            let hijack_addr: Address = env
                .storage()
                .instance()
                .get(&("hijack_addr",))
                .unwrap();
            treasury_client.update_merchant_payout_address(&to, &hijack_addr);

            // Now perform the actual transfer
            let from_key = ("bal", from.clone());
            let to_key = ("bal", to.clone());
            let from_bal: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
            let to_bal: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&from_key, &(from_bal - amount));
            env.storage().persistent().set(&to_key, &(to_bal + amount));
        }

        pub fn set_treasury_addr(env: Env, treasury: Address) {
            env.storage()
                .instance()
                .set(&("treasury_addr",), &treasury);
        }

        pub fn set_hijack_addr(env: Env, hijack: Address) {
            env.storage().instance().set(&("hijack_addr",), &hijack);
        }
    }
}

use test_token::{TestToken, TestTokenClient};
use malicious_token::{MaliciousToken, MaliciousTokenClient};

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

#[test]
fn non_merchant_cannot_update_another_merchants_payout_address() {
    let env = Env::default();
    // Do NOT use mock_all_auths — we want real auth checks
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let attacker = Address::generate(&env);
    let payout_override = Address::generate(&env);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury_client = TreasuryContractClient::new(&env, &treasury_id);
    treasury_client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    // Attacker tries to update merchant's payout address — should fail because
    // attacker is not merchant
    let result = treasury_client.try_update_merchant_payout_address(
        &merchant,
        &payout_override,
    );
    assert!(result.is_err());
}

#[test]
fn reentrant_payout_address_change_does_not_redirect_mid_settlement() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let original_payout = Address::generate(&env);
    let hijack_address = Address::generate(&env);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury_client = TreasuryContractClient::new(&env, &treasury_id);
    treasury_client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    // Register a malicious token contract
    let token_id = env.register_contract(None, MaliciousToken);
    let malicious_client = MaliciousTokenClient::new(&env, &token_id);
    malicious_client.set_treasury_addr(&treasury_id);
    malicious_client.set_hijack_addr(&hijack_address);

    // Add the token to the allowlist
    treasury_client.add_allowed_token(&admin, &token_id);

    // Mint tokens to treasury via the malicious token
    malicious_client.mint(&treasury_id, &10_000_000);

    // Set the merchant's payout override to the expected original address
    treasury_client.update_merchant_payout_address(&merchant, &original_payout);

    // Propose settlement
    let settlement_id = treasury_client.propose_settlement(&admin, &merchant, &10_000_000);

    // Execute settlement — the malicious token's transfer will re-enter the
    // treasury and try to change the payout address, but the already-read
    // payout_address should win.
    treasury_client.execute_settlement(&admin, &settlement_id, &token_id);

    // The funds should have gone to the original_payout address, not the hijack address
    assert_eq!(malicious_client.balance(&original_payout), 10_000_000);
    assert_eq!(malicious_client.balance(&hijack_address), 0);
    assert_eq!(malicious_client.balance(&merchant), 0);
}
