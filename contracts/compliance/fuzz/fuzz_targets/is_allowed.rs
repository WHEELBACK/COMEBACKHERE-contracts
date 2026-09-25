#![no_main]

//! Fuzz harness for `ComplianceContract::is_allowed` (#613).
//!
//! # Why a sequence fuzzer, not a state fuzzer
//!
//! `is_allowed` is a pure *read* over four storage keys per address
//! (`Blocked` / `BlockedUntil` / `Allowed` / `AllowedUntil`), but the values of
//! those keys are only ever reached through a long chain of admin mutations
//! (`allow_address`, `block_address_until`, `clear_address`, `revoke_allow`,
//! pause gating, ...) and through the passage of ledger time. A fuzzer that
//! picked a single final storage state would never exercise the transitions
//! between states, and several of the interesting bugs live in the
//! transitions: `block_address` deliberately does *not* clear a previously
//! written `BlockedUntil`, `clear_address` deliberately does *not* clear
//! `AllowedUntil`, and `require_not_paused` gates only *some* of the
//! allow/revoke mutations. So this harness fuzzes an **ordered sequence** of
//! mutations and time jumps, re-checking `is_allowed` against a model after
//! every step.
//!
//! # The model
//!
//! `Model` is a plain-Rust mirror of the four storage keys plus the `Paused`
//! flag, and `reference_is_allowed` re-derives the precedence rules purely from
//! `contracts/compliance/README.md#is_allowed-precedence`. It shares no code
//! with `contracts/compliance/src/lib.rs`, so a regression that drifts the real
//! implementation away from the documented spec shows up as a disagreement
//! rather than being mirrored in both places at once. This is the same
//! reference implementation style as
//! `contracts/compliance/tests/is_allowed_differential_test.rs`, extended from
//! a single final state to a whole op sequence.
//!
//! # Cost budget
//!
//! The `Env` is built fresh per input, so iteration cost is dominated by
//! contract registration. `MAX_OPS` and `SUBJECT_POOL` are deliberately small so
//! a 5-minute `cargo fuzz run` explores a very large number of sequences rather
//! than a few very long ones.

use arbitrary::Arbitrary;
use compliance::{ComplianceContract, ComplianceContractClient};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

/// Maximum number of operations replayed from one fuzz input.
///
/// Bounded so a single input can't dominate the run: libFuzzer mutates byte
/// strings, and an unbounded `Vec<Op>` would let a few long inputs eat the
/// whole time budget.
const MAX_OPS: usize = 12;

/// Number of distinct subject addresses the sequence cycles between.
///
/// Small on purpose — reusing addresses is what creates state *interactions*
/// (e.g. allow → block_until → clear → allow_until on the same address), which
/// is where the interesting behaviour is.
const SUBJECT_POOL: usize = 4;

/// One admin mutation or time jump in a fuzzed sequence.
#[derive(Debug, Clone, Arbitrary)]
enum Op {
    /// `allow_address` — permanent allow, clears any `AllowedUntil`.
    Allow { subject: u8 },
    /// `allow_address_until` — time-bound allow.
    AllowUntil { subject: u8, expires_at: u64 },
    /// `block_address` — permanent block. Note this does *not* clear a
    /// previously written `BlockedUntil`, which the model replicates.
    Block { subject: u8 },
    /// `block_address_until` — time-bound block.
    BlockUntil { subject: u8, unblock_at: u64 },
    /// `clear_address` — unblock and allow. Does *not* clear `AllowedUntil`.
    Clear { subject: u8 },
    /// `revoke_allow` — soft de-listing, permitted only while unpaused.
    Revoke { subject: u8 },
    /// `pause` — gates later allow/revoke mutations, but must not change what
    /// `is_allowed` reports for already-written state.
    Pause,
    /// `unpause`.
    Unpause,
    /// Move the ledger clock forward by `delta` seconds (saturating).
    AdvanceTime { delta: u64 },
    /// Move the ledger clock to an expiry timestamp this sequence previously
    /// wrote, plus a small signed `offset` — i.e. land exactly on, just before,
    /// or just after a block/allow boundary. This is where `now >= unblock_at`
    /// (inclusive) and `now < expires_at` (exclusive) diverge, and it is the
    /// single highest-value generator in this harness.
    JumpToBoundary { subject: u8, offset: i8 },
}

#[derive(Debug, Arbitrary)]
struct Input {
    ops: Vec<Op>,
}

/// Mirror of one subject address's compliance storage.
#[derive(Debug, Clone, Copy, Default)]
struct Model {
    allowed: bool,
    allowed_until: Option<u64>,
    blocked: bool,
    blocked_until: Option<u64>,
    /// Most recent expiry timestamp written for this subject by
    /// `AllowUntil` / `BlockUntil`, retained after the field is cleared so
    /// `JumpToBoundary` can keep aiming at interesting timestamps.
    last_boundary: Option<u64>,
}

/// Re-derivation of the precedence rules in
/// `contracts/compliance/README.md#is_allowed-precedence`, mirroring
/// `reference_is_allowed` in
/// `contracts/compliance/tests/is_allowed_differential_test.rs`:
///
/// 1. If `blocked` and no `blocked_until` (or `now < blocked_until`): `false`.
/// 2. If `blocked` and `now >= blocked_until`: the block has auto-expired, fall
///    through.
/// 3. If not `allowed`: `false`.
/// 4. If `allowed` and `allowed_until` is set: `true` iff `now < allowed_until`.
/// 5. If `allowed` with no expiry: `true`.
fn reference_is_allowed(m: &Model, now: u64) -> bool {
    if m.blocked {
        match m.blocked_until {
            Some(unblock_at) if now >= unblock_at => {
                // Block auto-expired — fall through.
            }
            _ => return false,
        }
    }
    if !m.allowed {
        return false;
    }
    match m.allowed_until {
        Some(expires_at) => now < expires_at,
        None => true,
    }
}

/// Applies the storage effect of `op`, skipping mutations the contract rejects.
///
/// `paused` matters here because `require_not_paused` gates `allow_address`,
/// `allow_address_until` and `revoke_allow`, while `block_address`,
/// `block_address_until` and `clear_address` stay available while paused as
/// part of the emergency-remediation policy. Getting this wrong would make the
/// harness report a false positive, so it mirrors the pause policy documented
/// on each entrypoint rather than assuming a blanket "paused means read-only".
fn apply(m: &mut Model, op: &Op, paused: bool) {
    match *op {
        Op::Allow { .. } if !paused => {
            m.allowed = true;
            m.allowed_until = None;
        }
        Op::AllowUntil { expires_at, .. } if !paused => {
            m.allowed = true;
            m.allowed_until = Some(expires_at);
            m.last_boundary = Some(expires_at);
        }
        Op::Block { .. } => {
            // `block_address` does not touch `BlockedUntil`, so an earlier
            // `block_address_until` expiry survives and keeps being evaluated.
            m.blocked = true;
        }
        Op::BlockUntil { unblock_at, .. } => {
            m.blocked = true;
            m.blocked_until = Some(unblock_at);
            m.last_boundary = Some(unblock_at);
        }
        Op::Clear { .. } => {
            m.blocked = false;
            m.blocked_until = None;
            // `clear_address` leaves `AllowedUntil` in place, so a re-allowed
            // address can still be governed by an older expiry.
            m.allowed = true;
        }
        Op::Revoke { .. } if !paused => {
            m.allowed = false;
            m.allowed_until = None;
        }
        _ => {}
    }
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register(ComplianceContract, ());
    let client = ComplianceContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    let subjects: [Address; SUBJECT_POOL] = std::array::from_fn(|_| Address::generate(&env));
    let mut model = [Model::default(); SUBJECT_POOL];
    let mut now: u64 = env.ledger().timestamp();
    let mut paused = false;

    let mut log: Vec<String> = Vec::new();

    for op in input.ops.iter().take(MAX_OPS) {
        let subject_idx = (match op {
            Op::Allow { subject }
            | Op::AllowUntil { subject, .. }
            | Op::Block { subject }
            | Op::BlockUntil { subject, .. }
            | Op::Clear { subject }
            | Op::Revoke { subject }
            | Op::JumpToBoundary { subject, .. } => *subject as usize,
            Op::Pause | Op::Unpause | Op::AdvanceTime { .. } => 0,
        }) % SUBJECT_POOL;

        // Record the op before applying it so a failure message reproduces the
        // exact sequence that diverged, ready to be lifted into a regression
        // test in contracts/compliance/tests/.
        log.push(format!(
            "{op:?} (subject={subject_idx}, now_before={now}, paused={paused})"
        ));

        match *op {
            Op::Allow { .. } => {
                let _ = client.try_allow_address(&admin, &subjects[subject_idx]);
            }
            Op::AllowUntil { expires_at, .. } => {
                let _ = client.try_allow_address_until(&admin, &subjects[subject_idx], &expires_at);
            }
            Op::Block { .. } => {
                let _ = client.try_block_address(&admin, &subjects[subject_idx], &None);
            }
            Op::BlockUntil { unblock_at, .. } => {
                let _ =
                    client.try_block_address_until(&admin, &subjects[subject_idx], &unblock_at, &None);
            }
            Op::Clear { .. } => {
                let _ = client.try_clear_address(&admin, &subjects[subject_idx]);
            }
            Op::Revoke { .. } => {
                let _ = client.try_revoke_allow(&admin, &subjects[subject_idx]);
            }
            Op::Pause => {
                let _ = client.try_pause(&admin);
                paused = true;
            }
            Op::Unpause => {
                let _ = client.try_unpause(&admin);
                paused = false;
            }
            Op::AdvanceTime { delta } => {
                now = now.saturating_add(delta);
                env.ledger().set_timestamp(now);
            }
            Op::JumpToBoundary { offset, .. } => {
                if let Some(boundary) = model[subject_idx].last_boundary {
                    now = boundary.saturating_add_signed(offset as i64);
                    env.ledger().set_timestamp(now);
                }
            }
        }

        apply(&mut model[subject_idx], op, paused);

        // Check every subject, not just the one this op touched: a time jump in
        // particular changes the answer for addresses it never mentions.
        for (i, subject) in subjects.iter().enumerate() {
            let expected = reference_is_allowed(&model[i], now);
            let actual = client.is_allowed(subject);
            assert_eq!(
                actual, expected,
                "is_allowed disagreed with the model for subject {i} after:\n{}\n\
                 (subject={i} expected={expected} actual={actual} now={now} paused={paused})",
                log.join("\n")
            );
        }
    }
});
