//! #86: storage key collision audit for the compliance contract.
//!
//! Compliance keeps eight per-address flag keys (`Allowed`, `Blocked`,
//! `AllowedUntil`, `BlockedUntil`, `BlockReason`, `Tier`, `Jurisdiction`,
//! `LastBulkAllow`, `LastBulkBlock`, `AddrTracked`). `is_allowed` reads a
//! combination of them, so a collision between any two would answer an
//! allow/block question from the wrong record. See
//! `docs/STORAGE_VERSIONING.md`.

use compliance::DataKey;
use soroban_sdk::{testutils::Address as _, xdr::ToXdr, Address, Env};

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
    let addr = Address::generate(&env);
    let other = Address::generate(&env);

    assert_all_unique(vec![
        ("Admin", xdr(&env, DataKey::Admin)),
        ("PendingAdmin", xdr(&env, DataKey::PendingAdmin)),
        ("Operator", xdr(&env, DataKey::Operator)),
        ("Allowed(addr)", xdr(&env, DataKey::Allowed(addr.clone()))),
        ("Allowed(other)", xdr(&env, DataKey::Allowed(other.clone()))),
        ("Blocked(addr)", xdr(&env, DataKey::Blocked(addr.clone()))),
        ("Blocked(other)", xdr(&env, DataKey::Blocked(other.clone()))),
        (
            "AllowedUntil(addr)",
            xdr(&env, DataKey::AllowedUntil(addr.clone())),
        ),
        (
            "AllowedUntil(other)",
            xdr(&env, DataKey::AllowedUntil(other.clone())),
        ),
        (
            "BlockedUntil(addr)",
            xdr(&env, DataKey::BlockedUntil(addr.clone())),
        ),
        (
            "BlockedUntil(other)",
            xdr(&env, DataKey::BlockedUntil(other.clone())),
        ),
        (
            "BlockReason(addr)",
            xdr(&env, DataKey::BlockReason(addr.clone())),
        ),
        (
            "BlockReason(other)",
            xdr(&env, DataKey::BlockReason(other.clone())),
        ),
        ("SchemaVersion", xdr(&env, DataKey::SchemaVersion)),
        ("Paused", xdr(&env, DataKey::Paused)),
        // Legacy index, retained for append-only reasons and read by nothing.
        ("AddressIndex", xdr(&env, DataKey::AddressIndex)),
        ("AllowCount", xdr(&env, DataKey::AllowCount)),
        ("BlockCount", xdr(&env, DataKey::BlockCount)),
        ("Tier(addr)", xdr(&env, DataKey::Tier(addr.clone()))),
        ("Tier(other)", xdr(&env, DataKey::Tier(other.clone()))),
        (
            "Jurisdiction(addr)",
            xdr(&env, DataKey::Jurisdiction(addr.clone())),
        ),
        (
            "Jurisdiction(other)",
            xdr(&env, DataKey::Jurisdiction(other.clone())),
        ),
        (
            "LastBulkAllow(addr)",
            xdr(&env, DataKey::LastBulkAllow(addr.clone())),
        ),
        (
            "LastBulkAllow(other)",
            xdr(&env, DataKey::LastBulkAllow(other.clone())),
        ),
        (
            "LastBulkBlock(addr)",
            xdr(&env, DataKey::LastBulkBlock(addr.clone())),
        ),
        (
            "LastBulkBlock(other)",
            xdr(&env, DataKey::LastBulkBlock(other.clone())),
        ),
        (
            "AddrTracked(addr)",
            xdr(&env, DataKey::AddrTracked(addr.clone())),
        ),
        (
            "AddrTracked(other)",
            xdr(&env, DataKey::AddrTracked(other.clone())),
        ),
        ("AddrIndexPage(0)", xdr(&env, DataKey::AddrIndexPage(0))),
        ("AddrIndexPage(1)", xdr(&env, DataKey::AddrIndexPage(1))),
        ("AddrIndexCount", xdr(&env, DataKey::AddrIndexCount)),
    ]);
}

/// `AddrIndexPage` is keyed by a `u32` page number; page 0 and page 1 must not
/// alias, or the index silently loses every address past the first page.
#[test]
fn address_index_pages_stay_separate() {
    let env = Env::default();
    assert_all_unique(vec![
        ("AddrIndexPage(0)", xdr(&env, DataKey::AddrIndexPage(0))),
        ("AddrIndexPage(1)", xdr(&env, DataKey::AddrIndexPage(1))),
        (
            "AddrIndexPage(u32::MAX)",
            xdr(&env, DataKey::AddrIndexPage(u32::MAX)),
        ),
    ]);
}

/// Unit variants are distinguished by name alone, so a new unit variant can
/// never alias an existing one.
#[test]
fn unit_variants_are_distinct() {
    let env = Env::default();
    assert_all_unique(vec![
        ("Admin", xdr(&env, DataKey::Admin)),
        ("PendingAdmin", xdr(&env, DataKey::PendingAdmin)),
        ("Operator", xdr(&env, DataKey::Operator)),
        ("SchemaVersion", xdr(&env, DataKey::SchemaVersion)),
        ("Paused", xdr(&env, DataKey::Paused)),
        ("AddressIndex", xdr(&env, DataKey::AddressIndex)),
        ("AllowCount", xdr(&env, DataKey::AllowCount)),
        ("BlockCount", xdr(&env, DataKey::BlockCount)),
        ("AddrIndexCount", xdr(&env, DataKey::AddrIndexCount)),
    ]);
}
