//! Exact-boundary coverage for `AddressIndex`'s `MAX_TRACKED_ADDRESSES` cap (issue #48).
//!
//! `ComplianceContract::track_address` (see `contracts/compliance/src/lib.rs`) only
//! grows `DataKey::AddressIndex` when the address being tracked is *new* to the
//! index. It rejects growth with `ContractError::AddressIndexFull` once
//! `index.len() >= MAX_TRACKED_ADDRESSES`, but never rejects an operation on an
//! address that is already present in the index — that address's entry is a
//! no-op with respect to `track_address`, regardless of how full the index is.
//!
//! These tests exercise that boundary precisely, rather than relying on tests
//! that happen to use a handful of addresses well below the cap:
//!
//! 1. Filling the index to exactly `MAX_TRACKED_ADDRESSES` distinct addresses
//!    must succeed for every single one (no off-by-one rejecting the last slot).
//! 2. The next *new*, distinct address beyond the cap must be rejected with
//!    `ContractError::AddressIndexFull`, not silently accepted or rejected with
//!    some other error.
//! 3. Operations on addresses that are already tracked — re-blocking an
//!    already-blocked address, re-allowing an already-allowed address, and
//!    updating an already-tracked address's expiry — must continue to succeed
//!    cleanly while the index is completely full, since none of them need to
//!    grow the index.
//!
//! `MAX_TRACKED_ADDRESSES` is re-exported by the `compliance` crate, so this
//! suite asserts against the real value rather than a hand-mirrored copy.

use compliance::{
    ComplianceContract, ComplianceContractClient, ContractError, BULK_OP_COOLDOWN_SECS,
    MAX_BATCH_SIZE, MAX_TRACKED_ADDRESSES,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn setup() -> (Env, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    env.cost_estimate().budget().reset_unlimited();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

/// Fills the address index to exactly `MAX_TRACKED_ADDRESSES` distinct, newly
/// generated addresses. Uses `bulk_allow_addresses` (batched, `MAX_BATCH_SIZE`
/// per call) rather than `MAX_TRACKED_ADDRESSES` individual `allow_address`
/// calls so the fill stays fast in the test host; every batch is asserted to
/// succeed, so the cap still must not reject any entry up to and including the
/// boundary one.
fn fill_index_to_cap(
    env: &Env,
    admin: &Address,
    client: &ComplianceContractClient<'static>,
) -> Vec<Address> {
    let mut addresses = Vec::with_capacity(MAX_TRACKED_ADDRESSES as usize);
    let mut remaining = MAX_TRACKED_ADDRESSES;
    while remaining > 0 {
        let batch_size = remaining.min(MAX_BATCH_SIZE);
        let mut batch = soroban_sdk::Vec::new(env);
        for _ in 0..batch_size {
            let address = Address::generate(env);
            batch.push_back(address.clone());
            addresses.push(address);
        }
        client.bulk_allow_addresses(admin, &batch);
        remaining -= batch_size;
        // bulk_allow_addresses enforces BULK_OP_COOLDOWN_SECS between calls by
        // the same admin (#454); step the ledger clock past it before the next
        // batch.
        if remaining > 0 {
            env.ledger()
                .set_timestamp(env.ledger().timestamp() + BULK_OP_COOLDOWN_SECS + 1);
        }
    }
    addresses
}

#[test]
fn index_accepts_up_to_exactly_max_tracked_addresses() {
    let (env, admin, client) = setup();
    let addresses = fill_index_to_cap(&env, &admin, &client);
    assert_eq!(addresses.len() as u32, MAX_TRACKED_ADDRESSES);

    // Spot-check the very first and very last address tracked — both must be
    // present and allowed, confirming no entry was silently dropped anywhere
    // along the way to the cap.
    assert!(client.is_allowed(&addresses[0]));
    assert!(client.is_allowed(&addresses[(MAX_TRACKED_ADDRESSES - 1) as usize]));
}

#[test]
fn new_distinct_address_beyond_cap_is_rejected_with_address_index_full() {
    let (env, admin, client) = setup();
    fill_index_to_cap(&env, &admin, &client);

    // The index is now completely full. A brand-new, never-before-seen address
    // must be rejected specifically with `AddressIndexFull` — not accepted, and
    // not rejected with a different, less specific error.
    let overflow_address = Address::generate(&env);
    let result = client.try_allow_address(&admin, &overflow_address);
    assert_eq!(
        result,
        Err(Ok(ContractError::AddressIndexFull)),
        "the (MAX_TRACKED_ADDRESSES + 1)th distinct address must be rejected"
    );

    // The rejected address must not have been tracked as a side effect of the
    // failed call — it should read as not-allowed.
    assert!(!client.is_allowed(&overflow_address));

    // The same rejection must apply across every entrypoint that calls
    // `track_address` with a new address, not just `allow_address`.
    let overflow_address_2 = Address::generate(&env);
    let block_result = client.try_block_address(&admin, &overflow_address_2, &None);
    assert_eq!(block_result, Err(Ok(ContractError::AddressIndexFull)));

    let overflow_address_3 = Address::generate(&env);
    let allow_until_result =
        client.try_allow_address_until(&admin, &overflow_address_3, &1_000_000);
    assert_eq!(allow_until_result, Err(Ok(ContractError::AddressIndexFull)));
}

#[test]
fn already_tracked_address_operations_succeed_while_index_is_full() {
    let (env, admin, client) = setup();
    let addresses = fill_index_to_cap(&env, &admin, &client);

    // Pick a handful of already-tracked addresses spread across the fill order
    // (first, middle, last) to re-exercise, rather than only the most recently
    // added one.
    let first = addresses[0].clone();
    let middle = addresses[(MAX_TRACKED_ADDRESSES / 2) as usize].clone();
    let last = addresses[(MAX_TRACKED_ADDRESSES - 1) as usize].clone();

    // Re-allowing an already-allowed (and already-tracked) address must
    // succeed even though the index is completely full — it does not need to
    // grow the index, since the address is already present in it.
    client.allow_address(&admin, &first);
    assert!(client.is_allowed(&first));

    // Blocking an already-tracked address must succeed — `block_address` calls
    // `track_address`, but since `middle` is already in the index this is a
    // no-op with respect to growth.
    client.block_address(&admin, &middle, &None);
    assert!(client.is_blocked(&middle));
    assert!(!client.is_allowed(&middle));

    // Re-blocking that same, already-blocked address again must also succeed
    // cleanly — it's still a no-op for the index.
    client.block_address(&admin, &middle, &None);
    assert!(client.is_blocked(&middle));

    // Updating an existing, already-tracked entry's expiry must succeed while
    // the index is full.
    client.allow_address_until(&admin, &last, &2_000_000);
    assert!(client.is_allowed(&last));

    // And re-allowing that same address with a fresh expiry afterward must
    // continue to succeed — still no growth required.
    client.allow_address_until(&admin, &last, &3_000_000);
    assert!(client.is_allowed(&last));

    // Sanity: the index is still exactly at the cap — none of the above
    // already-tracked operations grew it.
    let overflow_address = Address::generate(&env);
    let result = client.try_allow_address(&admin, &overflow_address);
    assert_eq!(result, Err(Ok(ContractError::AddressIndexFull)));
}

#[test]
fn clear_address_on_already_tracked_address_succeeds_while_index_is_full() {
    let (env, admin, client) = setup();
    let addresses = fill_index_to_cap(&env, &admin, &client);
    let target = addresses[0].clone();

    // `clear_address` operates on an already-tracked address and does not call
    // `track_address` at all, so it must be entirely unaffected by a full index.
    client.block_address(&admin, &target, &None);
    assert!(client.is_blocked(&target));
    client.clear_address(&admin, &target);
    // clear_address unblocks *and* re-allows (Blocked -> false, Allowed -> true).
    assert!(!client.is_blocked(&target));
    assert!(client.is_allowed(&target));
}
