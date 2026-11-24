//! #747: deposit reference ids must be unique within their TTL window.
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Bytes, Env,
};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

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

struct Setup {
    env: Env,
    client: TreasuryContractClient<'static>,
    depositor: Address,
    token: Address,
}

fn setup() -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let depositor = Address::generate(&env);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &treasury_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    let token = env.register_contract(None, TestToken);
    TestTokenClient::new(&env, &token).mint(&depositor, &1_000_000);
    Setup { env, client, depositor, token }
}

fn reference(env: &Env, s: &str) -> Option<Bytes> {
    Some(Bytes::from_slice(env, s.as_bytes()))
}

#[test]
fn deposit_rejects_duplicate_reference_id() {
    let s = setup();
    let r = reference(&s.env, "ref-0001");
    s.client.deposit(&s.depositor, &s.token, &100, &r);
    assert_eq!(
        s.client.try_deposit(&s.depositor, &s.token, &100, &r),
        Err(Ok(TreasuryError::DuplicateReferenceId))
    );
}

#[test]
fn batch_deposit_rejects_reference_used_by_deposit() {
    let s = setup();
    let r = reference(&s.env, "ref-0002");
    s.client.deposit(&s.depositor, &s.token, &100, &r);
    let batch = soroban_sdk::vec![&s.env, (s.token.clone(), 50i128)];
    assert_eq!(
        s.client.try_batch_deposit(&s.depositor, &batch, &r),
        Err(Ok(TreasuryError::DuplicateReferenceId))
    );
}

#[test]
fn deposit_allows_reference_id_after_ttl() {
    let s = setup();
    let r = reference(&s.env, "ref-0003");
    s.client.deposit(&s.depositor, &s.token, &100, &r);

    // Advance the ledger past the reference-id TTL window (~30 days of ledgers).
    s.env.ledger().with_mut(|li| {
        li.sequence_number += 600_000;
        li.timestamp += 600_000 * 5;
    });

    assert!(s.client.try_deposit(&s.depositor, &s.token, &100, &r).is_ok());
}

#[test]
fn deposit_allows_distinct_and_absent_reference_ids() {
    let s = setup();
    s.client.deposit(&s.depositor, &s.token, &100, &reference(&s.env, "ref-a"));
    s.client.deposit(&s.depositor, &s.token, &100, &reference(&s.env, "ref-b"));
    // No reference: never deduplicated.
    s.client.deposit(&s.depositor, &s.token, &100, &None);
    s.client.deposit(&s.depositor, &s.token, &100, &None);
}
