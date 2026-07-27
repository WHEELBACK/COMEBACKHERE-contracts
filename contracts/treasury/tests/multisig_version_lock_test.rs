//! Guards against silent ABI drift between `crates/multisig` and `contracts/treasury`.
//!
//! `treasury`'s public interface re-exports multisig's contract types directly
//! (see `pub use multisig::{...}` in `contracts/treasury/src/lib.rs`), and several
//! contract functions (`get_settlement`, `hold_settlement`, `get_dispute`,
//! `cancel_rotation`, ...) return or accept those types as-is. A multisig crate
//! bump that adds, removes, renames, or reorders a field/variant is a semver-
//! compatible Rust change but can silently change the treasury ABI without any
//! compile error - exactly the risk in issue #68.
//!
//! This file pins the expected multisig version and exhaustively matches every
//! field/variant of the multisig types exposed through treasury's ABI. If any of
//! those shapes change, this file fails to *compile* (not just fails a test),
//! forcing a deliberate review here and a regenerated treasury ABI snapshot
//! (see README's "ABI Snapshots" section) before `EXPECTED_MULTISIG_VERSION` is
//! bumped to match.

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use treasury::{
    Dispute, DisputeStatus, RotationStatus, Settlement, SettlementHoldReason, SettlementStatus,
    SignerRotationProposal, TreasuryContract, TreasuryContractClient, TreasuryError,
};

/// Expected version of `crates/multisig` (see its `Cargo.toml`). Bump this only
/// alongside a review of every exhaustive match below - if they still compile,
/// the ABI-relevant shape of multisig's types is unchanged.
const EXPECTED_MULTISIG_VERSION: &str = "0.1.0";

const MULTISIG_CARGO_TOML: &str = include_str!("../../../crates/multisig/Cargo.toml");

fn parse_version(manifest: &str) -> &str {
    manifest
        .lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("version")?;
            let rest = rest.trim_start().strip_prefix('=')?;
            let rest = rest.trim_start().strip_prefix('"')?;
            rest.split('"').next()
        })
        .expect("crates/multisig/Cargo.toml must declare a [package] version")
}

fn setup(env: &Env) -> (TreasuryContractClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &Vec::new(env));
    (client, admin)
}

#[test]
fn multisig_crate_version_is_pinned() {
    let actual = parse_version(MULTISIG_CARGO_TOML);
    assert_eq!(
        actual, EXPECTED_MULTISIG_VERSION,
        "crates/multisig version changed from {EXPECTED_MULTISIG_VERSION} to {actual}, but \
         treasury re-exports multisig's contract types directly through its public ABI. \
         Regenerate the treasury ABI snapshot (README's \"ABI Snapshots\" section), confirm \
         every exhaustive match in multisig_version_lock_test.rs still compiles unchanged, \
         then bump EXPECTED_MULTISIG_VERSION here to match."
    );
}

/// `TreasuryError` discriminants are part of the on-chain ABI (clients match on the
/// numeric code), so both the value and the variant set are pinned.
#[test]
fn treasury_error_shape_is_unchanged() {
    assert_eq!(TreasuryError::AlreadyInitialized as u32, 1);
    assert_eq!(TreasuryError::ZeroThreshold as u32, 2);
    assert_eq!(TreasuryError::SettlementNotFound as u32, 3);
    assert_eq!(TreasuryError::AlreadyExecuted as u32, 4);
    assert_eq!(TreasuryError::ThresholdNotMet as u32, 5);
    assert_eq!(TreasuryError::ThresholdNotConfigured as u32, 6);
    assert_eq!(TreasuryError::InvalidAmount as u32, 7);
    assert_eq!(TreasuryError::ContractPaused as u32, 8);
    assert_eq!(TreasuryError::Unauthorized as u32, 9);
    assert_eq!(TreasuryError::UnauthorizedSigner as u32, 10);
    assert_eq!(TreasuryError::InvalidTokenContract as u32, 11);
    assert_eq!(TreasuryError::TokenNotAllowed as u32, 12);
    assert_eq!(TreasuryError::RotationNotFound as u32, 13);
    assert_eq!(TreasuryError::RotationAlreadyExecuted as u32, 14);
    assert_eq!(TreasuryError::SettlementOnHold as u32, 15);
    assert_eq!(TreasuryError::DisputeNotExpired as u32, 16);
    assert_eq!(TreasuryError::AlreadyOnHold as u32, 17);

    // No wildcard arm: adding, removing, or renaming a variant fails this compile.
    fn assert_exhaustive(err: TreasuryError) {
        match err {
            TreasuryError::AlreadyInitialized
            | TreasuryError::ZeroThreshold
            | TreasuryError::SettlementNotFound
            | TreasuryError::AlreadyExecuted
            | TreasuryError::ThresholdNotMet
            | TreasuryError::ThresholdNotConfigured
            | TreasuryError::InvalidAmount
            | TreasuryError::ContractPaused
            | TreasuryError::Unauthorized
            | TreasuryError::UnauthorizedSigner
            | TreasuryError::InvalidTokenContract
            | TreasuryError::TokenNotAllowed
            | TreasuryError::RotationNotFound
            | TreasuryError::RotationAlreadyExecuted
            | TreasuryError::SettlementOnHold
            | TreasuryError::DisputeNotExpired
            | TreasuryError::AlreadyOnHold => {}
        }
    }
    assert_exhaustive(TreasuryError::AlreadyOnHold);
}

fn assert_settlement_status_exhaustive(status: SettlementStatus) {
    match status {
        SettlementStatus::Pending => {}
        SettlementStatus::Executed => {}
        SettlementStatus::PartiallySettled => {}
        SettlementStatus::PartiallyExecuted => {}
        SettlementStatus::OnHold => {}
        SettlementStatus::Cancelled => {}
        SettlementStatus::Expired => {}
    }
}

fn assert_hold_reason_exhaustive(reason: SettlementHoldReason) {
    match reason {
        SettlementHoldReason::None => {}
        SettlementHoldReason::ComplianceReview => {}
        SettlementHoldReason::FraudCheck => {}
        SettlementHoldReason::KycPending => {}
        SettlementHoldReason::AdminHold => {}
    }
}

fn assert_dispute_status_exhaustive(status: DisputeStatus) {
    match status {
        DisputeStatus::Raised => {}
        DisputeStatus::ResolvedClaimant => {}
        DisputeStatus::ResolvedCounterparty => {}
        DisputeStatus::Expired => {}
    }
}

fn assert_rotation_status_exhaustive(status: RotationStatus) {
    match status {
        RotationStatus::Pending => {}
        RotationStatus::Executed => {}
        RotationStatus::Cancelled => {}
    }
}

/// Exhaustively covers every `SettlementHoldReason` and `SettlementStatus` variant,
/// independent of which ones a single settlement lifecycle happens to reach.
#[test]
fn settlement_enum_variants_are_unchanged() {
    for status in [
        SettlementStatus::Pending,
        SettlementStatus::Executed,
        SettlementStatus::PartiallySettled,
        SettlementStatus::PartiallyExecuted,
        SettlementStatus::OnHold,
        SettlementStatus::Cancelled,
        SettlementStatus::Expired,
    ] {
        assert_settlement_status_exhaustive(status);
    }
    for reason in [
        SettlementHoldReason::None,
        SettlementHoldReason::ComplianceReview,
        SettlementHoldReason::FraudCheck,
        SettlementHoldReason::KycPending,
        SettlementHoldReason::AdminHold,
    ] {
        assert_hold_reason_exhaustive(reason);
    }
}

#[test]
fn dispute_enum_variants_are_unchanged() {
    for status in [
        DisputeStatus::Raised,
        DisputeStatus::ResolvedClaimant,
        DisputeStatus::ResolvedCounterparty,
        DisputeStatus::Expired,
    ] {
        assert_dispute_status_exhaustive(status);
    }
}

#[test]
fn rotation_enum_variants_are_unchanged() {
    for status in [
        RotationStatus::Pending,
        RotationStatus::Executed,
        RotationStatus::Cancelled,
    ] {
        assert_rotation_status_exhaustive(status);
    }
}

/// Builds a real `Settlement` through the deployed contract and destructures it with
/// no `..` - a field added, removed, or renamed in multisig's `Settlement` struct
/// fails this compile.
#[test]
fn settlement_struct_shape_is_unchanged() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    let settlement: Settlement = client.approve_settlement(&admin, &sid);

    let Settlement {
        id,
        merchant_address,
        amount,
        approvals,
        approval_weight,
        status,
        hold_reason,
        proposed_at,
    } = settlement;

    assert_eq!(id, sid);
    assert_eq!(merchant_address, merchant);
    assert_eq!(amount, 10_000_000);
    assert!(approvals.contains(&admin));
    assert!(approval_weight > 0);
    assert_settlement_status_exhaustive(status);
    assert_hold_reason_exhaustive(hold_reason);
    assert_eq!(proposed_at, env.ledger().timestamp());
}

/// Builds a real `Dispute` through the deployed contract and destructures it with no
/// `..` - a field added, removed, or renamed in multisig's `Dispute` struct fails
/// this compile.
#[test]
fn dispute_struct_shape_is_unchanged() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let counterparty = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    let did = client.raise_dispute(&admin, &sid, &counterparty, &5_000_000, &1_000);
    let dispute: Dispute = client.get_dispute(&did);

    let Dispute {
        id,
        settlement_id,
        claimant,
        counterparty: dispute_counterparty,
        amount,
        status,
        resolution_approvals,
        resolution_weight,
        resolution_for_claimant,
        dispute_expires_at,
    } = dispute;

    assert_eq!(id, did);
    assert_eq!(settlement_id, sid);
    assert_eq!(claimant, admin);
    assert_eq!(dispute_counterparty, counterparty);
    assert_eq!(amount, 5_000_000);
    assert_dispute_status_exhaustive(status);
    assert_eq!(resolution_approvals.len(), 0);
    assert_eq!(resolution_weight, 0);
    assert!(!resolution_for_claimant);
    assert_eq!(dispute_expires_at, 1_000);
}

/// Builds a real `SignerRotationProposal` through the deployed contract and
/// destructures it with no `..` - a field added, removed, or renamed in multisig's
/// `SignerRotationProposal` struct fails this compile.
#[test]
fn signer_rotation_proposal_struct_shape_is_unchanged() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let new_signer = Address::generate(&env);

    let rid = client.propose_signer_rotation(&admin, &admin, &new_signer);
    let proposal: SignerRotationProposal = client.cancel_rotation(&admin, &rid);

    let SignerRotationProposal {
        id,
        old_signer,
        new_signer: proposal_new_signer,
        approvals,
        approval_weight,
        status,
    } = proposal;

    assert_eq!(id, rid);
    assert_eq!(old_signer, admin);
    assert_eq!(proposal_new_signer, new_signer);
    assert!(approvals.contains(&admin));
    assert!(approval_weight > 0);
    assert_rotation_status_exhaustive(status);
}
