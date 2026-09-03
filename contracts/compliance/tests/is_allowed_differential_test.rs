//! Differential test for `ComplianceContract::is_allowed`.
//!
//! `is_allowed` is a multi-condition, multi-storage-key decision function
//! (Blocked / BlockedUntil / Allowed / AllowedUntil, per the precedence rules
//! documented in `contracts/compliance/README.md#is_allowed-precedence`,
//! formalized in #56). Example-based unit tests in `compliance_test.rs` cover
//! specific scenarios, but a copy-paste second test case can silently inherit
//! the same bug the implementation has. This file instead reimplements the
//! documented precedence rules as a standalone, dependency-free reference
//! function (`reference_is_allowed`, below) and asserts it agrees with the
//! real contract across the full combinatorial state space plus randomized
//! ledger timestamps near the block/allow expiry boundaries.
//!
//! The reference function shares no code with `contracts/compliance/src/lib.rs`
//! — it is re-derived purely from the README's four-step description, so a
//! regression that quietly drifts the real implementation away from that spec
//! (e.g. swapping the order of the Blocked/Allowed checks) will produce a
//! disagreement here even though it would not show up in a test that just adds
//! more examples of the same code path.

use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

/// Independent reimplementation of the precedence rules documented in
/// `contracts/compliance/README.md`:
///
/// 1. If `blocked` and no `blocked_until` (or `now < blocked_until`): `false`.
/// 2. If `blocked` and `now >= blocked_until`: the block has auto-expired,
///    fall through to step 3.
/// 3. If not `allowed`: `false`.
/// 4. If `allowed` and `allowed_until` is set: `true` iff `now < allowed_until`.
/// 5. If `allowed` with no expiry: `true`.
fn reference_is_allowed(
    blocked: bool,
    blocked_until: Option<u64>,
    allowed: bool,
    allowed_until: Option<u64>,
    now: u64,
) -> bool {
    if blocked {
        match blocked_until {
            Some(unblock_at) if now >= unblock_at => {
                // Block auto-expired — fall through.
            }
            _ => return false,
        }
    }
    if !allowed {
        return false;
    }
    match allowed_until {
        Some(expires_at) => now < expires_at,
        None => true,
    }
}

/// One randomized (blocked, blocked_until, allowed, allowed_until, now) storage
/// state to drive both the real contract and the reference implementation.
struct Case {
    blocked: bool,
    blocked_until: Option<u64>,
    allowed: bool,
    allowed_until: Option<u64>,
    now: u64,
}

/// Tiny deterministic PRNG (xorshift64) so the "randomized" sweep below is
/// reproducible across runs without pulling in a new dev-dependency.
struct Xorshift64(u64);

impl Xorshift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + (self.next() % (hi - lo + 1))
    }

    fn next_bool(&mut self) -> bool {
        self.next() % 2 == 0
    }
}

const BASE_NOW: u64 = 1_000_000;

fn generate_cases() -> Vec<Case> {
    let mut cases = Vec::new();

    // Exhaustive sweep over the discrete axes: blocked x blocked_until-shape x
    // allowed x allowed_until-shape, at a fixed `now`. This alone covers every
    // branch combination in the documented precedence rules.
    let blocked_shapes: [Option<Option<i64>>; 4] = [
        None,                // not blocked
        Some(None),          // blocked, permanent (no BlockedUntil)
        Some(Some(-500)),    // blocked, BlockedUntil in the past (expired)
        Some(Some(500_000)), // blocked, BlockedUntil far in the future (active)
    ];
    let allowed_shapes: [Option<Option<i64>>; 4] = [
        None,                // not allowed
        Some(None),          // allowed, permanent (no AllowedUntil)
        Some(Some(-500)),    // allowed, AllowedUntil in the past (expired)
        Some(Some(500_000)), // allowed, AllowedUntil far in the future (active)
    ];

    for b in blocked_shapes {
        for a in allowed_shapes {
            let (blocked, blocked_until) = match b {
                None => (false, None),
                Some(None) => (true, None),
                Some(Some(off)) => (true, Some((BASE_NOW as i64 + off) as u64)),
            };
            let (allowed, allowed_until) = match a {
                None => (false, None),
                Some(None) => (true, None),
                Some(Some(off)) => (true, Some((BASE_NOW as i64 + off) as u64)),
            };
            cases.push(Case {
                blocked,
                blocked_until,
                allowed,
                allowed_until,
                now: BASE_NOW,
            });
        }
    }

    // Boundary cases: `now` exactly equal to the expiry timestamp. Blocked
    // uses `>=` (inclusive expiry) while Allowed uses strict `<` (exclusive
    // expiry) per the documented rules — these are exactly the kind of
    // off-by-one details a differential test is meant to catch.
    cases.push(Case {
        blocked: true,
        blocked_until: Some(BASE_NOW),
        allowed: true,
        allowed_until: None,
        now: BASE_NOW,
    });
    cases.push(Case {
        blocked: false,
        blocked_until: None,
        allowed: true,
        allowed_until: Some(BASE_NOW),
        now: BASE_NOW,
    });

    // Randomized sweep: many random combinations of the same axes with random
    // `now` values clustered around the expiry boundaries, to fuzz timestamp
    // edge cases the hand-picked exhaustive sweep above might not hit exactly.
    let mut rng = Xorshift64(0x5EED_C0FF_EE15_BEEF);
    for _ in 0..500 {
        let blocked = rng.next_bool();
        let blocked_until = if blocked && rng.next_bool() {
            Some(rng.next_range(BASE_NOW - 1000, BASE_NOW + 1000))
        } else {
            None
        };
        let allowed = rng.next_bool();
        let allowed_until = if allowed && rng.next_bool() {
            Some(rng.next_range(BASE_NOW - 1000, BASE_NOW + 1000))
        } else {
            None
        };
        let now = rng.next_range(BASE_NOW - 1500, BASE_NOW + 1500);
        cases.push(Case {
            blocked,
            blocked_until,
            allowed,
            allowed_until,
            now,
        });
    }

    cases
}

/// Drives the real contract into the state described by `case` using a fresh
/// address per case, then returns `is_allowed` for comparison against the
/// reference implementation.
fn real_is_allowed(
    env: &Env,
    client: &ComplianceContractClient,
    admin: &Address,
    case: &Case,
) -> bool {
    let address = Address::generate(env);

    if case.allowed {
        match case.allowed_until {
            Some(expires_at) => {
                client.allow_address_until(admin, &address, &expires_at);
            }
            None => {
                client.allow_address(admin, &address);
            }
        }
    }
    if case.blocked {
        match case.blocked_until {
            Some(unblock_at) => {
                client.block_address_until(admin, &address, &unblock_at, &None);
            }
            None => {
                client.block_address(admin, &address, &None);
            }
        }
    }

    env.ledger().set_timestamp(case.now);
    client.is_allowed(&address)
}

#[test]
fn is_allowed_matches_independent_reference_implementation() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);

    let cases = generate_cases();
    assert!(
        cases.len() >= 500,
        "expected a large randomized case set, got {}",
        cases.len()
    );

    let mut mismatches = Vec::new();
    for (i, case) in cases.iter().enumerate() {
        let expected = reference_is_allowed(
            case.blocked,
            case.blocked_until,
            case.allowed,
            case.allowed_until,
            case.now,
        );
        let actual = real_is_allowed(&env, &client, &admin, case);
        if actual != expected {
            mismatches.push(format!(
                "case {i}: blocked={} blocked_until={:?} allowed={} allowed_until={:?} now={} \
                 -> contract={actual} reference={expected}",
                case.blocked, case.blocked_until, case.allowed, case.allowed_until, case.now
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "is_allowed disagreed with the independent reference implementation in {} of {} cases:\n{}",
        mismatches.len(),
        cases.len(),
        mismatches.join("\n")
    );
}

/// Sanity check on the reference function itself: block-beats-allow is the
/// core precedence rule under test, so pin it down directly in addition to
/// the differential sweep above.
#[test]
fn reference_implementation_blocks_beat_allows() {
    assert!(!reference_is_allowed(true, None, true, None, BASE_NOW));
    assert!(reference_is_allowed(false, None, true, None, BASE_NOW));
    assert!(!reference_is_allowed(false, None, false, None, BASE_NOW));
    // Expired block falls through to the allow check.
    assert!(reference_is_allowed(
        true,
        Some(BASE_NOW - 1),
        true,
        None,
        BASE_NOW
    ));
    // Still-active block wins even if allowed.
    assert!(!reference_is_allowed(
        true,
        Some(BASE_NOW + 1),
        true,
        None,
        BASE_NOW
    ));
}
