// Issue #902 — Automatic TTL bump logic for persistent storage keys.
// Every helper that reads or writes a persistent key calls bump_persistent_ttl
// so entries cannot be silently evicted from the ledger.

use soroban_sdk::Env;

use crate::DataKey;

/// Minimum number of ledgers remaining before a persistent entry must be
/// extended.  Below this threshold the bump fires.
pub const MIN_TTL_THRESHOLD: u32 = 100;

/// Number of ledgers to extend the TTL to on each bump.
/// 518_400 ledgers × 5 s/ledger ≈ 30 days.
pub const TTL_BUMP_AMOUNT: u32 = 518_400;

/// Extend the TTL of a persistent storage entry identified by `key`.
///
/// Should be called on every read or write of a persistent key so entries are
/// never silently evicted (issue #902).
pub fn bump_persistent_ttl(env: &Env, key: &DataKey) {
    env.storage()
        .persistent()
        .extend_ttl(key, MIN_TTL_THRESHOLD, TTL_BUMP_AMOUNT);
}

/// Bump the instance-storage TTL.  Called from `bump_state_ttl` and after
/// every state mutation that touches instance keys.
pub fn bump_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(MIN_TTL_THRESHOLD, TTL_BUMP_AMOUNT);
}
