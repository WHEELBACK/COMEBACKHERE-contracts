//! Automatic TTL extension for the workflow's storage (#875).
//!
//! Every key this contract keeps (`Admin`, `ComplianceId`, `TreasuryId`,
//! emergency-pause configuration, ...) lives in instance storage, which has
//! persistent durability and is archived together with the contract instance
//! once its TTL runs out. An evicted instance would leave the workflow
//! unusable until someone restores it, so every entrypoint that touches state
//! bumps the instance TTL, and `extend_ttl` lets anyone keep an idle workflow
//! alive.
//!
//! Values mirror the invoice contract's persistent-entry policy.

use soroban_sdk::Env;

/// Remaining-ledger threshold below which the instance TTL is extended
/// (~30 days at 5s/ledger).
pub const INSTANCE_TTL_THRESHOLD: u32 = 518_400;

/// Ledger count the instance TTL is extended to once bumped
/// (~60 days at 5s/ledger).
pub const INSTANCE_TTL_EXTEND_TO: u32 = 1_036_800;

/// Extends the contract instance (and therefore every instance-storage key)
/// if its TTL has dropped below [`INSTANCE_TTL_THRESHOLD`]. A no-op otherwise,
/// so calling it on every entrypoint does not rewrite the TTL each time.
pub fn extend_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_TTL_THRESHOLD, INSTANCE_TTL_EXTEND_TO);
}
