//! Tests for merchant payout address change history tracking (#579).
//!
//! Covers:
//! - History is empty before any update
//! - Each `update_merchant_payout_address` call appends an entry with the correct address
//! - Timestamps are recorded and non-decreasing
//! - The history is capped at MAX_PAYOUT_HISTORY (10) entries; older entries are dropped FIFO
//! - Different merchants have independent history lists

use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

fn setup(env: &Env) -> (TreasuryContractClient<'_>, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

/// A freshly-initialised contract returns an empty history for any merchant.
#[test]
fn history_is_empty_before_any_update() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);
    let merchant = Address::generate(&env);

    let history = client.get_payout_address_history(&merchant);
    assert_eq!(history.len(), 0);
}

/// After a single update the history has exactly one entry containing the new address.
#[test]
fn single_update_produces_one_history_entry() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let merchant = Address::generate(&env);
    let payout1 = Address::generate(&env);

    client.update_merchant_payout_address(&merchant, &payout1);

    let history = client.get_payout_address_history(&merchant);
    assert_eq!(history.len(), 1);
    let (addr, _ts) = history.get(0).unwrap();
    assert_eq!(addr, payout1);
}

/// Multiple updates append in order; the most recent entry is the last element.
#[test]
fn multiple_updates_append_in_order() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let merchant = Address::generate(&env);
    let payout1 = Address::generate(&env);
    let payout2 = Address::generate(&env);
    let payout3 = Address::generate(&env);

    client.update_merchant_payout_address(&merchant, &payout1);
    client.update_merchant_payout_address(&merchant, &payout2);
    client.update_merchant_payout_address(&merchant, &payout3);

    let history = client.get_payout_address_history(&merchant);
    assert_eq!(history.len(), 3);

    let (addr0, _) = history.get(0).unwrap();
    let (addr1, _) = history.get(1).unwrap();
    let (addr2, _) = history.get(2).unwrap();

    assert_eq!(addr0, payout1);
    assert_eq!(addr1, payout2);
    assert_eq!(addr2, payout3);
}

/// Once the MAX_PAYOUT_HISTORY cap (10) is reached, the oldest entry is dropped
/// so the list never grows beyond 10 entries.
#[test]
fn history_capped_at_max_entries_drops_oldest() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let merchant = Address::generate(&env);

    // Generate 11 distinct payout addresses and apply them in order.
    let mut addrs: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
    for _ in 0..11 {
        addrs.push_back(Address::generate(&env));
    }
    for i in 0..11 {
        client.update_merchant_payout_address(&merchant, &addrs.get(i).unwrap());
    }

    let history = client.get_payout_address_history(&merchant);
    // Must not exceed 10 entries.
    assert_eq!(history.len(), 10);

    // The first entry in the history should be the *second* address we set (index 1),
    // because the first address (index 0) was the one that got dropped.
    let (oldest_retained, _) = history.get(0).unwrap();
    assert_eq!(oldest_retained, addrs.get(1).unwrap());

    // The last entry should be the most recently set address (index 10).
    let (newest, _) = history.get(9).unwrap();
    assert_eq!(newest, addrs.get(10).unwrap());
}

/// Each merchant has an independent payout history; updating one does not affect another.
#[test]
fn different_merchants_have_independent_histories() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let merchant_a = Address::generate(&env);
    let merchant_b = Address::generate(&env);
    let payout_a = Address::generate(&env);
    let payout_b1 = Address::generate(&env);
    let payout_b2 = Address::generate(&env);

    client.update_merchant_payout_address(&merchant_a, &payout_a);
    client.update_merchant_payout_address(&merchant_b, &payout_b1);
    client.update_merchant_payout_address(&merchant_b, &payout_b2);

    let history_a = client.get_payout_address_history(&merchant_a);
    let history_b = client.get_payout_address_history(&merchant_b);

    assert_eq!(history_a.len(), 1);
    assert_eq!(history_b.len(), 2);

    let (addr_a, _) = history_a.get(0).unwrap();
    assert_eq!(addr_a, payout_a);
}

/// The timestamp recorded in the history entry matches the ledger timestamp at
/// which `update_merchant_payout_address` was called.
#[test]
fn history_entry_records_correct_timestamp() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, _admin) = setup(&env);

    let merchant = Address::generate(&env);
    let payout = Address::generate(&env);

    let ts_before = env.ledger().timestamp();
    client.update_merchant_payout_address(&merchant, &payout);
    let ts_after = env.ledger().timestamp();

    let history = client.get_payout_address_history(&merchant);
    assert_eq!(history.len(), 1);
    let (_, ts) = history.get(0).unwrap();
    assert!(ts >= ts_before && ts <= ts_after);
}
