/// Tests for `get_pending_metrics` per-token breakdown (issue #585).
///
/// `get_pending_metrics` now returns `Vec<(MaybeAddress, u64, i128)>` where each
/// entry groups all pending settlements that share the same token address.
/// Settlements proposed via `propose_settlement` (no token) land in the
/// `MaybeAddress::None` bucket; those proposed via `propose_settlement_with_token`
/// land in a `MaybeAddress::Some(_)` bucket keyed by the token address.
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use treasury::{MaybeAddress, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env) -> (TreasuryContractClient<'_>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &Vec::new(env));
    (client, admin)
}

// ── No pending settlements ───────────────────────────────────────────────

#[test]
fn empty_treasury_returns_empty_metrics() {
    let env = Env::default();
    let (client, _) = setup(&env);
    let metrics = client.get_pending_metrics();
    assert_eq!(metrics.len(), 0);
}

// ── Single token bucket ──────────────────────────────────────────────────

#[test]
fn single_token_returns_one_bucket() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    client.propose_settlement_with_token(
        &admin,
        &merchant,
        &5_000_000,
        &MaybeAddress::Some(token.clone()),
    );
    client.propose_settlement_with_token(
        &admin,
        &merchant,
        &3_000_000,
        &MaybeAddress::Some(token.clone()),
    );

    let metrics = client.get_pending_metrics();
    assert_eq!(metrics.len(), 1);

    let (tok, cnt, total) = metrics.get(0).unwrap();
    assert_eq!(tok, MaybeAddress::Some(token));
    assert_eq!(cnt, 2u64);
    assert_eq!(total, 8_000_000i128);
}

// ── Multi-token: two distinct tokens each with their own bucket ──────────

#[test]
fn multi_token_returns_separate_buckets() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    // Two settlements for token_a
    client.propose_settlement_with_token(
        &admin,
        &merchant,
        &1_000_000,
        &MaybeAddress::Some(token_a.clone()),
    );
    client.propose_settlement_with_token(
        &admin,
        &merchant,
        &2_000_000,
        &MaybeAddress::Some(token_a.clone()),
    );
    // One settlement for token_b
    client.propose_settlement_with_token(
        &admin,
        &merchant,
        &4_000_000,
        &MaybeAddress::Some(token_b.clone()),
    );

    let metrics = client.get_pending_metrics();
    assert_eq!(metrics.len(), 2);

    // Find the bucket for each token (order is insertion order).
    let mut found_a = false;
    let mut found_b = false;
    for i in 0..metrics.len() {
        let (tok, cnt, total) = metrics.get(i).unwrap();
        if tok == MaybeAddress::Some(token_a.clone()) {
            assert_eq!(cnt, 2u64);
            assert_eq!(total, 3_000_000i128);
            found_a = true;
        } else if tok == MaybeAddress::Some(token_b.clone()) {
            assert_eq!(cnt, 1u64);
            assert_eq!(total, 4_000_000i128);
            found_b = true;
        }
    }
    assert!(found_a, "token_a bucket missing");
    assert!(found_b, "token_b bucket missing");
}

// ── No-token settlements land in MaybeAddress::None bucket ───────────────

#[test]
fn no_token_settlements_land_in_none_bucket() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);

    // Use legacy propose_settlement (no token).
    client.propose_settlement(&admin, &merchant, &7_000_000);
    client.propose_settlement(&admin, &merchant, &3_000_000);

    let metrics = client.get_pending_metrics();
    assert_eq!(metrics.len(), 1);

    let (tok, cnt, total) = metrics.get(0).unwrap();
    assert_eq!(tok, MaybeAddress::None);
    assert_eq!(cnt, 2u64);
    assert_eq!(total, 10_000_000i128);
}

// ── Mixed: token and no-token settlements coexist in separate buckets ────

#[test]
fn mixed_token_and_no_token_returns_two_buckets() {
    let env = Env::default();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    // One settlement without token and one with.
    client.propose_settlement(&admin, &merchant, &1_000_000);
    client.propose_settlement_with_token(
        &admin,
        &merchant,
        &2_000_000,
        &MaybeAddress::Some(token.clone()),
    );

    let metrics = client.get_pending_metrics();
    assert_eq!(metrics.len(), 2);

    let mut found_none = false;
    let mut found_some = false;
    for i in 0..metrics.len() {
        let (tok, cnt, total) = metrics.get(i).unwrap();
        if tok == MaybeAddress::None {
            assert_eq!(cnt, 1u64);
            assert_eq!(total, 1_000_000i128);
            found_none = true;
        } else if tok == MaybeAddress::Some(token.clone()) {
            assert_eq!(cnt, 1u64);
            assert_eq!(total, 2_000_000i128);
            found_some = true;
        }
    }
    assert!(found_none, "None bucket missing");
    assert!(found_some, "Some(token) bucket missing");
}

// ── Executed settlements are excluded from metrics ───────────────────────

#[test]
fn executed_settlements_excluded_from_metrics() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    // Threshold=1 so proposer alone can execute.
    client.initialize(&admin, &1, &Vec::new(&env));

    let merchant = Address::generate(&env);
    let token = env.register_stellar_asset_contract(admin.clone());
    soroban_sdk::token::StellarAssetClient::new(&env, &token).mint(&contract_id, &10_000_000);

    // Propose with a specific token and then execute it.
    let sid = client.propose_settlement_with_token(
        &admin,
        &merchant,
        &5_000_000,
        &MaybeAddress::Some(token.clone()),
    );
    client.execute_settlement(&admin, &sid, &token);

    // Propose a second, still-pending settlement for the same token.
    client.propose_settlement_with_token(
        &admin,
        &merchant,
        &2_000_000,
        &MaybeAddress::Some(token.clone()),
    );

    let metrics = client.get_pending_metrics();
    assert_eq!(metrics.len(), 1);
    let (_, cnt, total) = metrics.get(0).unwrap();
    assert_eq!(cnt, 1u64);
    assert_eq!(total, 2_000_000i128);
}
