// #48 added a MAX_TRACKED_ADDRESSES cap on compliance's AddressIndex; #47 later
// paginated the read side of that same index via export_snapshot_page. This
// suite exists to confirm, rather than assume, that the pagination logic
// behaves correctly once the index is genuinely as large as the cap allows
// (compliance::MAX_TRACKED_ADDRESSES) and that adversarial start/limit
// combinations against that maximally-full index return cleanly rather than
// panicking, reading out of bounds, or burning instructions disproportionate
// to the requested page size.
//
// This file also covers the write-between-reads consistency guarantee (#605):
// because the AddressIndex is append-only, a page already fetched is stable —
// no entry can be inserted into a page whose slot range has already been
// written. Addresses added after a page has been read appear at the end of
// the index and will be present in subsequent page fetches that span their
// insertion position. The consequence for exporters: a snapshot taken in
// multiple page calls is consistent for all entries up to the count observed
// on the first page call, and any entry added during the export appears in
// the later pages that cover its index slot.

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

// ── Write-between-reads consistency tests (#605) ─────────────────────────────
//
// The AddressIndex is append-only: each `allow_address` / `bulk_allow_addresses`
// call appends to the end and never modifies an existing slot. This means:
//
//   1. A page that has been fetched is permanently stable; no subsequent write
//      can change the content of an already-returned page.
//   2. Addresses added while an export is in progress appear at the tail of the
//      index. If a second page fetch starts at a position beyond the new entry,
//      the entry is visible; if the second fetch starts before it, the entry
//      will be on a later page that the caller has not yet fetched.
//   3. No address is ever skipped or duplicated due to concurrent writes: the
//      only effect of a mid-export write is that the exporter may see more
//      entries than existed at the start, never fewer.

/// Addresses added *after* the first page has been read do not appear in
/// that first page on re-read (pages are immutable once written), but they
/// DO appear in a subsequent page fetch that covers their index slot.
#[test]
fn address_added_after_first_page_read_appears_in_later_page() {
    let (env, admin, client) = setup();

    // Seed two addresses so we have something on page 0.
    let addr_a = Address::generate(&env);
    let addr_b = Address::generate(&env);
    client.allow_address(&admin, &addr_a);
    client.allow_address(&admin, &addr_b);

    // Read page 0 (start=0, limit=2) — captures only addr_a and addr_b.
    let page0_before = client.export_snapshot_page(&admin, &0, &2);
    assert_eq!(page0_before.len(), 2);

    // Now add a third address — simulating a write between page reads.
    let addr_c = Address::generate(&env);
    client.allow_address(&admin, &addr_c);

    // Re-reading page 0 with the same parameters must return the same two
    // entries; addr_c must NOT appear here because it was appended after them.
    let page0_after = client.export_snapshot_page(&admin, &0, &2);
    assert_eq!(page0_after.len(), 2);
    assert_eq!(page0_after.get(0).unwrap().0, addr_a);
    assert_eq!(page0_after.get(1).unwrap().0, addr_b);

    // Fetching the next page (start=2, limit=10) must include addr_c.
    let page1 = client.export_snapshot_page(&admin, &2, &10);
    assert_eq!(page1.len(), 1);
    assert_eq!(page1.get(0).unwrap().0, addr_c);
}

/// When multiple addresses are added between page reads, all of them appear
/// in the later pages that cover their index slots — none are dropped.
#[test]
fn multiple_addresses_added_between_page_reads_all_visible_in_later_pages() {
    let (env, admin, client) = setup();

    // Fill exactly one page worth of addresses (ADDR_INDEX_PAGE_SIZE = 25, but
    // we use a small number here to keep the test fast and independent of the
    // internal page size constant, which is not pub).
    let page_size: u64 = 5;
    let mut first_batch: std::vec::Vec<Address> = std::vec::Vec::new();
    for _ in 0..page_size {
        let addr = Address::generate(&env);
        client.allow_address(&admin, &addr);
        first_batch.push(addr);
    }

    // Read the first page — establishes a "snapshot" of the first 5 entries.
    let page0 = client.export_snapshot_page(&admin, &0, &page_size);
    assert_eq!(page0.len() as u64, page_size);
    for (i, (addr, _state)) in page0.iter().enumerate() {
        assert_eq!(addr, first_batch[i]);
    }

    // Simulate writes between page reads: add 3 more addresses.
    let mut second_batch: std::vec::Vec<Address> = std::vec::Vec::new();
    for _ in 0..3u32 {
        let addr = Address::generate(&env);
        client.allow_address(&admin, &addr);
        second_batch.push(addr);
    }

    // Fetching the next page must see all 3 newly added addresses.
    let page1 = client.export_snapshot_page(&admin, &page_size, &10);
    assert_eq!(page1.len(), 3);
    for (i, (addr, _state)) in page1.iter().enumerate() {
        assert_eq!(addr, second_batch[i]);
    }

    // The first page remains unchanged after the writes.
    let page0_reread = client.export_snapshot_page(&admin, &0, &page_size);
    assert_eq!(page0_reread.len() as u64, page_size);
    for (i, (addr, _state)) in page0_reread.iter().enumerate() {
        assert_eq!(addr, first_batch[i]);
    }
}

/// A full sequential scan started before a mid-export write covers all
/// addresses that existed at any point during the scan: entries present at
/// the start are on earlier pages, entries added mid-scan are on later pages,
/// and no entry is duplicated or skipped.
#[test]
fn sequential_scan_with_mid_export_write_produces_no_duplicates_and_no_gaps() {
    let (env, admin, client) = setup();

    // Seed an initial set.
    let initial_count: u64 = 4;
    let mut all_expected: std::vec::Vec<Address> = std::vec::Vec::new();
    for _ in 0..initial_count {
        let addr = Address::generate(&env);
        client.allow_address(&admin, &addr);
        all_expected.push(addr);
    }

    // Read the first page (limit=2).
    let limit: u64 = 2;
    let page0 = client.export_snapshot_page(&admin, &0, &limit);
    assert_eq!(page0.len() as u64, limit);

    // Add a new address mid-scan.
    let mid_addr = Address::generate(&env);
    client.allow_address(&admin, &mid_addr);
    all_expected.push(mid_addr);

    // Collect the rest of the index page by page until we get an empty page.
    let mut collected: std::vec::Vec<Address> = page0.iter().map(|(a, _)| a).collect();
    let mut start: u64 = limit;
    loop {
        let page = client.export_snapshot_page(&admin, &start, &limit);
        if page.len() == 0 {
            break;
        }
        for (addr, _state) in page.iter() {
            collected.push(addr);
        }
        start += limit;
    }

    // Every address in all_expected must appear exactly once.
    for expected_addr in &all_expected {
        let count = collected.iter().filter(|a| *a == expected_addr).count();
        assert_eq!(
            count, 1,
            "address {:?} appeared {count} times; expected exactly once",
            expected_addr
        );
    }
    // No unexpected extras.
    assert_eq!(
        collected.len(),
        all_expected.len(),
        "collected {} entries but expected {}",
        collected.len(),
        all_expected.len()
    );
}
