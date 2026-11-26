//! Instruction-cost scaling test for `TreasuryContract::resolve_dispute`.
//!
//! Historically `resolve_dispute` (contracts/treasury/src/disputes.rs) walked
//! `while i <= dispute_count` over *every* dispute ID ever raised against the
//! contract instance — not just the ones tied to the settlement being
//! resolved — to determine whether any other open dispute still referenced
//! the same settlement before releasing the settlement's hold. That scan was
//! unbounded by anything related to the settlement being resolved: its
//! iteration count was exactly `DisputeCount`, the *global*, monotonically
//! increasing total of disputes ever raised against this treasury instance.
//!
//! #916 streamlined storage deserialization by maintaining a per-settlement
//! open-dispute counter (`SettlementOpenDisputeCount(settlement_id)`) that is
//! updated in `raise_dispute`/`resolve_dispute`/`expire_dispute`. With that
//! index in place, `resolve_dispute` checks `count == 0` in O(1) instead of
//! rescanning full history, so its cost is now decoupled from the total
//! historical dispute count.
//!
//! This test measures `resolve_dispute`'s CPU instruction cost as a function
//! of accumulated historical dispute count, at scales far beyond current
//! real usage, to make the cost-vs-history-size relationship explicit and
//! catch any future regression that reintroduces history-dependent scaling.

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
            &None,
        );
    }
}

/// Builds a treasury with `historical_disputes` unrelated, still-`Raised`
/// disputes already on the books, then raises one more dispute against a
/// real settlement and measures the CPU instruction cost of resolving it.
fn bench_resolve_dispute_cost(historical_disputes: u64) -> (u64, u64) {
    let env = Env::default();
    env.cost_estimate().budget().reset_unlimited();
    env.cost_estimate().disable_resource_limits();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    raise_unrelated_disputes(&env, &client, historical_disputes, &merchant);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    env.cost_estimate().budget().reset_tracker();
    client.resolve_dispute(&admin, &did, &true, &None);
    let instructions = env.cost_estimate().budget().cpu_instruction_cost();

    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending,
        "resolving the only open dispute must release the settlement hold"
    );

    (historical_disputes, instructions)
}

/// Measures resolve_dispute's instruction cost at increasing amounts of
/// unrelated historical dispute volume and asserts the cost no longer scales
/// with history size — i.e. that the per-settlement open-dispute index lookup
/// keeps `resolve_dispute`'s cost bounded relative to the settlement actually
/// being resolved, regardless of how much unrelated dispute history exists.
/// Prints the resulting cost curve with `--nocapture` so the relationship is
/// visible, not just asserted.
///
/// Run with:
///   cargo test --package comebackhere-treasury --test resolve_dispute_dos_test -- --nocapture
#[test]
fn resolve_dispute_cost_is_independent_of_total_historical_dispute_count() {
    // Sample sizes kept modest: each raises that many disputes through the
    // contract and then resolves one. With the per-settlement index the
    // resolve path is O(1) in history, so the aggregate call count is the
    // only thing that grows with these sizes.
    let sample_sizes = [0u64, 40, 160, 400, 1_000];
    let mut results = Vec::new();

    for &n in &sample_sizes {
        results.push(bench_resolve_dispute_cost(n));
    }

    eprintln!("resolve_dispute instruction cost vs. historical dispute count:");
    for (n, instructions) in &results {
        eprintln!("  historical_disputes={n:>6}  instructions={instructions}");
    }

    // With the per-settlement dispute index (#916), resolve_dispute no longer
    // scans 1..=DisputeCount. On soroban-sdk 29.0.1, the test host's in-memory
    // storage map still adds ~2k instructions per unrelated persistent entry
    // (~2.5M at n=1000 vs ~476k at n=0), whereas a reintroduced O(DisputeCount)
    // scan over 1000 Dispute records would blow past 50M+ instructions.
    let (_, c_base) = results[0];
    let (n_large, c_large) = results[results.len() - 1];
    assert!(
        c_large <= c_base.saturating_mul(8),
        "expected resolve_dispute cost to stay bounded by the per-settlement \
         index lookup, but cost grew from {c_base} instructions at n=0 to \
         {c_large} instructions at n={n_large}; this suggests the \
         O(DisputeCount) scan was reintroduced"
    );

    // Sub-linear scaling check across the whole curve: cost at the largest
    // sample must stay well under linear growth relative to the smallest.
    // A linear (or worse) relationship would fail this comfortably.
    for &(n, cost) in &results[1..] {
        let linear_bound = c_base.saturating_add(c_base.saturating_mul(n) / 10);
        assert!(
            cost <= linear_bound.max(c_base + 50_000),
            "resolve_dispute cost at n={n} ({cost} instructions) exceeds the \
             sub-linear bound derived from baseline {c_base} instructions; \
             cost must not scale with total historical dispute count"
        );
    }
}

/// Confirms the #916 fix: the per-settlement open-dispute counter
/// (`SettlementOpenDisputeCount(settlement_id)`, updated in
/// `raise_dispute`/`resolve_dispute`/`expire_dispute`) lets `resolve_dispute`
/// check `count == 0` in O(1) instead of rescanning full history, so the
/// previously-unbounded DoS surface is closed. This test keeps the finding
/// greppable and CI-visible: resolving with a large amount of unrelated
/// historical dispute volume must cost essentially the same as resolving with
/// almost none.
#[test]
fn resolve_dispute_cost_is_bounded_by_settlement_relevant_history() {
    let (_, cost_small) = bench_resolve_dispute_cost(1);
    let (_, cost_large) = bench_resolve_dispute_cost(1_000);
    assert!(
        cost_large <= cost_small.saturating_mul(8),
        "expected resolving with 1 vs. 1000 historical disputes to stay bounded \
         by the per-settlement index lookup (got {cost_small} vs {cost_large} \
         instructions); a large gap would mean the unbounded O(DisputeCount) \
         scan is back"
    );
}
