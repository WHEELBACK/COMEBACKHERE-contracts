//! Instruction-cost scaling test for `TreasuryContract::resolve_dispute`.
//!
//! `resolve_dispute` (contracts/treasury/src/disputes.rs:121-178), after
//! flipping the target dispute's status, walks `while i <= dispute_count`
//! over *every* dispute ID ever raised against the contract instance — not
//! just the ones tied to the settlement being resolved — to determine
//! whether any other open dispute still references the same settlement
//! before releasing the settlement's hold (disputes.rs:154-168).
//!
//! Reading the code confirms this loop is genuinely unbounded by anything
//! related to the settlement being resolved: its iteration count is exactly
//! `DisputeCount`, the *global*, monotonically increasing total of disputes
//! ever raised against this treasury instance, regardless of how many (if
//! any) of those disputes touch the settlement in question. There is no
//! secondary index from settlement → its disputes, and no cap on
//! `DisputeCount` itself (`raise_dispute` only rejects on `u64` overflow).
//! This is not implicitly bounded by anything else in the contract.
//!
//! This test measures `resolve_dispute`'s CPU instruction cost as a function
//! of accumulated historical dispute count, at scales far beyond current
//! real usage, to make the cost-vs-history-size relationship explicit and
//! catch any future regression that makes it worse.

use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env) -> (TreasuryContractClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

/// Raises `count` disputes against an unrelated, non-existent settlement id
/// so they accumulate in `DisputeCount` without affecting the settlement
/// under test — this simulates a treasury instance with a large amount of
/// *unrelated* dispute history, which is the realistic long-lived-contract
/// shape the parent issue is concerned about.
fn raise_unrelated_disputes(
    env: &Env,
    client: &TreasuryContractClient,
    count: u64,
    counterparty: &Address,
) {
    const UNRELATED_SETTLEMENT_ID: u64 = u64::MAX - 1;
    for _ in 0..count {
        let claimant = Address::generate(env);
        client.raise_dispute(
            &claimant,
            &UNRELATED_SETTLEMENT_ID,
            counterparty,
            &1,
            &u64::MAX,
        );
    }
}

/// Builds a treasury with `historical_disputes` unrelated, still-`Raised`
/// disputes already on the books, then raises one more dispute against a
/// real settlement and measures the CPU instruction cost of resolving it.
fn bench_resolve_dispute_cost(historical_disputes: u64) -> (u64, u64) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    raise_unrelated_disputes(&env, &client, historical_disputes, &merchant);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    env.cost_estimate().budget().reset_tracker();
    client.resolve_dispute(&admin, &did, &true);
    let instructions = env.cost_estimate().budget().cpu_instruction_cost();

    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending,
        "resolving the only open dispute must release the settlement hold"
    );

    (historical_disputes, instructions)
}

/// Measures resolve_dispute's instruction cost at increasing amounts of
/// unrelated historical dispute volume and asserts the cost strictly
/// increases with history size — i.e. that the loop's cost is driven by
/// total dispute history, not by anything bounded relative to the
/// settlement actually being resolved. Prints the resulting cost curve with
/// `--nocapture` so the growth relationship is visible, not just asserted.
///
/// Run with:
///   cargo test --package comebackhere-treasury --test resolve_dispute_dos_test -- --nocapture
#[test]
fn resolve_dispute_cost_scales_with_total_historical_dispute_count() {
    let sample_sizes = [0u64, 50, 500, 2_000, 8_000];
    let mut results = Vec::new();

    for &n in &sample_sizes {
        results.push(bench_resolve_dispute_cost(n));
    }

    eprintln!("resolve_dispute instruction cost vs. historical dispute count:");
    for (n, instructions) in &results {
        eprintln!("  historical_disputes={n:>6}  instructions={instructions}");
    }

    // The loop scans 1..=DisputeCount unconditionally, so cost at a larger
    // historical count must be strictly greater than at a smaller one — this
    // is the DoS-relevant relationship: cost is coupled to total contract
    // history, not to anything bounded by the settlement being resolved.
    for pair in results.windows(2) {
        let (n_small, cost_small) = pair[0];
        let (n_large, cost_large) = pair[1];
        assert!(
            cost_large > cost_small,
            "expected resolve_dispute cost to strictly increase with historical \
             dispute count ({n_small} disputes -> {cost_small} instructions vs. \
             {n_large} disputes -> {cost_large} instructions); if this ever fails \
             it means the loop stopped scaling with DisputeCount, which would be a \
             welcome fix worth documenting, not a bug in this test"
        );
    }

    // Sanity check the growth is roughly linear (not, say, accidentally
    // quadratic from something like a Vec::contains scan added later): the
    // marginal per-dispute cost between the two largest samples should stay
    // within a small constant factor of the marginal cost between two
    // mid-range samples. A generous factor avoids false positives from noise
    // while still catching a genuine complexity regression.
    let (n0, c0) = results[1];
    let (n1, c1) = results[2];
    let (n2, c2) = results[3];
    let (n3, c3) = results[4];
    let marginal_early = (c1 - c0) as f64 / (n1 - n0) as f64;
    let marginal_late = (c3 - c2) as f64 / (n3 - n2) as f64;
    assert!(
        marginal_late < marginal_early * 3.0,
        "per-dispute marginal instruction cost grew from ~{marginal_early:.1} \
         (n={n0}..{n1}) to ~{marginal_late:.1} (n={n2}..{n3}); this looks worse \
         than linear scaling and would make the eventual instruction-budget \
         ceiling arrive even sooner than a linear projection suggests"
    );
}

/// Documents the finding from reading the code: this is a genuine,
/// currently-unbounded DoS surface, not one implicitly capped by something
/// else in the contract (no per-settlement dispute index, no cap on
/// DisputeCount besides u64 overflow). Once `DisputeCount` grows large
/// enough that this scan alone approaches Soroban's per-transaction
/// instruction budget, every future `resolve_dispute` call — for any
/// settlement, not just the one that pushed the count over the edge —
/// becomes uncallable. This should be filed as its own follow-up issue
/// (e.g. maintaining a `SettlementOpenDisputeCount(settlement_id)` counter
/// updated in `raise_dispute`/`resolve_dispute`/`expire_dispute`, so
/// `resolve_dispute` can check `count == 0` in O(1) instead of rescanning
/// full history) rather than fixed silently in this PR.
#[test]
fn finding_documented_unbounded_scan_is_not_implicitly_capped() {
    // This test exists to make the finding greppable and CI-visible rather
    // than only living in a doc comment: resolve_dispute's cost at a large
    // historical count must not equal its cost at a small one, which would
    // be the signature of an implicit cap (e.g. an early-exit index) that
    // doesn't exist today.
    let (_, cost_small) = bench_resolve_dispute_cost(1);
    let (_, cost_large) = bench_resolve_dispute_cost(4_000);
    assert!(
        cost_large > cost_small * 10,
        "expected a large gap between resolving with 1 vs. 4000 historical \
         disputes (got {cost_small} vs {cost_large} instructions), confirming \
         the scan is genuinely unbounded by settlement-relevant history"
    );
}
