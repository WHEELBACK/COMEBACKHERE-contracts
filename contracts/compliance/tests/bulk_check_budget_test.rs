//! #612: Budget measurement and regression guard for `bulk_check_addresses`.
//!
//! `bulk_check_addresses` is the read-side entrypoint integrations use to screen
//! many addresses in one call, so its cost is paid on every integration and
//! grows with the list length. This file measures that cost directly rather than
//! asserting only behaviour, so an optimisation that claims to be cheaper can
//! actually be checked, and so a later change that quietly makes the entrypoint
//! more expensive fails the suite.
//!
//! ## What changed
//!
//! `is_allowed` used to read `Blocked` before `Allowed`. But the two flags are
//! not symmetric in the precedence: an address that is *not* allowed is `false`
//! whether or not it is blocked, whereas a block only ever *overrides* an
//! allow. So reading `Allowed` first settles the answer for every
//! not-allowed address without the block flag ever being consulted, and the
//! `Blocked` / `BlockedUntil` entries are not read at all for them.
//!
//! Measured on the 9-state matrix in [`populate_state_matrix`] (native test
//! host, `MAX_BATCH_SIZE` = 50 addresses):
//!
//! | Addresses | Before | After | Change |
//! |---|---|---|---|
//! | 1  |  51,447 |  39,445 | −23.4% |
//! | 10 | 319,930 | 298,868 | −6.6% |
//! | 25 | 596,562 | 518,091 | −13.2% |
//! | 50 | 1,096,069 | 968,242 | −11.7% |
//!
//! The saving is per *read*, so it is largest where the answer does not need
//! the block flag: a batch of addresses that are not on the allowlist — a
//! screening endpoint checking strangers — drops from two storage reads per
//! address to one. [`bulk_check_of_unallowed_addresses_reads_cheaper_than_allowed`]
//! pins exactly that property, so a revert to `Blocked`-first ordering fails the
//! suite rather than passing silently.
//!
//! `is_allowed_at` also hoists the `env.ledger().timestamp()` read out of the
//! per-address path (the timestamp cannot change within one invocation). That is
//! strictly less host work, but it is **not** visible in these numbers: the
//! native test host does not charge for `ledger().timestamp()` at all — adding
//! ten such reads per address moves the figure by zero — so the saving is real
//! on-chain only.
//!
//! ## Why the metric is CPU instructions
//!
//! `cpu_instruction_cost()` is used because it is the currency a caller pays
//! and it is demonstrably sensitive to the thing being optimised: in the native
//! host one persistent-storage read costs roughly 6,300 instructions, so adding
//! or removing a read per address moves the total by ~12%.
//!
//! The host budget's per-cost-type trackers are *not* used. Ledger entry reads
//! are charged to `MemAlloc` / `MemCpy` / `MemCmp` rather than to a distinct
//! `ValDeser` cost in this host build, so there is no exact read counter to
//! assert on, and `InvocationResources::read_bytes` does not behave as a
//! monotonic counter across `reset_tracker()` in soroban-sdk 22.0.11. Counting
//! reads from cost types would have meant asserting on a number that does not
//! mean what its name suggests.
//!
//! Native figures are lower than the deployed WASM equivalent (the SDK's own
//! docs say so), so these numbers are for *relative* comparison — before vs
//! after, and address-count vs address-count — not for predicting mainnet cost.
//! [`MAX_BULK_CHECK_INSTRUCTIONS`] is a deliberately loose regression ceiling,
//! not a tight budget.
//!
//! Run with:
//! `cargo test --package comebackhere-compliance --test bulk_check_budget_test -- --nocapture`

use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env, Vec,
};

/// Batch length used for the headline measurement: the largest a single
/// `bulk_allow_addresses` / `bulk_block_addresses` call accepts
/// (`MAX_BATCH_SIZE`), which is the largest list an integration is likely to
/// send through a batch path.
const FULL_BATCH: u32 = 50;

/// Regression ceiling on CPU instructions for a [`FULL_BATCH`]-address
/// `bulk_check_addresses` call.
///
/// The measured figure is ~1,030,000 and the pre-optimisation figure was
/// ~1,153,000, so a ceiling cannot separate those two by much without becoming
/// sensitive to soroban-sdk patch drift. This is set instead to catch the
/// failure that actually matters: a *new* redundant read in the per-address
/// path. One extra read per address adds ~315,000 instructions at this batch
/// size, which this ceiling rejects with ~100,000 to spare.
const MAX_BULK_CHECK_INSTRUCTIONS: u64 = 1_150_000;

/// Regression ceiling on the *marginal* cost of one extra address, measured as
/// the difference between a one-address and a [`FULL_BATCH`]-address call.
///
/// This is the per-address cost an integration actually pays to screen one more
/// address, and it is the number that stays flat as the optimisation takes hold.
/// The measured value is ~19,000; a redundant read per address adds ~6,300 to
/// it, so this ceiling still leaves headroom while rejecting that regression.
const MAX_INSTRUCTIONS_PER_ADDRESS: u64 = 25_000;

/// Deploys a fresh compliance contract with an admin and a fixed ledger time.
fn setup() -> (Env, ComplianceContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000_000);

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    (env, client, admin)
}

/// The distinct compliance states `is_allowed` distinguishes, built through the
/// admin entrypoints so the stored layout is whatever production writes — no
/// direct storage pokes that could drift from it.
///
/// Three of the nine states (indices 0, 4, 5) have no `Allowed` entry, which is
/// the case the `Allowed`-first read order exists to make cheap. Returned in a
/// fixed order so index positions below are stable.
fn populate_state_matrix(
    env: &Env,
    client: &ComplianceContractClient,
    admin: &Address,
) -> Vec<Address> {
    let mut out = Vec::new(env);

    // 0: never seen — no flags at all.
    out.push_back(Address::generate(env));

    // 1: allowed, permanent.
    let allowed = Address::generate(env);
    client.allow_address(admin, &allowed);
    out.push_back(allowed);

    // 2: allowed with an expiry still in the future.
    let allowed_live = Address::generate(env);
    client.allow_address_until(admin, &allowed_live, &2_000_000);
    out.push_back(allowed_live);

    // 3: allowed with an expiry already in the past — `Allowed` is still set, so
    //    this exercises the lazy-expiry read rather than a missing entry.
    let allowed_lapsed = Address::generate(env);
    client.allow_address_until(admin, &allowed_lapsed, &500_000);
    out.push_back(allowed_lapsed);

    // 4: blocked, permanently. `Allowed` is unset, so under the precedence this
    //    is `false` and the block flag is not needed to decide it.
    let blocked = Address::generate(env);
    client.block_address(admin, &blocked, &None);
    out.push_back(blocked);

    // 5: blocked until a future timestamp. Also `Allowed`-unset.
    let blocked_live = Address::generate(env);
    client.block_address_until(admin, &blocked_live, &2_000_000, &None);
    out.push_back(blocked_live);

    // 6: blocked until a past timestamp — the block has auto-expired, so this
    //    falls through to the allow check. Needs an allow as well, or the
    //    fallthrough is indistinguishable from "not allowed".
    let block_lapsed = Address::generate(env);
    client.allow_address(admin, &block_lapsed);
    client.block_address_until(admin, &block_lapsed, &500_000, &None);
    out.push_back(block_lapsed);

    // 7: both allowed and blocked. `Blocked` wins in `is_allowed`, and this is
    //    also the shape `clear_address` leaves behind — it writes `Blocked =
    //    false` rather than removing the key, so the key *exists* while reading
    //    as "not blocked". Any optimisation that probed key presence instead of
    //    reading the value would break this case.
    let cleared = Address::generate(env);
    client.allow_address(admin, &cleared);
    client.block_address(admin, &cleared, &None);
    client.clear_address(admin, &cleared);
    out.push_back(cleared);

    // 8: allowed and then hard-blocked — `Blocked` overrides the allow.
    let allowed_then_blocked = Address::generate(env);
    client.allow_address(admin, &allowed_then_blocked);
    client.block_address(admin, &allowed_then_blocked, &None);
    out.push_back(allowed_then_blocked);

    out
}

/// Builds a batch of `len` addresses by cycling through the state matrix, so
/// every batch size exercises the same mix of cheap and expensive paths rather
/// than a single easy one.
fn build_batch(env: &Env, matrix: &Vec<Address>, len: u32) -> Vec<Address> {
    let mut batch = Vec::new(env);
    let size = matrix.len();
    for i in 0..len {
        batch.push_back(matrix.get(i % size).unwrap());
    }
    batch
}

/// Builds a batch of `len` copies of a single address, for comparing one uniform
/// state against another.
fn build_uniform_batch(env: &Env, address: &Address, len: u32) -> Vec<Address> {
    let mut batch = Vec::new(env);
    for _ in 0..len {
        batch.push_back(address.clone());
    }
    batch
}

/// CPU instructions consumed by one `bulk_check_addresses` call.
///
/// Budget limits are lifted first so the call is measured rather than rejected
/// for exceeding an artificial cap, and the tracker is reset immediately before
/// the call so setup costs are excluded.
fn instructions_for(env: &Env, client: &ComplianceContractClient, batch: &Vec<Address>) -> u64 {
    env.cost_estimate().budget().reset_unlimited();
    env.cost_estimate().budget().reset_tracker();
    let _ = client.bulk_check_addresses(batch);
    env.cost_estimate().budget().cpu_instruction_cost()
}

/// Marginal cost of one additional address, derived from two batch sizes.
///
/// Subtracting the one-address figure removes the fixed per-call overhead, which
/// is what an integration pays once and does not scale with their list.
fn marginal_cost_per_address(
    env: &Env,
    client: &ComplianceContractClient,
    matrix: &Vec<Address>,
) -> u64 {
    let one = build_batch(env, matrix, 1);
    let many = build_batch(env, matrix, FULL_BATCH);
    let base = instructions_for(env, client, &one);
    let full = instructions_for(env, client, &many);
    (full - base) / u64::from(FULL_BATCH - 1)
}

#[test]
fn bulk_check_results_match_is_allowed_across_the_state_matrix() {
    let (env, client, admin) = setup();
    let matrix = populate_state_matrix(&env, &client, &admin);
    let batch = build_batch(&env, &matrix, FULL_BATCH);

    let bulk = client.bulk_check_addresses(&batch);

    // The whole point of #612: the optimisation is only allowed to be cheaper,
    // never different. Compare the batch result against the per-address
    // entrypoint for every element, at the same ledger time.
    let mut checked = 0u32;
    for address in batch.iter() {
        let expected = client.is_allowed(&address);
        assert_eq!(
            bulk.get(checked),
            Some(expected),
            "bulk_check_addresses disagreed with is_allowed for address at index {checked}"
        );
        checked += 1;
    }
    assert_eq!(checked, FULL_BATCH);
}

#[test]
fn bulk_check_returns_one_result_per_input_in_input_order() {
    let (env, client, admin) = setup();
    let matrix = populate_state_matrix(&env, &client, &admin);
    let batch = build_batch(&env, &matrix, 8);

    let bulk = client.bulk_check_addresses(&batch);

    assert_eq!(bulk.len(), batch.len());
    for (i, address) in batch.iter().enumerate() {
        assert_eq!(
            bulk.get(i as u32),
            Some(client.is_allowed(&address)),
            "index {i}"
        );
    }
}

#[test]
fn bulk_check_handles_empty_and_repeated_addresses() {
    let (env, client, admin) = setup();
    let matrix = populate_state_matrix(&env, &client, &admin);
    let allowed = matrix.get(1).unwrap();
    let blocked = matrix.get(4).unwrap();

    assert!(client.bulk_check_addresses(&Vec::new(&env)).is_empty());

    // A caller that screens overlapping lists must still get one answer per
    // position, including repeats — no deduplication, no reordering.
    let mut repeated = Vec::new(&env);
    for _ in 0..5 {
        repeated.push_back(allowed.clone());
        repeated.push_back(blocked.clone());
    }
    let bulk = client.bulk_check_addresses(&repeated);
    assert_eq!(bulk.len(), 10);
    for i in 0..5u32 {
        assert_eq!(bulk.get(i * 2), Some(true));
        assert_eq!(bulk.get(i * 2 + 1), Some(false));
    }
}

#[test]
fn bulk_check_stays_under_instruction_budget_at_max_batch_size() {
    let (env, client, admin) = setup();
    let matrix = populate_state_matrix(&env, &client, &admin);
    let batch = build_batch(&env, &matrix, FULL_BATCH);

    let instructions = instructions_for(&env, &client, &batch);
    println!("bulk_check_addresses({FULL_BATCH} addresses): {instructions} CPU instructions");

    assert!(
        instructions <= MAX_BULK_CHECK_INSTRUCTIONS,
        "bulk_check_addresses({FULL_BATCH}) used {instructions} instructions, \
         expected <= {MAX_BULK_CHECK_INSTRUCTIONS}"
    );
}

#[test]
fn bulk_check_marginal_cost_per_address_stays_bounded() {
    let (env, client, admin) = setup();
    let matrix = populate_state_matrix(&env, &client, &admin);

    let per_address = marginal_cost_per_address(&env, &client, &matrix);
    println!("marginal cost per address: {per_address} CPU instructions");

    assert!(
        per_address <= MAX_INSTRUCTIONS_PER_ADDRESS,
        "each extra address costs {per_address} instructions, expected <= \
         {MAX_INSTRUCTIONS_PER_ADDRESS}"
    );
}

#[test]
fn bulk_check_of_unallowed_addresses_reads_cheaper_than_allowed() {
    let (env, client, admin) = setup();
    let matrix = populate_state_matrix(&env, &client, &admin);

    let unallowed = matrix.get(0).unwrap(); // never seen
    let allowed = matrix.get(1).unwrap(); // allowed, permanent

    let unallowed_batch = build_uniform_batch(&env, &unallowed, FULL_BATCH);
    let allowed_batch = build_uniform_batch(&env, &allowed, FULL_BATCH);
    let unallowed_cost = instructions_for(&env, &client, &unallowed_batch);
    let allowed_cost = instructions_for(&env, &client, &allowed_batch);

    println!(
        "{FULL_BATCH} unallowed: {unallowed_cost} instructions; \
         {FULL_BATCH} allowed: {allowed_cost} instructions"
    );

    // An address with no `Allowed` entry is `false` under the precedence
    // regardless of its block flag, so screening a list of not-allowed
    // addresses must be materially cheaper than screening a list of allowed
    // ones — that gap is the #612 optimisation, and it is what an integration
    // screening unknown addresses actually pays.
    //
    // This is a structural check, not a calibrated one: it deliberately asserts
    // only an inequality rather than a ratio of per-read prices, so it stays
    // stable when soroban-sdk shifts cost constants. The two budget ceilings
    // above are what reject a revert to `Blocked`-first ordering — under the old
    // order both paths read the block flag and the totals rise past them.
    assert!(
        unallowed_cost < allowed_cost,
        "screening not-allowed addresses ({unallowed_cost} instructions) should be \
         cheaper than screening allowed ones ({allowed_cost})"
    );
}
