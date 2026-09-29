//! Tests for `withdraw_all` across multiple allowed tokens.
//!
//! These tests exercise the multi-token behaviour of `withdraw_all`, ensuring
//! that every allowed token balance is emptied while respecting withdrawal
//! limits and the pause state. They reuse the mock tokens from the reentrancy
//! suite helpers.

use comebackhere_treasury::{TreasuryContract, TreasuryContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

mod reentrancy_helpers {
    //! Minimal mock token helpers mirroring the reentrancy suite.
    use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

    #[contracttype]
    #[derive(Clone)]
    pub struct AllowanceValue {
        pub amount: i128,
        pub expiration_ledger: u32,
    }

    #[contracttype]
    #[derive(Clone)]
    pub struct AllowanceKey {
        pub from: Address,
        pub spender: Address,
    }

    #[contract]
    pub struct MockToken;

    #[contractimpl]
    impl MockToken {
        pub fn mint(env: Env, to: Address, amount: i128) {
            let key = to.clone();
            let balance: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            env.storage().persistent().set(&key, &(balance + amount));
        }

        pub fn balance(env: Env, id: Address) -> i128 {
            env.storage().persistent().get(&id).unwrap_or(0)
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            let from_balance: i128 = env.storage().persistent().get(&from).unwrap_or(0);
            assert!(from_balance >= amount, "insufficient balance");
            env.storage().persistent().set(&from, &(from_balance - amount));
            let to_balance: i128 = env.storage().persistent().get(&to).unwrap_or(0);
            env.storage().persistent().set(&to, &(to_balance + amount));
        }

        pub fn approve(
            env: Env,
            from: Address,
            spender: Address,
            amount: i128,
            expiration_ledger: u32,
        ) {
            from.require_auth();
            let key = AllowanceKey {
                from: from.clone(),
                spender: spender.clone(),
            };
            env.storage().persistent().set(
                &key,
                &AllowanceValue {
                    amount,
                    expiration_ledger,
                },
            );
        }

        pub fn allowance(env: Env, from: Address, spender: Address) -> i128 {
            let key = AllowanceKey { from, spender };
            let value: Option<AllowanceValue> = env.storage().persistent().get(&key);
            match value {
                Some(v) if v.expiration_ledger >= env.ledger().sequence() => v.amount,
                _ => 0,
            }
        }

        pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
            spender.require_auth();
            let key = AllowanceKey {
                from: from.clone(),
                spender: spender.clone(),
            };
            let value: AllowanceValue = env
                .storage()
                .persistent()
                .get(&key)
                .expect("no allowance");
            assert!(value.expiration_ledger >= env.ledger().sequence(), "allowance expired");
            assert!(value.amount >= amount, "allowance exceeded");
            env.storage().persistent().set(
                &key,
                &AllowanceValue {
                    amount: value.amount - amount,
                    expiration_ledger: value.expiration_ledger,
                },
            );
            let from_balance: i128 = env.storage().persistent().get(&from).unwrap_or(0);
            assert!(from_balance >= amount, "insufficient balance");
            env.storage().persistent().set(&from, &(from_balance - amount));
            let to_balance: i128 = env.storage().persistent().get(&to).unwrap_or(0);
            env.storage().persistent().set(&to, &(to_balance + amount));
        }
    }
}

use reentrancy_helpers::{MockToken, MockTokenClient};

fn setup() -> (Env, TreasuryContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

fn deploy_token(env: &Env) -> (Address, MockTokenClient<'static>) {
    let token_id = env.register_contract(None, MockToken);
    let token = MockTokenClient::new(env, &token_id);
    (token_id, token)
}

#[test]
fn withdraw_all_multi_token_empties_every_allowed_balance() {
    let (env, client, admin) = setup();

    let (token_a_id, token_a) = deploy_token(&env);
    let (token_b_id, token_b) = deploy_token(&env);
    let (token_c_id, token_c) = deploy_token(&env);

    client.allow_token(&admin, &token_a_id);
    client.allow_token(&admin, &token_b_id);
    client.allow_token(&admin, &token_c_id);

    token_a.mint(&client.address, &1_000);
    token_b.mint(&client.address, &2_500);
    token_c.mint(&client.address, &7);

    let recipient = Address::generate(&env);
    client.withdraw_all(&admin, &recipient);

    assert_eq!(token_a.balance(&client.address), 0);
    assert_eq!(token_b.balance(&client.address), 0);
    assert_eq!(token_c.balance(&client.address), 0);

    assert_eq!(token_a.balance(&recipient), 1_000);
    assert_eq!(token_b.balance(&recipient), 2_500);
    assert_eq!(token_c.balance(&recipient), 7);
}

#[test]
fn withdraw_all_multi_token_respects_per_token_limit() {
    let (env, client, admin) = setup();

    let (token_a_id, token_a) = deploy_token(&env);
    let (token_b_id, token_b) = deploy_token(&env);

    client.allow_token(&admin, &token_a_id);
    client.allow_token(&admin, &token_b_id);

    // Token A is under its limit and should be fully withdrawn.
    token_a.mint(&client.address, &500);
    // Token B exceeds its configured withdrawal limit.
    token_b.mint(&client.address, &10_000);

    client.set_withdrawal_limit(&admin, &token_b_id, &1_000);

    let recipient = Address::generate(&env);
    client.withdraw_all(&admin, &recipient);

    // Token A is fully emptied.
    assert_eq!(token_a.balance(&client.address), 0);
    assert_eq!(token_a.balance(&recipient), 500);

    // Token B is capped at its limit; the remainder stays in the treasury.
    assert_eq!(token_b.balance(&recipient), 1_000);
    assert_eq!(token_b.balance(&client.address), 9_000);
}

#[test]
fn withdraw_all_multi_token_skips_removed_token() {
    let (env, client, admin) = setup();

    let (token_a_id, token_a) = deploy_token(&env);
    let (token_b_id, token_b) = deploy_token(&env);

    client.allow_token(&admin, &token_a_id);
    client.allow_token(&admin, &token_b_id);

    token_a.mint(&client.address, &1_000);
    token_b.mint(&client.address, &2_000);

    // Remove token B from the allowlist before withdrawing.
    client.disallow_token(&admin, &token_b_id);

    let recipient = Address::generate(&env);
    client.withdraw_all(&admin, &recipient);

    // Only the still-allowed token is withdrawn.
    assert_eq!(token_a.balance(&client.address), 0);
    assert_eq!(token_a.balance(&recipient), 1_000);

    // The disallowed token is left untouched.
    assert_eq!(token_b.balance(&client.address), 2_000);
    assert_eq!(token_b.balance(&recipient), 0);
}

#[test]
fn withdraw_all_multi_token_blocked_when_paused() {
    let (env, client, admin) = setup();

    let (token_a_id, token_a) = deploy_token(&env);
    let (token_b_id, token_b) = deploy_token(&env);

    client.allow_token(&admin, &token_a_id);
    client.allow_token(&admin, &token_b_id);

    token_a.mint(&client.address, &1_000);
    token_b.mint(&client.address, &2_000);

    client.pause(&admin);

    let recipient = Address::generate(&env);
    let result = client.try_withdraw_all(&admin, &recipient);
    assert!(result.is_err(), "withdraw_all must fail while paused");

    // Balances remain untouched while paused.
    assert_eq!(token_a.balance(&client.address), 1_000);
    assert_eq!(token_b.balance(&client.address), 2_000);
}
