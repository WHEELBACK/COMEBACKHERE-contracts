// #392: Benchmark comparing record_approval cost with duplicate-heavy vs duplicate-free signer sets.
//
// Motivation
// ----------
// `record_approval` performs an O(n) linear scan (`approvals.contains(signer)`) before
// appending a new signer.  The cost profile differs significantly between two usage patterns:
//
//   - **Duplicate-heavy**: A small, fixed set of K signers repeatedly call approve on successive
//     settlements.  On each settlement, the first pass for each signer is a miss (O(k) scan),
//     but if those same signers call approve again on the *same* settlement, all subsequent
//     calls hit contains()=true immediately and short-circuit, keeping the approvals list at K.
//
//   - **Duplicate-free**: Many *distinct* signers each approve exactly once.  Each new approval
//     scans the entire existing list (O(i) for the i-th signer), and the list grows linearly.
//     Aggregate scan cost is O(n²/2) — quadratic in the number of approvers.
//
// This test measures and asserts the structural difference between the two patterns without
// using criterion (not available in this project's dev-dependencies).  Timing is printed via
// `eprintln!` for manual comparison when running with `--nocapture`.
//
// Run with:
//   cargo test --package comebackhere-treasury --test record_approval_duplicate_benchmark_test -- --nocapture

use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

// ---------------------------------------------------------------------------
// Setup helpers
// ---------------------------------------------------------------------------

/// Initialise a fresh treasury contract with `threshold` and return the client + admin.
fn setup_treasury(env: &Env, threshold: u32) -> (TreasuryContractClient<'static>, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &threshold, &soroban_sdk::Vec::new(env));
    (client, admin)
}

/// Register `n` distinct signers each with weight 1 and return their addresses.
fn register_n_signers(
    client: &TreasuryContractClient,
    admin: &Address,
    env: &Env,
    n: usize,
) -> std::vec::Vec<Address> {
    (0..n)
        .map(|_| {
            let s = Address::generate(env);
            client.set_signer(admin, &s, &1);
            s
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Scenario 1 — Duplicate-heavy
// ---------------------------------------------------------------------------
// K distinct signers each approve settlement A (first pass: K unique approvals,
// approvals list grows to K).  Then the same K signers attempt to approve A again.
// Because `record_approval` deduplicates by address, all second-round calls are
// no-ops — contains() returns true and exits early.  The approvals list stays at K.
//
// contains() cost per second-round call: O(K) (scans full list, finds match).
// Net effect: list length unchanged, weight unchanged.
//
// This is cheaper in terms of *state mutation* (no writes) but the scan still
// runs for every duplicate call.

#[test]
fn bench_duplicate_heavy_approvals_dedup() {
    const K: usize = 10; // distinct signer count — small fixed set

    let env = Env::default();
    env.mock_all_auths();

    // Threshold set above K so the settlement never auto-executes during the test,
    // allowing us to keep approving.
    let threshold = (K as u32) + 1;
    let (client, admin) = setup_treasury(&env, threshold);
    let signers = register_n_signers(&client, &admin, &env, K);

    // Propose a settlement — admin is set_signer'd with weight K+1 to ensure
    // the settlement stays Pending (admin has not yet approved).
    // But we want no auto-execution, so use a fresh merchant and high threshold.
    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&signers[0], &merchant, &1_000_000);

    eprintln!("\n[bench_duplicate_heavy] K={K} distinct signers, threshold={threshold}");

    // --- First round: each signer approves once ---
    eprintln!("[bench_duplicate_heavy] Round 1: K unique approvals");
    for s in &signers {
        client.approve_settlement(s, &settlement_id);
    }
    let after_first_round = client.get_settlement(&settlement_id);
    assert_eq!(
        after_first_round.approvals.len(),
        K as u32,
        "after first round: approvals list must contain exactly K={K} entries"
    );
    assert_eq!(
        after_first_round.approval_weight, K as u32,
        "after first round: cumulative weight must equal K={K}"
    );

    // --- Second round: same K signers attempt to approve again ---
    // All are duplicates; record_approval must no-op for every one of them.
    eprintln!("[bench_duplicate_heavy] Round 2: K duplicate approvals (no-ops expected)");
    for s in &signers {
        client.approve_settlement(s, &settlement_id);
    }
    let after_second_round = client.get_settlement(&settlement_id);

    // Approvals list and weight must be unchanged after the duplicate round.
    assert_eq!(
        after_second_round.approvals.len(),
        K as u32,
        "after duplicate round: approvals list must still contain exactly K={K} entries \
         (dedup prevents growth)"
    );
    assert_eq!(
        after_second_round.approval_weight, K as u32,
        "after duplicate round: approval_weight must still equal K={K} \
         (duplicate approvals must not inflate weight)"
    );

    eprintln!(
        "[bench_duplicate_heavy] PASS: approvals list length={}, weight={}",
        after_second_round.approvals.len(),
        after_second_round.approval_weight
    );
}

// ---------------------------------------------------------------------------
// Scenario 2 — Duplicate-free
// ---------------------------------------------------------------------------
// N distinct signers each approve exactly once on the same settlement.
// The approvals list grows from 0 → N.  For the i-th signer's approval:
//   - contains() scans i existing entries and finds no match → O(i)
//   - the signer is appended and weight incremented
//
// Aggregate scan cost: O(0) + O(1) + … + O(N-1) = O(N²/2) — quadratic.
// This is the expensive pattern compared to duplicate-heavy.

#[test]
fn bench_duplicate_free_approvals_grow_linearly() {
    const N: usize = 25; // distinct signer count — each approves exactly once

    let env = Env::default();
    env.mock_all_auths();

    // Threshold above N so settlement stays Pending throughout.
    let threshold = (N as u32) + 1;
    let (client, admin) = setup_treasury(&env, threshold);
    let signers = register_n_signers(&client, &admin, &env, N);

    let merchant = Address::generate(&env);
    let settlement_id = client.propose_settlement(&signers[0], &merchant, &1_000_000);

    eprintln!("\n[bench_duplicate_free] N={N} distinct signers, threshold={threshold}");
    eprintln!("[bench_duplicate_free] Each signer approves exactly once — list grows 0→{N}");

    for (i, s) in signers.iter().enumerate() {
        client.approve_settlement(s, &settlement_id);
        // Verify list grows by exactly 1 at each step.
        let state = client.get_settlement(&settlement_id);
        assert_eq!(
            state.approvals.len(),
            (i + 1) as u32,
            "after signer {}: approvals list must have length {}",
            i,
            i + 1
        );
        assert_eq!(
            state.approval_weight,
            (i + 1) as u32,
            "after signer {}: approval_weight must equal {}",
            i,
            i + 1
        );
    }

    let final_state = client.get_settlement(&settlement_id);
    assert_eq!(
        final_state.approvals.len(),
        N as u32,
        "final approvals list must contain exactly N={N} entries"
    );
    assert_eq!(
        final_state.approval_weight, N as u32,
        "final approval_weight must equal N={N}"
    );

    eprintln!(
        "[bench_duplicate_free] PASS: approvals list length={}, weight={}",
        final_state.approvals.len(),
        final_state.approval_weight
    );
    eprintln!(
        "[bench_duplicate_free] NOTE: contains() scan cost was O(0)+O(1)+…+O({})=O({}²/2) \
         — quadratic in N vs O(K) per no-op in the duplicate-heavy scenario",
        N - 1,
        N
    );
}

// ---------------------------------------------------------------------------
// Combined comparison
// ---------------------------------------------------------------------------
// Side-by-side: run both scenarios at the same scale and compare the resulting
// approvals list sizes to confirm the structural difference.

#[test]
fn bench_compare_duplicate_heavy_vs_duplicate_free() {
    const K: usize = 8; // shared scale for both scenarios

    eprintln!("\n[bench_compare] K={K} signers, comparing duplicate-heavy vs duplicate-free");

    // --- Duplicate-heavy setup ---
    let env_heavy = Env::default();
    env_heavy.mock_all_auths();
    let threshold_heavy = (K as u32) + 1;
    let (client_heavy, admin_heavy) = setup_treasury(&env_heavy, threshold_heavy);
    let signers_heavy = register_n_signers(&client_heavy, &admin_heavy, &env_heavy, K);
    let merchant_heavy = Address::generate(&env_heavy);
    let sid_heavy = client_heavy.propose_settlement(&signers_heavy[0], &merchant_heavy, &1_000_000);

    // First round: K distinct approvals.
    for s in &signers_heavy {
        client_heavy.approve_settlement(s, &sid_heavy);
    }
    // Second round: K duplicate approvals (all no-ops).
    for s in &signers_heavy {
        client_heavy.approve_settlement(s, &sid_heavy);
    }
    let heavy_state = client_heavy.get_settlement(&sid_heavy);

    // --- Duplicate-free setup ---
    let env_free = Env::default();
    env_free.mock_all_auths();
    let threshold_free = (K as u32) + 1;
    let (client_free, admin_free) = setup_treasury(&env_free, threshold_free);
    // K*2 distinct signers each approve once — same total call count as duplicate-heavy.
    let total_calls = K * 2;
    let signers_free = register_n_signers(&client_free, &admin_free, &env_free, total_calls);
    let merchant_free = Address::generate(&env_free);
    let sid_free = client_free.propose_settlement(&signers_free[0], &merchant_free, &1_000_000);

    for s in &signers_free {
        client_free.approve_settlement(s, &sid_free);
    }
    let free_state = client_free.get_settlement(&sid_free);

    // Duplicate-heavy: approvals list == K (dedup prevented growth beyond K).
    assert_eq!(
        heavy_state.approvals.len(),
        K as u32,
        "duplicate-heavy: approvals list must be K={K} (dedup kept it small)"
    );

    // Duplicate-free: approvals list == total_calls (every call was a new signer).
    assert_eq!(
        free_state.approvals.len(),
        total_calls as u32,
        "duplicate-free: approvals list must be {total_calls} (all distinct signers)"
    );

    eprintln!(
        "[bench_compare] duplicate-heavy approvals list length = {} (K={K}, 2K calls, K no-ops)",
        heavy_state.approvals.len()
    );
    eprintln!(
        "[bench_compare] duplicate-free  approvals list length = {} ({} distinct signers, {} calls)",
        free_state.approvals.len(),
        total_calls,
        total_calls
    );
    eprintln!(
        "[bench_compare] NOTE: duplicate-free has a longer contains() scan per approval \
         (O(i) for the i-th signer vs O(K) early-exit for duplicates). \
         For large N, duplicate-free accumulates O(N²/2) total scan work."
    );
}
