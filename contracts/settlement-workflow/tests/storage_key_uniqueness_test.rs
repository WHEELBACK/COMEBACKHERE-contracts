//! #86: storage key collision audit for the settlement workflow (orchestrator).
//!
//! The orchestrator's key space is small but load-bearing: `ComplianceId` and
//! `TreasuryId` pin which instances the compliance gate trusts, so a collision
//! there would route settlements through an unpinned contract. See
//! `docs/STORAGE_VERSIONING.md`.

use settlement_workflow::DataKey;
use soroban_sdk::{xdr::ToXdr, Env};

fn xdr(env: &Env, key: impl ToXdr) -> Vec<u8> {
    key.to_xdr(env).into_iter().collect()
}

fn assert_all_unique(entries: Vec<(&'static str, Vec<u8>)>) {
    assert!(!entries.is_empty(), "key list must not be empty");
    for (index, (name, bytes)) in entries.iter().enumerate() {
        for (other_name, other_bytes) in &entries[..index] {
            assert_ne!(
                bytes, other_bytes,
                "DataKey::{name} and DataKey::{other_name} serialize identically \
                 ({bytes:?}): they would share one storage slot"
            );
        }
    }
}

#[test]
fn every_variant_serializes_uniquely() {
    let env = Env::default();
    assert_all_unique(vec![
        (
            "ExecutedSettlements",
            xdr(&env, DataKey::ExecutedSettlements),
        ),
        ("ComplianceId", xdr(&env, DataKey::ComplianceId)),
        ("TreasuryId", xdr(&env, DataKey::TreasuryId)),
    ]);
}
