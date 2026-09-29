//! Property-based tests for dispute split rounding correctness.
//!
//! `resolve_dispute_split` divides a disputed amount between claimant and counterparty
//! using integer arithmetic (basis points out of 10,000). Integer division can leave a
//! remainder. This test verifies that:
//!
//! 1. The sum of payouts always equals the original dispute amount exactly (no funds lost or created)
//! 2. Edge cases like odd amounts and extreme ratios are handled correctly
//! 3. The rounding behavior is deterministic and never changes between runs
//!
//! The implementation uses integer subtraction to assign the remainder to counterparty,
//! ensuring the invariant: claimant_amount + counterparty_amount == dispute.amount

use proptest::prelude::*;
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

/// Minimal SEP-41–shaped test token for dispute split testing
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

const BPS_DENOMINATOR: u32 = 10_000;

fn setup(env: &Env) -> (TreasuryContractClient, Address, TestTokenClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let treasury_id = env.register(TreasuryContract, ());
    let client = TreasuryContractClient::new(env, &treasury_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));

    let token_id = env.register(TestToken, ());
    let token_client = TestTokenClient::new(env, &token_id);

    // Pre-fund the treasury with enough balance for any test
    token_client.mint(&treasury_id, &(i128::MAX / 2));

    (client, admin, token_client, token_id)
}

/// Test the core invariant: claimant_amount + counterparty_amount == dispute.amount
/// Parameterized by dispute amount and claimant basis points (0-10,000)
#[test]
fn prop_split_payout_sum_equals_disputed_amount() {
    proptest!(|(
        amount in 1i128..=1_000_000_000i128,
        claimant_bps in 0u32..=BPS_DENOMINATOR
    )| {
        let env = Env::default();
        let (client, admin, token_client, token_id) = setup(&env);

        let claimant = Address::generate(&env);
        let counterparty = Address::generate(&env);

        // Create a settlement and dispute
        let sid = client.propose_settlement(&admin, &claimant, &amount);
        let did = client.raise_dispute(&claimant, &sid, &counterparty, &amount, &500);

        // Get initial balances
        let claimant_before = token_client.balance(&claimant);
        let counterparty_before = token_client.balance(&counterparty);

        // Resolve the dispute with a split
        client.resolve_dispute_split(&admin, &did, &claimant_bps, &token_id);

        // Get final balances
        let claimant_after = token_client.balance(&claimant);
        let counterparty_after = token_client.balance(&counterparty);

        // Calculate actual payouts
        let claimant_payout = claimant_after - claimant_before;
        let counterparty_payout = counterparty_after - counterparty_before;

        // Assert the invariant: sum of payouts equals dispute amount
        prop_assert_eq!(
            claimant_payout + counterparty_payout,
            amount,
            "Sum of payouts must equal dispute amount. \
             amount={}, claimant_bps={}, \
             claimant_payout={}, counterparty_payout={}",
            amount, claimant_bps, claimant_payout, counterparty_payout
        );

        // Both amounts should be non-negative
        prop_assert!(claimant_payout >= 0, "claimant_payout must be non-negative");
        prop_assert!(counterparty_payout >= 0, "counterparty_payout must be non-negative");

        // Claimant payout should be proportional to claimant_bps
        let expected_claimant = amount
            .checked_mul(claimant_bps as i128)
            .unwrap() / BPS_DENOMINATOR as i128;
        prop_assert_eq!(
            claimant_payout,
            expected_claimant,
            "claimant_payout must match expected calculation"
        );
    });
}

/// Test edge case: 0% split (all to counterparty)
#[test]
fn split_zero_percent_claimant() {
    let env = Env::default();
    let (client, admin, token_client, token_id) = setup(&env);

    let claimant = Address::generate(&env);
    let counterparty = Address::generate(&env);
    let amount = 123_456i128;

    let sid = client.propose_settlement(&admin, &claimant, &amount);
    let did = client.raise_dispute(&claimant, &sid, &counterparty, &amount, &500);

    let claimant_before = token_client.balance(&claimant);
    let counterparty_before = token_client.balance(&counterparty);

    client.resolve_dispute_split(&admin, &did, &0, &token_id);

    let claimant_after = token_client.balance(&claimant);
    let counterparty_after = token_client.balance(&counterparty);

    assert_eq!(claimant_after - claimant_before, 0);
    assert_eq!(counterparty_after - counterparty_before, amount);
}

/// Test edge case: 100% split (all to claimant)
#[test]
fn split_hundred_percent_claimant() {
    let env = Env::default();
    let (client, admin, token_client, token_id) = setup(&env);

    let claimant = Address::generate(&env);
    let counterparty = Address::generate(&env);
    let amount = 987_654i128;

    let sid = client.propose_settlement(&admin, &claimant, &amount);
    let did = client.raise_dispute(&claimant, &sid, &counterparty, &amount, &500);

    let claimant_before = token_client.balance(&claimant);
    let counterparty_before = token_client.balance(&counterparty);

    client.resolve_dispute_split(&admin, &did, &BPS_DENOMINATOR, &token_id);

    let claimant_after = token_client.balance(&claimant);
    let counterparty_after = token_client.balance(&counterparty);

    assert_eq!(claimant_after - claimant_before, amount);
    assert_eq!(counterparty_after - counterparty_before, 0);
}

/// Test edge case: 50% split (exactly even)
#[test]
fn split_fifty_percent_claimant() {
    let env = Env::default();
    let (client, admin, token_client, token_id) = setup(&env);

    let claimant = Address::generate(&env);
    let counterparty = Address::generate(&env);
    let amount = 1_000_000i128;

    let sid = client.propose_settlement(&admin, &claimant, &amount);
    let did = client.raise_dispute(&claimant, &sid, &counterparty, &amount, &500);

    let claimant_before = token_client.balance(&claimant);
    let counterparty_before = token_client.balance(&counterparty);

    client.resolve_dispute_split(&admin, &did, &5_000, &token_id);

    let claimant_after = token_client.balance(&claimant);
    let counterparty_after = token_client.balance(&counterparty);

    let claimant_payout = claimant_after - claimant_before;
    let counterparty_payout = counterparty_after - counterparty_before;

    assert_eq!(claimant_payout + counterparty_payout, amount);
    assert_eq!(claimant_payout, amount / 2);
    assert_eq!(counterparty_payout, amount / 2);
}

/// Test odd amounts that expose rounding issues more clearly.
/// Amounts that aren't evenly divisible by BPS_DENOMINATOR will have remainders.
#[test]
fn split_odd_amounts_various_ratios() {
    proptest!(|(
        amount in prop::sample::select(vec![1i128, 3, 7, 11, 13, 17, 19, 23, 29, 31, 
                       99, 101, 999, 1001, 9999, 10001, 99999, 100001]),
        claimant_bps in prop::sample::select(vec![0u32, 1, 99, 333, 1_000, 5_000, 9_999, 10_000])
    )| {
        let env = Env::default();
        let (client, admin, token_client, token_id) = setup(&env);

        let claimant = Address::generate(&env);
        let counterparty = Address::generate(&env);

        let sid = client.propose_settlement(&admin, &claimant, &amount);
        let did = client.raise_dispute(&claimant, &sid, &counterparty, &amount, &500);

        let claimant_before = token_client.balance(&claimant);
        let counterparty_before = token_client.balance(&counterparty);

        client.resolve_dispute_split(&admin, &did, &claimant_bps, &token_id);

        let claimant_after = token_client.balance(&claimant);
        let counterparty_after = token_client.balance(&counterparty);

        let claimant_payout = claimant_after - claimant_before;
        let counterparty_payout = counterparty_after - counterparty_before;

        // The invariant must hold for ALL combinations
        prop_assert_eq!(
            claimant_payout + counterparty_payout,
            amount,
            "Invariant violated: claimant_payout + counterparty_payout != amount. \
             amount={}, claimant_bps={}, claimant_payout={}, counterparty_payout={}",
            amount, claimant_bps, claimant_payout, counterparty_payout
        );
    });
}

/// Test extreme ratios that cause high rounding pressures
#[test]
fn split_extreme_ratios() {
    proptest!(|(
        amount in 1i128..=100_000i128,
        claimant_bps in prop::sample::select(vec![1u32, 2, 3, 9_997, 9_998, 9_999])
    )| {
        let env = Env::default();
        let (client, admin, token_client, token_id) = setup(&env);

        let claimant = Address::generate(&env);
        let counterparty = Address::generate(&env);

        let sid = client.propose_settlement(&admin, &claimant, &amount);
        let did = client.raise_dispute(&claimant, &sid, &counterparty, &amount, &500);

        let claimant_before = token_client.balance(&claimant);
        let counterparty_before = token_client.balance(&counterparty);

        client.resolve_dispute_split(&admin, &did, &claimant_bps, &token_id);

        let claimant_after = token_client.balance(&claimant);
        let counterparty_after = token_client.balance(&counterparty);

        let claimant_payout = claimant_after - claimant_before;
        let counterparty_payout = counterparty_after - counterparty_before;

        prop_assert_eq!(
            claimant_payout + counterparty_payout,
            amount,
            "Extreme ratio test failed: amount={}, claimant_bps={}, \
             claimant_payout={}, counterparty_payout={}",
            amount, claimant_bps, claimant_payout, counterparty_payout
        );
    });
}

/// Test that the remainder always goes to counterparty (verifies implementation detail)
/// If claimant_amount is truncated by integer division, counterparty gets the difference
#[test]
fn split_remainder_assigned_to_counterparty() {
    proptest!(|(
        amount in 1i128..=100_000i128,
        claimant_bps in 0u32..=BPS_DENOMINATOR
    )| {
        let env = Env::default();
        let (client, admin, token_client, token_id) = setup(&env);

        let claimant = Address::generate(&env);
        let counterparty = Address::generate(&env);

        let sid = client.propose_settlement(&admin, &claimant, &amount);
        let did = client.raise_dispute(&claimant, &sid, &counterparty, &amount, &500);

        let claimant_before = token_client.balance(&claimant);
        let counterparty_before = token_client.balance(&counterparty);

        client.resolve_dispute_split(&admin, &did, &claimant_bps, &token_id);

        let claimant_after = token_client.balance(&claimant);
        let counterparty_after = token_client.balance(&counterparty);

        let claimant_payout = claimant_after - claimant_before;
        let counterparty_payout = counterparty_after - counterparty_before;

        // Calculate what claimant should get with truncation
        let expected_claimant = amount
            .checked_mul(claimant_bps as i128)
            .unwrap() / BPS_DENOMINATOR as i128;

        prop_assert_eq!(claimant_payout, expected_claimant);

        // Counterparty should get exactly the remainder
        let expected_counterparty = amount - expected_claimant;
        prop_assert_eq!(counterparty_payout, expected_counterparty);
    });
}

/// Test that the same amount and ratio always produce the same result (determinism)
#[test]
fn split_deterministic_results() {
    let amount = 123_456i128;
    let claimant_bps = 3_333u32;

    let mut results = std::vec::Vec::new();

    for _ in 0..5 {
        let env_inner = Env::default();
        let (client_inner, admin_inner, token_client_inner, token_id) = setup(&env_inner);

        let claimant = Address::generate(&env_inner);
        let counterparty = Address::generate(&env_inner);

        let sid = client_inner.propose_settlement(&admin_inner, &claimant, &amount);
        let did = client_inner.raise_dispute(&claimant, &sid, &counterparty, &amount, &500);

        let claimant_before = token_client_inner.balance(&claimant);
        let counterparty_before = token_client_inner.balance(&counterparty);

        client_inner.resolve_dispute_split(&admin_inner, &did, &claimant_bps, &token_id);

        let claimant_after = token_client_inner.balance(&claimant);
        let counterparty_after = token_client_inner.balance(&counterparty);

        let claimant_payout = claimant_after - claimant_before;
        let counterparty_payout = counterparty_after - counterparty_before;

        results.push((claimant_payout, counterparty_payout));
    }

    // All results should be identical
    for i in 1..results.len() {
        assert_eq!(
            results[0], results[i],
            "Results must be deterministic. Run 0: {:?}, Run {}: {:?}",
            results[0], i, results[i]
        );
    }
}
