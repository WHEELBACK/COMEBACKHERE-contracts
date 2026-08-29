#![no_std]
use soroban_sdk::{contracterror, contracttype, Address, Env, Vec};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TreasuryError {
    AlreadyInitialized = 1,
    ZeroThreshold = 2,
    SettlementNotFound = 3,
    AlreadyExecuted = 4,
    ThresholdNotMet = 5,
    ThresholdNotConfigured = 6,
    InvalidAmount = 7,
    ContractPaused = 8,
    Unauthorized = 9,
    UnauthorizedSigner = 10,
    InvalidTokenContract = 11,
    TokenNotAllowed = 12,
    RotationNotFound = 13,
    RotationAlreadyExecuted = 14,
    SettlementOnHold = 15,
    DisputeNotExpired = 16,
    AlreadyOnHold = 17,
    ThresholdUnreachable = 18,
    ComplianceCheckFailed = 19,
    // Appended (not renumbered) to keep discriminants stable for existing
    // on-chain state; see scripts/check-enum-ordering.sh (#74).
    ArithmeticOverflow = 20,
    DisputeNotFound = 21,
    DisputeAlreadyResolved = 22,
    ResolutionDirectionMismatch = 23,
    BatchTooLarge = 24,
    WeightOverflow = 25,
    SettlementNotCancellable = 26,
    TtlNotElapsed = 27,
    AllowlistFull = 28,
    NotOnHold = 29,
    DestinationNotAllowed = 30,
    InsufficientBalance = 31,
    NotPaused = 32,
    RotationProposalCooldown = 33,
}

// Issue #48: reason codes attached to a held settlement; None means not on hold
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementHoldReason {
    None,
    ComplianceReview,
    FraudCheck,
    KycPending,
    AdminHold,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementStatus {
    Pending,
    Executed,
    PartiallySettled,
    PartiallyExecuted,
    OnHold,
    Cancelled,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeStatus {
    Raised,
    ResolvedClaimant,
    ResolvedCounterparty,
    Expired,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Settlement {
    pub id: u64,
    pub merchant_address: Address,
    pub amount: i128,
    pub approvals: Vec<Address>,
    pub approval_weight: u32,
    pub status: SettlementStatus,
    pub hold_reason: SettlementHoldReason,
    pub proposed_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispute {
    pub id: u64,
    pub settlement_id: u64,
    pub claimant: Address,
    pub counterparty: Address,
    pub amount: i128,
    pub status: DisputeStatus,
    pub resolution_approvals: Vec<Address>,
    pub resolution_weight: u32,
    pub resolution_for_claimant: bool,
    pub dispute_expires_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RotationStatus {
    Pending,
    Executed,
    Cancelled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignerRotationProposal {
    pub id: u64,
    pub old_signer: Address,
    pub new_signer: Address,
    pub approvals: Vec<Address>,
    pub approval_weight: u32,
    pub status: RotationStatus,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Threshold,
    SettlementCount,
    Settlement(u64),
    Signer(Address),
    Paused,
    DisputeCount,
    Dispute(u64),
    /// Deposit balance for (holder, token_contract), segregated per token so
    /// concurrently-allowlisted tokens never share an accounting bucket (#448).
    Balance(Address, Address),
    TokenAllowlist,
    RotationCount,
    SignerRotation(u64),
    MerchantPayoutAddress(Address),
    SignerList,
    WithdrawalAllowlist,
    LastRotationProposal(Address),
    PartialApprovedTotal(u64),
}

/// Returns the approval weight assigned to `signer`, or `0` if not registered.
pub fn signer_weight(env: &Env, signer: &Address) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::Signer(signer.clone()))
        .unwrap_or(0)
}

/// Requires `signer` to authenticate and have a non-zero weight in the signer registry.
/// Panics: `UnauthorizedSigner`.
pub fn require_authorized_signer(env: &Env, signer: &Address) {
    signer.require_auth();
    if signer_weight(env, signer) == 0 {
        soroban_sdk::panic_with_error!(env, TreasuryError::UnauthorizedSigner);
    }
}

/// Adds `signer`'s weight to `weight` and appends `signer` to `approvals`, unless `signer` has
/// already approved (in which case this is a no-op). Captures the dedup-then-accumulate pattern
/// used for settlement, dispute, and rotation approvals.
pub fn record_approval(
    env: &Env,
    approvals: &mut Vec<Address>,
    weight: &mut u32,
    signer: &Address,
) {
    if !approvals.contains(signer) {
        *weight = weight
            .checked_add(signer_weight(env, signer))
            .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, TreasuryError::WeightOverflow));
        approvals.push_back(signer.clone());
    }
}

/// Returns whether `weight` satisfies simple weighted-threshold quorum, i.e. `weight >= threshold`.
pub fn meets_threshold(weight: u32, threshold: u32) -> bool {
    weight >= threshold
}
