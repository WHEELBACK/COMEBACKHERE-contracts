// #48 added a MAX_TRACKED_ADDRESSES cap on compliance's AddressIndex; #47 later
// paginated the read side of that same index via export_snapshot_page. This
// suite exists to confirm, rather than assume, that the pagination logic
// behaves correctly once the index is genuinely as large as the cap allows
// (compliance::MAX_TRACKED_ADDRESSES) and that adversarial start/limit
// combinations against that maximally-full index return cleanly rather than
// panicking, reading out of bounds, or burning instructions disproportionate
// to the requested page size.

use compliance::{
    AddressState, ComplianceContract, ComplianceContractClient, ContractError,
    BULK_OP_COOLDOWN_SECS, MAX_BATCH_SIZE, MAX_TRACKED_ADDRESSES,
};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

extern crate std;

const CAP: u32 = MAX_TRACKED_ADDRESSES;

/// A read-only page scan over an already-maximally-full index should not
/// cost meaningfully more than scanning the page itself; this is a generous
/// ceiling (not a tight one), chosen the same way as PAGE_INSTRUCTION_BUDGET
/// in contracts/treasury/tests/settlement_pagination_test.rs.
const SMALL_PAGE_INSTRUCTION_BUDGET: u64 = 200_000_000;

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

/// Fills the AddressIndex to exactly `CAP` distinct tracked addresses using
/// batches of MAX_BATCH_SIZE, and returns them in insertion order -- the same
/// order export_snapshot_page must respect.
fn fill_index_to_cap(
    env: &Env,
    admin: &Address,
    client: &ComplianceContractClient,
) -> std::vec::Vec<Address> {
    let mut all = std::vec::Vec::with_capacity(CAP as usize);
    let mut remaining = CAP;
    while remaining > 0 {
        let batch_size = remaining.min(MAX_BATCH_SIZE);
        let mut batch = soroban_sdk::Vec::new(env);
        for _ in 0..batch_size {
            let addr = Address::generate(env);
            batch.push_back(addr.clone());
            all.push(addr);
        }
        client.bulk_allow_addresses(admin, &batch);
        remaining -= batch_size;
        // bulk_allow_addresses enforces BULK_OP_COOLDOWN_SECS between calls by
        // the same admin (#454); step the ledger clock past it before the next
        // batch so filling the index doesn't trip the cooldown.
        if remaining > 0 {
            env.ledger()
                .set_timestamp(env.ledger().timestamp() + BULK_OP_COOLDOWN_SECS + 1);
        }
    }
    all
}

#[test]
fn index_filled_to_cap_rejects_further_growth() {
    let (env, admin, client) = setup();
    let tracked = fill_index_to_cap(&env, &admin, &client);
    assert_eq!(tracked.len() as u32, CAP);

    // Sanity check that this suite is genuinely sitting at the #48 cap
    // boundary before exercising pagination against it: one more *new*
    // address must be rejected with AddressIndexFull.
    let one_more = Address::generate(&env);
    let result = client.try_allow_address(&admin, &one_more);
    assert_eq!(result, Err(Ok(ContractError::AddressIndexFull)));
}

#[test]
fn page_with_start_near_end_and_oversized_limit_returns_remaining_suffix_only() {
    let (env, admin, client) = setup();
    let tracked = fill_index_to_cap(&env, &admin, &client);

    let start = (CAP - 3) as u64;
    // limit is far larger than the 3 entries actually remaining after start.
    let page = client.export_snapshot_page(&admin, &start, &10_000);

    assert_eq!(page.len(), 3);
    for (i, (addr, state)) in page.iter().enumerate() {
        assert_eq!(addr, tracked[CAP as usize - 3 + i]);
        assert_eq!(state, AddressState::Allowed);
    }
}

#[test]
fn page_with_limit_zero_returns_empty_regardless_of_start() {
    let (env, admin, client) = setup();
    fill_index_to_cap(&env, &admin, &client);

    assert_eq!(client.export_snapshot_page(&admin, &0, &0).len(), 0);
    assert_eq!(
        client
            .export_snapshot_page(&admin, &((CAP - 1) as u64), &0)
            .len(),
        0
    );
}

#[test]
fn page_with_start_exactly_at_total_returns_empty() {
    let (env, admin, client) = setup();
    fill_index_to_cap(&env, &admin, &client);
    let page = client.export_snapshot_page(&admin, &(CAP as u64), &10);
    assert_eq!(page.len(), 0);
}

#[test]
fn page_with_start_beyond_total_returns_empty_not_panic() {
    let (env, admin, client) = setup();
    fill_index_to_cap(&env, &admin, &client);

    let page = client.export_snapshot_page(&admin, &(CAP as u64 + 12_345), &10);
    assert_eq!(page.len(), 0);

    // u64::MAX as `start` must not overflow or wrap the internal u32 cast
    // used when indexing into the AddressIndex vector.
    let page_max = client.export_snapshot_page(&admin, &u64::MAX, &10);
    assert_eq!(page_max.len(), 0);
}

#[test]
fn page_with_limit_u64_max_near_end_is_bounded_by_remaining_not_by_limit() {
    let (env, admin, client) = setup();
    let tracked = fill_index_to_cap(&env, &admin, &client);

    let start = (CAP - 25) as u64;
    env.cost_estimate().budget().reset_tracker();
    let page = client.export_snapshot_page(&admin, &start, &u64::MAX);
    let instructions = env.cost_estimate().budget().cpu_instruction_cost();

    // Only the 25 remaining entries should come back -- not an attempt to
    // materialize u64::MAX entries or to scan proportionally to `limit`.
    assert_eq!(page.len(), 25);
    for (i, (addr, _)) in page.iter().enumerate() {
        assert_eq!(addr, tracked[CAP as usize - 25 + i]);
    }
    assert!(
        instructions <= SMALL_PAGE_INSTRUCTION_BUDGET,
        "export_snapshot_page(near-end, u64::MAX) over a {CAP}-entry index used \
         {instructions} instructions returning only 25 results, expected <= {SMALL_PAGE_INSTRUCTION_BUDGET}"
    );
}

#[test]
fn full_index_page_matches_insertion_order() {
    let (env, admin, client) = setup();
    let tracked = fill_index_to_cap(&env, &admin, &client);

    let page = client.export_snapshot_page(&admin, &0, &(CAP as u64));
    assert_eq!(page.len() as u32, CAP);
    for (i, (addr, state)) in page.iter().enumerate() {
        assert_eq!(addr, tracked[i]);
        assert_eq!(state, AddressState::Allowed);
    }
}
