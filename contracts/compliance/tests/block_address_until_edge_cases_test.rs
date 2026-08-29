//! Adversarial timestamp coverage for `block_address_until`, mirroring the
//! fuzz-style precedent set for `allow_address_until` (issue #58) for its
//! structurally symmetric `unblock_at` parameter.
//!
//! `block_address_until` sits on the emergency-remediation path that is
//! specifically permitted even while the contract is paused, so getting its
//! timestamp handling wrong under adversarial input is higher-stakes than an
//! equivalent bug on a normal, pause-gated entrypoint: there is no "just
//! unpause and fix it first" fallback if the block itself misbehaves.
//!
//! Cases covered:
//! - `unblock_at` in the past: does the block still apply and immediately
//!   read as expired, per `is_allowed`'s lazy-expiry evaluation?
//! - `unblock_at` exactly equal to the current ledger timestamp: the boundary
//!   case, distinct from "in the past".
//! - `unblock_at` at or near `u64::MAX`: must not overflow when combined with
//!   other timestamp arithmetic (the same class of concern that motivated
//!   `ArithmeticOverflow` handling elsewhere in this protocol).
//! - A sweep of adversarial deltas around "now", exercised in a loop in place
//!   of a full property-testing harness.

use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn setup() -> (Env, Address, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let subject = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, subject, client)
}

/// `unblock_at` strictly in the past: the block is still recorded (the call
/// does not error), but `is_allowed`'s lazy-expiry check means the block has
/// already "expired" by the time anyone reads it — the address behaves as
/// unblocked immediately, not merely eventually.
#[test]
fn unblock_at_in_the_past_expires_immediately() {
    let (env, admin, subject, client) = setup();
    env.ledger().set_timestamp(10_000);

    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));

    // unblock_at is 1 second before "now" - already lapsed at write time.
    client.block_address_until(&admin, &subject, &9_999, &None);

    assert!(client.is_blocked(&subject), "Blocked flag is still set");
    assert!(
        client.is_allowed(&subject),
        "a past unblock_at must read as already-expired, not as an active block"
    );
}

/// `unblock_at == now` is the boundary case: `is_allowed` treats the block as
/// expired once `now >= unblock_at`, so an exact match must behave the same
/// as "in the past", not as "still active for one more instant".
#[test]
fn unblock_at_exactly_equal_to_now_is_treated_as_expired() {
    let (env, admin, subject, client) = setup();
    env.ledger().set_timestamp(50_000);

    client.allow_address(&admin, &subject);
    client.block_address_until(&admin, &subject, &50_000, &None);

    assert!(client.is_blocked(&subject));
    assert!(
        client.is_allowed(&subject),
        "unblock_at equal to the current ledger timestamp must already read as expired"
    );
}

/// A block whose `unblock_at` is one second in the future is still active
/// right up to (but not including) that timestamp, and expires the instant
/// the ledger reaches it — pinning the other side of the boundary.
#[test]
fn unblock_at_one_second_in_future_is_active_until_that_instant() {
    let (env, admin, subject, client) = setup();
    env.ledger().set_timestamp(1_000);

    client.allow_address(&admin, &subject);
    client.block_address_until(&admin, &subject, &1_001, &None);
    assert!(
        !client.is_allowed(&subject),
        "block must still be active one second before unblock_at"
    );

    env.ledger().set_timestamp(1_001);
    assert!(
        client.is_allowed(&subject),
        "block must expire the instant the ledger reaches unblock_at"
    );
}

/// `unblock_at` set to `u64::MAX`: must be accepted and stored without
/// panicking, and must not overflow if later arithmetic (e.g. a future
/// cooldown/duration check) adds to it.
#[test]
fn unblock_at_at_u64_max_does_not_panic() {
    let (env, admin, subject, client) = setup();
    env.ledger().set_timestamp(1);

    client.allow_address(&admin, &subject);
    client.block_address_until(&admin, &subject, &u64::MAX, &None);

    assert!(client.is_blocked(&subject));
    assert!(
        !client.is_allowed(&subject),
        "a far-future unblock_at must keep the block active"
    );

    // Sanity: comparing against u64::MAX must not itself overflow.
    assert!(env.ledger().timestamp() < u64::MAX);
}

/// A sweep of adversarial deltas relative to "now" (large negative, zero,
/// small positive, and near-`u64::MAX` positive), checking `is_allowed`
/// agrees with the `now >= unblock_at` rule at every point.
#[test]
fn unblock_at_adversarial_delta_sweep() {
    let now: u64 = 1_000_000;
    let deltas: [i128; 7] = [
        -1_000_000, // far past (would underflow a naive u64 subtraction)
        -1,         // just past
        0,          // exact boundary
        1,          // just future
        1_000_000,  // moderate future
        (u64::MAX as i128) - (now as i128), // pushes unblock_at to exactly u64::MAX
        (u64::MAX as i128) - (now as i128) - 1,
    ];

    for delta in deltas {
        let (env, admin, subject, client) = setup();
        env.ledger().set_timestamp(now);
        client.allow_address(&admin, &subject);

        let unblock_at = (now as i128 + delta) as u64;
        client.block_address_until(&admin, &subject, &unblock_at, &None);

        let expired = now >= unblock_at;
        assert_eq!(
            client.is_allowed(&subject),
            expired,
            "delta={delta}, unblock_at={unblock_at}, now={now}: is_allowed must match now >= unblock_at"
        );
    }
}
