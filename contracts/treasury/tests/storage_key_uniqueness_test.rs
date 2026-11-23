//! #86: storage key collision audit for the treasury contract.
//!
//! Treasury's key space is `multisig::DataKey`, re-exported as
//! `treasury::DataKey`. A collision here is the most expensive kind in the
//! protocol — `Balance`, `Settlement` and `Signer` all gate money movement — so
//! every variant is enumerated and asserted to serialize uniquely. See
//! `docs/STORAGE_VERSIONING.md`.

use soroban_sdk::{testutils::Address as _, xdr::ToXdr, Address, Env};
use treasury::DataKey;

/// Mock caller contract used to exercise treasury storage accessors through a
/// real cross-contract invocation (Soroban testutils host).
///
/// The caller holds no storage of its own; it simply forwards calls into the
/// treasury contract so the host records the nested invocation footprint.
#[soroban_sdk::contract]
pub struct StorageLayoutCaller;

#[soroban_sdk::contractimpl]
impl StorageLayoutCaller {
    /// Read a treasury storage key through a cross-contract call.
    pub fn read_key(env: Env, treasury: Address, key: DataKey) -> Option<u32> {
        let client = treasury::TreasuryClient::new(&env, &treasury);
        client.get_u32(&key)
    }

    /// Write a treasury storage key through a cross-contract call.
    pub fn write_key(env: Env, treasury: Address, key: DataKey, value: u32) {
        let client = treasury::TreasuryClient::new(&env, &treasury);
        client.set_u32(&key, &value);
    }

    /// Extend the TTL of a treasury storage key through a cross-contract call.
    pub fn bump_key(env: Env, treasury: Address, key: DataKey, extend_to: u32) {
        let client = treasury::TreasuryClient::new(&env, &treasury);
        client.bump_key_ttl(&key, &extend_to);
    }
}

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
    let holder = Address::generate(&env);
    let other_holder = Address::generate(&env);
    let token = Address::generate(&env);
    let other_token = Address::generate(&env);

    assert_all_unique(vec![
        ("Admin", xdr(&env, DataKey::Admin)),
        ("Threshold", xdr(&env, DataKey::Threshold)),
        ("SettlementCount", xdr(&env, DataKey::SettlementCount)),
        ("Settlement(1)", xdr(&env, DataKey::Settlement(1))),
        ("Settlement(2)", xdr(&env, DataKey::Settlement(2))),
        ("Signer(holder)", xdr(&env, DataKey::Signer(holder.clone()))),
        (
            "Signer(other)",
            xdr(&env, DataKey::Signer(other_holder.clone())),
        ),
        ("Paused", xdr(&env, DataKey::Paused)),
        ("DisputeCount", xdr(&env, DataKey::DisputeCount)),
        ("Dispute(1)", xdr(&env, DataKey::Dispute(1))),
        ("Dispute(2)", xdr(&env, DataKey::Dispute(2))),
        // Per-token balance segregation: the same holder under two tokens, and
        // two holders under one token, must be four distinct buckets (#448).
        (
            "Balance(holder, token)",
            xdr(&env, DataKey::Balance(holder.clone(), token.clone())),
        ),
        (
            "Balance(holder, other_token)",
            xdr(&env, DataKey::Balance(holder.clone(), other_token.clone())),
        ),
        (
            "Balance(other_holder, token)",
            xdr(&env, DataKey::Balance(other_holder.clone(), token.clone())),
        ),
        (
            "Balance(other_holder, other_token)",
            xdr(&env, DataKey::Balance(other_holder.clone(), other_token)),
        ),
        ("TokenAllowlist", xdr(&env, DataKey::TokenAllowlist)),
        ("RotationCount", xdr(&env, DataKey::RotationCount)),
        ("SignerRotation(1)", xdr(&env, DataKey::SignerRotation(1))),
        ("SignerRotation(2)", xdr(&env, DataKey::SignerRotation(2))),
        (
            "MerchantPayoutAddress(holder)",
            xdr(&env, DataKey::MerchantPayoutAddress(holder.clone())),
        ),
        (
            "MerchantPayoutAddress(other)",
            xdr(&env, DataKey::MerchantPayoutAddress(other_holder.clone())),
        ),
        ("SignerList", xdr(&env, DataKey::SignerList)),
        (
            "WithdrawalAllowlist",
            xdr(&env, DataKey::WithdrawalAllowlist),
        ),
        (
            "LastRotationProposal(holder)",
            xdr(&env, DataKey::LastRotationProposal(holder.clone())),
        ),
        (
            "LastRotationProposal(other)",
            xdr(&env, DataKey::LastRotationProposal(other_holder.clone())),
        ),
        (
            "PartialApprovedTotal(1)",
            xdr(&env, DataKey::PartialApprovedTotal(1)),
        ),
        (
            "PartialApprovedTotal(2)",
            xdr(&env, DataKey::PartialApprovedTotal(2)),
        ),
        (
            "WithdrawalLimitPerWindow",
            xdr(&env, DataKey::WithdrawalLimitPerWindow),
        ),
        (
            "WithdrawalWindowSecs",
            xdr(&env, DataKey::WithdrawalWindowSecs),
        ),
        (
            "WithdrawalWindowStart(holder)",
            xdr(&env, DataKey::WithdrawalWindowStart(holder.clone())),
        ),
        (
            "WithdrawalWindowStart(other)",
            xdr(&env, DataKey::WithdrawalWindowStart(other_holder.clone())),
        ),
        (
            "WithdrawnInWindow(holder)",
            xdr(&env, DataKey::WithdrawnInWindow(holder.clone())),
        ),
        (
            "WithdrawnInWindow(other)",
            xdr(&env, DataKey::WithdrawnInWindow(other_holder)),
        ),
        ("SignerChangeCount", xdr(&env, DataKey::SignerChangeCount)),
        ("SignerChange(1)", xdr(&env, DataKey::SignerChange(1))),
        ("SignerChange(2)", xdr(&env, DataKey::SignerChange(2))),
    ]);
}

/// The three id-keyed collections must not share a slot just because they take
/// the same numeric id.
#[test]
fn same_id_keyed_collections_stay_separate() {
    let env = Env::default();
    assert_all_unique(vec![
        ("Settlement(3)", xdr(&env, DataKey::Settlement(3))),
        ("Dispute(3)", xdr(&env, DataKey::Dispute(3))),
        ("SignerRotation(3)", xdr(&env, DataKey::SignerRotation(3))),
        ("SignerChange(3)", xdr(&env, DataKey::SignerChange(3))),
        (
            "PartialApprovedTotal(3)",
            xdr(&env, DataKey::PartialApprovedTotal(3)),
        ),
    ]);
}

/// Per-holder window bookkeeping and the payout-address override are keyed by
/// the same address type; a mix-up would cap or redirect the wrong account.
#[test]
fn per_holder_keys_stay_separate() {
    let env = Env::default();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    assert_all_unique(vec![
        ("Signer(a)", xdr(&env, DataKey::Signer(a.clone()))),
        (
            "Balance(a, a)",
            xdr(&env, DataKey::Balance(a.clone(), a.clone())),
        ),
        (
            "MerchantPayoutAddress(a)",
            xdr(&env, DataKey::MerchantPayoutAddress(a.clone())),
        ),
        (
            "LastRotationProposal(a)",
            xdr(&env, DataKey::LastRotationProposal(a.clone())),
        ),
        (
            "WithdrawalWindowStart(a)",
            xdr(&env, DataKey::WithdrawalWindowStart(a.clone())),
        ),
        (
            "WithdrawnInWindow(a)",
            xdr(&env, DataKey::WithdrawnInWindow(a)),
        ),
        ("Signer(b)", xdr(&env, DataKey::Signer(b.clone()))),
        (
            "Balance(b, b)",
            xdr(&env, DataKey::Balance(b.clone(), b.clone())),
        ),
        (
            "MerchantPayoutAddress(b)",
            xdr(&env, DataKey::MerchantPayoutAddress(b.clone())),
        ),
        (
            "LastRotationProposal(b)",
            xdr(&env, DataKey::LastRotationProposal(b.clone())),
        ),
        (
            "WithdrawalWindowStart(b)",
            xdr(&env, DataKey::WithdrawalWindowStart(b.clone())),
        ),
        (
            "WithdrawnInWindow(b)",
            xdr(&env, DataKey::WithdrawnInWindow(b)),
        ),
    ]);
}

/// Cross-contract invocation: a mock caller writes and reads treasury storage
/// keys, and the resulting key space must stay collision-free.
#[test]
fn cross_contract_storage_roundtrip_preserves_key_uniqueness() {
    let env = Env::default();
    env.mock_all_auths();

    let treasury_id = env.register_contract(None, treasury::Treasury);
    let caller_id = env.register_contract(None, StorageLayoutCaller);
    let caller = StorageLayoutCallerClient::new(&env, &caller_id);

    let holder = Address::generate(&env);
    let token = Address::generate(&env);

    let keys = [
        DataKey::SettlementCount,
        DataKey::DisputeCount,
        DataKey::RotationCount,
        DataKey::SignerChangeCount,
        DataKey::Settlement(1),
        DataKey::Dispute(1),
        DataKey::SignerRotation(1),
        DataKey::SignerChange(1),
        DataKey::PartialApprovedTotal(1),
        DataKey::Balance(holder.clone(), token.clone()),
        DataKey::Signer(holder.clone()),
        DataKey::MerchantPayoutAddress(holder.clone()),
        DataKey::LastRotationProposal(holder.clone()),
        DataKey::WithdrawalWindowStart(holder.clone()),
        DataKey::WithdrawnInWindow(holder.clone()),
    ];

    // Each key is written through the caller with a distinct value, so a
    // collision would surface as a clobbered read below.
    for (index, key) in keys.iter().enumerate() {
        let value = (index as u32) + 1;
        caller.write_key(&treasury_id, key, &value);
        assert_eq!(
            caller.read_key(&treasury_id, key),
            Some(value),
            "cross-contract write/read mismatch for {key:?}"
        );
    }

    // Re-read everything: no later write may have overwritten an earlier key.
    for (index, key) in keys.iter().enumerate() {
        assert_eq!(
            caller.read_key(&treasury_id, key),
            Some((index as u32) + 1),
            "storage collision detected for {key:?} after cross-contract writes"
        );
    }
}

/// Cross-contract TTL extension must succeed and leave the value intact.
#[test]
fn cross_contract_ttl_extension_keeps_value() {
    let env = Env::default();
    env.mock_all_auths();

    let treasury_id = env.register_contract(None, treasury::Treasury);
    let caller_id = env.register_contract(None, StorageLayoutCaller);
    let caller = StorageLayoutCallerClient::new(&env, &caller_id);

    let key = DataKey::SettlementCount;
    caller.write_key(&treasury_id, &key, &7);
    caller.bump_key(&treasury_id, &key, &10_000);

    assert_eq!(caller.read_key(&treasury_id, &key), Some(7));
}

/// Unit variants are distinguished by name alone.
#[test]
fn unit_variants_are_distinct() {
    let env = Env::default();
    assert_all_unique(vec![
        ("Admin", xdr(&env, DataKey::Admin)),
        ("Threshold", xdr(&env, DataKey::Threshold)),
        ("SettlementCount", xdr(&env, DataKey::SettlementCount)),
        ("DisputeCount", xdr(&env, DataKey::DisputeCount)),
        ("RotationCount", xdr(&env, DataKey::RotationCount)),
        ("SignerChangeCount", xdr(&env, DataKey::SignerChangeCount)),
        ("Paused", xdr(&env, DataKey::Paused)),
        ("TokenAllowlist", xdr(&env, DataKey::TokenAllowlist)),
        ("SignerList", xdr(&env, DataKey::SignerList)),
        (
            "WithdrawalAllowlist",
            xdr(&env, DataKey::WithdrawalAllowlist),
        ),
        (
            "WithdrawalLimitPerWindow",
            xdr(&env, DataKey::WithdrawalLimitPerWindow),
        ),
        (
            "WithdrawalWindowSecs",
            xdr(&env, DataKey::WithdrawalWindowSecs),
        ),
    ]);
}
