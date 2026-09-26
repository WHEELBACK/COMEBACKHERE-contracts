/// Tests for #591: deposit and batch_deposit must reject zero and negative amounts
/// before any token call is made, using typed TreasuryError::InvalidAmount.
///
/// These tests live alongside `treasury_deposit_withdraw_roundtrip_test.rs` and
/// share the same lightweight in-process TestToken stub.
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env, Vec};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

// ---------------------------------------------------------------------------
// Minimal in-process test token (mirrors the stub in
// treasury_deposit_withdraw_roundtrip_test.rs).
// ---------------------------------------------------------------------------
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

        pub fn transfer(env: Env, from: Address, _to: Address, amount: i128) {
            // Require auth so the treasury's transfer call path is exercised.
            from.require_auth();
            let from_key = ("bal", from.clone());
            let from_bal: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&from_key, &(from_bal - amount));
        }
    }
}

use test_token::{TestToken, TestTokenClient};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup(env: &Env) -> (TreasuryContractClient, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let depositor = Address::generate(env);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &treasury_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));

    (client, admin, depositor)
}

fn register_token(env: &Env) -> Address {
    env.register_contract(None, TestToken)
}

// ---------------------------------------------------------------------------
// deposit — zero amount
// ---------------------------------------------------------------------------

/// `deposit` with amount == 0 must return `InvalidAmount`.
/// The treasury must reject this *before* any token transfer is attempted so
/// that a zero-value transfer does not reach the token contract (token contracts
/// differ in how they handle zero-value transfers).
#[test]
fn deposit_rejects_zero_amount() {
    let env = Env::default();
    let (client, _admin, depositor) = setup(&env);
    let token_id = register_token(&env);

    let result = client.try_deposit(&depositor, &token_id, &0i128);
    assert_eq!(result, Err(Ok(TreasuryError::InvalidAmount)));
}

// ---------------------------------------------------------------------------
// deposit — negative amount
// ---------------------------------------------------------------------------

/// `deposit` with a negative amount must return `InvalidAmount`.
#[test]
fn deposit_rejects_negative_amount() {
    let env = Env::default();
    let (client, _admin, depositor) = setup(&env);
    let token_id = register_token(&env);

    let result = client.try_deposit(&depositor, &token_id, &-1i128);
    assert_eq!(result, Err(Ok(TreasuryError::InvalidAmount)));
}

/// `deposit` with the most-negative i128 value must return `InvalidAmount`.
#[test]
fn deposit_rejects_min_i128() {
    let env = Env::default();
    let (client, _admin, depositor) = setup(&env);
    let token_id = register_token(&env);

    let result = client.try_deposit(&depositor, &token_id, &i128::MIN);
    assert_eq!(result, Err(Ok(TreasuryError::InvalidAmount)));
}

// ---------------------------------------------------------------------------
// deposit — minimal positive amount (boundary)
// ---------------------------------------------------------------------------

/// `deposit` with amount == 1 (the smallest valid positive amount) must succeed.
/// This confirms the rejection boundary is `<= 0`, not `< some_minimum`.
#[test]
fn deposit_accepts_minimal_positive_amount() {
    let env = Env::default();
    let (client, _admin, depositor) = setup(&env);
    let token_id = register_token(&env);
    let token_client = TestTokenClient::new(&env, &token_id);

    token_client.mint(&depositor, &1i128);

    let result = client.try_deposit(&depositor, &token_id, &1i128);
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_balance(&depositor, &token_id), 1i128);
}

// ---------------------------------------------------------------------------
// batch_deposit — zero amount in a single-item batch
// ---------------------------------------------------------------------------

/// `batch_deposit` with a single entry whose amount is 0 must return `InvalidAmount`.
#[test]
fn batch_deposit_rejects_zero_amount_single_entry() {
    let env = Env::default();
    let (client, _admin, depositor) = setup(&env);
    let token_id = register_token(&env);

    let mut deposits = Vec::new(&env);
    deposits.push_back((token_id, 0i128));

    let result = client.try_batch_deposit(&depositor, &deposits);
    assert_eq!(result, Err(Ok(TreasuryError::InvalidAmount)));
}

// ---------------------------------------------------------------------------
// batch_deposit — negative amount in a single-item batch
// ---------------------------------------------------------------------------

/// `batch_deposit` with a single entry whose amount is negative must return `InvalidAmount`.
#[test]
fn batch_deposit_rejects_negative_amount_single_entry() {
    let env = Env::default();
    let (client, _admin, depositor) = setup(&env);
    let token_id = register_token(&env);

    let mut deposits = Vec::new(&env);
    deposits.push_back((token_id, -500i128));

    let result = client.try_batch_deposit(&depositor, &deposits);
    assert_eq!(result, Err(Ok(TreasuryError::InvalidAmount)));
}

// ---------------------------------------------------------------------------
// batch_deposit — zero amount in a mixed-validity multi-entry batch
// ---------------------------------------------------------------------------

/// When a `batch_deposit` contains a valid first entry followed by a zero-amount
/// entry, the call must fail with `InvalidAmount`.  The first valid entry must NOT
/// have been persisted (the treasury rolls back on the first invalid entry encountered).
#[test]
fn batch_deposit_rejects_zero_amount_in_mixed_batch() {
    let env = Env::default();
    let (client, _admin, depositor) = setup(&env);

    let token_a = register_token(&env);
    let token_b = register_token(&env);

    let token_a_client = TestTokenClient::new(&env, &token_a);
    token_a_client.mint(&depositor, &1_000i128);

    let mut deposits = Vec::new(&env);
    deposits.push_back((token_a.clone(), 1_000i128)); // valid
    deposits.push_back((token_b.clone(), 0i128)); // invalid — triggers error

    let result = client.try_batch_deposit(&depositor, &deposits);
    assert_eq!(result, Err(Ok(TreasuryError::InvalidAmount)));
}

// ---------------------------------------------------------------------------
// batch_deposit — negative amount in a multi-entry batch
// ---------------------------------------------------------------------------

/// `batch_deposit` with a negative amount in the second entry must return `InvalidAmount`.
#[test]
fn batch_deposit_rejects_negative_amount_in_multi_entry_batch() {
    let env = Env::default();
    let (client, _admin, depositor) = setup(&env);

    let token_a = register_token(&env);
    let token_b = register_token(&env);

    let token_a_client = TestTokenClient::new(&env, &token_a);
    token_a_client.mint(&depositor, &5_000i128);

    let mut deposits = Vec::new(&env);
    deposits.push_back((token_a.clone(), 5_000i128)); // valid
    deposits.push_back((token_b.clone(), -1i128)); // invalid

    let result = client.try_batch_deposit(&depositor, &deposits);
    assert_eq!(result, Err(Ok(TreasuryError::InvalidAmount)));
}

// ---------------------------------------------------------------------------
// batch_deposit — all-zero multi-entry batch
// ---------------------------------------------------------------------------

/// Every entry in the batch has amount == 0; the whole call must return `InvalidAmount`.
#[test]
fn batch_deposit_rejects_all_zero_entries() {
    let env = Env::default();
    let (client, _admin, depositor) = setup(&env);

    let token_a = register_token(&env);
    let token_b = register_token(&env);

    let mut deposits = Vec::new(&env);
    deposits.push_back((token_a, 0i128));
    deposits.push_back((token_b, 0i128));

    let result = client.try_batch_deposit(&depositor, &deposits);
    assert_eq!(result, Err(Ok(TreasuryError::InvalidAmount)));
}

// ---------------------------------------------------------------------------
// batch_deposit — minimal positive amount (boundary)
// ---------------------------------------------------------------------------

/// `batch_deposit` with two entries each carrying amount == 1 must succeed.
#[test]
fn batch_deposit_accepts_minimal_positive_amounts() {
    let env = Env::default();
    let (client, _admin, depositor) = setup(&env);

    let token_a = register_token(&env);
    let token_b = register_token(&env);

    let token_a_client = TestTokenClient::new(&env, &token_a);
    let token_b_client = TestTokenClient::new(&env, &token_b);
    token_a_client.mint(&depositor, &1i128);
    token_b_client.mint(&depositor, &1i128);

    let mut deposits = Vec::new(&env);
    deposits.push_back((token_a.clone(), 1i128));
    deposits.push_back((token_b.clone(), 1i128));

    let result = client.try_batch_deposit(&depositor, &deposits);
    assert_eq!(result, Ok(Ok(())));
    assert_eq!(client.get_balance(&depositor, &token_a), 1i128);
    assert_eq!(client.get_balance(&depositor, &token_b), 1i128);
}
