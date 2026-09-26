//! #86: storage key collision audit for the invoice contract.
//!
//! Soroban addresses a storage slot by the *XDR serialization* of the
//! `#[contracttype]` key value, not by its Rust name. Two variants that
//! serialize identically therefore share a slot, and the second write silently
//! overwrites the first — state corruption rather than a failed call. This
//! suite pins that no two variants of `invoice::DataKey` can collide.
//!
//! See `docs/STORAGE_VERSIONING.md` for the conventions and the change
//! procedure. When a variant is appended, add it to the exhaustive list in
//! `every_variant_serializes_uniquely` in the same change.

use invoice::DataKey;
use soroban_sdk::{testutils::Address as _, xdr::ToXdr, Address, Env};

/// The byte sequence Soroban uses to address the slot for `key`.
fn xdr(env: &Env, key: impl ToXdr) -> Vec<u8> {
    key.to_xdr(env).into_iter().collect()
}

/// Fails if any two entries serialize to the same bytes, naming both variants so
/// the collision is obvious from the failure message alone.
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

/// Every variant, with two distinct payload values for each parameterized one.
#[test]
fn every_variant_serializes_uniquely() {
    let env = Env::default();
    let addr_a = Address::generate(&env);
    let addr_b = Address::generate(&env);

    assert_all_unique(vec![
        ("Invoice(1)", xdr(&env, DataKey::Invoice(1))),
        ("Invoice(2)", xdr(&env, DataKey::Invoice(2))),
        ("InvoiceCount", xdr(&env, DataKey::InvoiceCount)),
        ("Admin", xdr(&env, DataKey::Admin)),
        ("PendingAdmin", xdr(&env, DataKey::PendingAdmin)),
        ("Paused", xdr(&env, DataKey::Paused)),
        ("GraceWindow", xdr(&env, DataKey::GraceWindow)),
        (
            "MerchantNonce(a, 1)",
            xdr(&env, DataKey::MerchantNonce(addr_a.clone(), 1)),
        ),
        (
            "MerchantNonce(b, 1)",
            xdr(&env, DataKey::MerchantNonce(addr_b.clone(), 1)),
        ),
        (
            "MerchantNonce(a, 2)",
            xdr(&env, DataKey::MerchantNonce(addr_a.clone(), 2)),
        ),
        (
            "MerchantInvoiceCount(a)",
            xdr(&env, DataKey::MerchantInvoiceCount(addr_a.clone())),
        ),
        (
            "MerchantInvoiceCount(b)",
            xdr(&env, DataKey::MerchantInvoiceCount(addr_b.clone())),
        ),
        (
            "MerchantInvoiceIndex(a, 0)",
            xdr(&env, DataKey::MerchantInvoiceIndex(addr_a.clone(), 0)),
        ),
        (
            "MerchantInvoiceIndex(a, 1)",
            xdr(&env, DataKey::MerchantInvoiceIndex(addr_a.clone(), 1)),
        ),
        (
            "MerchantInvoiceIndex(b, 0)",
            xdr(&env, DataKey::MerchantInvoiceIndex(addr_b.clone(), 0)),
        ),
        ("InvoiceHistory(1)", xdr(&env, DataKey::InvoiceHistory(1))),
        ("InvoiceHistory(2)", xdr(&env, DataKey::InvoiceHistory(2))),
        ("PendingIndex", xdr(&env, DataKey::PendingIndex)),
        ("CreationCooldown", xdr(&env, DataKey::CreationCooldown)),
        (
            "LastCreatedAt(a)",
            xdr(&env, DataKey::LastCreatedAt(addr_a.clone())),
        ),
        (
            "LastCreatedAt(b)",
            xdr(&env, DataKey::LastCreatedAt(addr_b.clone())),
        ),
        ("RefundBreakdown(1)", xdr(&env, DataKey::RefundBreakdown(1))),
        ("RefundBreakdown(2)", xdr(&env, DataKey::RefundBreakdown(2))),
    ]);
}

/// The `u64` payload alone must not be enough to make two keyed collections
/// collide: an invoice, its history log and its refund fee record (#71) all take
/// the same id, and each has to keep its own slot.
#[test]
fn same_id_keyed_collections_stay_separate() {
    let env = Env::default();
    assert_all_unique(vec![
        ("Invoice(7)", xdr(&env, DataKey::Invoice(7))),
        ("InvoiceHistory(7)", xdr(&env, DataKey::InvoiceHistory(7))),
        ("RefundBreakdown(7)", xdr(&env, DataKey::RefundBreakdown(7))),
    ]);
}

/// Unit variants are distinguished by name alone, so a new unit variant can
/// never alias an existing one.
#[test]
fn unit_variants_are_distinct() {
    let env = Env::default();
    assert_all_unique(vec![
        ("InvoiceCount", xdr(&env, DataKey::InvoiceCount)),
        ("Admin", xdr(&env, DataKey::Admin)),
        ("PendingAdmin", xdr(&env, DataKey::PendingAdmin)),
        ("Paused", xdr(&env, DataKey::Paused)),
        ("GraceWindow", xdr(&env, DataKey::GraceWindow)),
        ("PendingIndex", xdr(&env, DataKey::PendingIndex)),
        ("CreationCooldown", xdr(&env, DataKey::CreationCooldown)),
    ]);
}
