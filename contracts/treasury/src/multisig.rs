use soroban_sdk::{contracterror, contracttype, Address, Vec};

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
    Balance(Address),
    TokenAllowlist,
    RotationCount,
    SignerRotation(u64),
    MerchantPayoutAddress(Address),
    SignerList,
}
