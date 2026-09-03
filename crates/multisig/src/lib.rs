#![no_std]
use soroban_sdk::{contracterror, contracttype, Address, Env, Vec};

/// Error codes for all treasury contract operations. Variants are append-only
/// and must never be renumbered, as discriminants are stored on-chain and
/// matched by off-chain systems; see `scripts/check-enum-ordering.sh`.
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
    // Settlement-workflow precondition: the workflow contract must be registered
    // as a treasury signer (see settlement-workflow #370). Without this the nested
    // `execute_settlement` would fail with the generic `UnauthorizedSigner`, which
    // gives a first-time deployer no hint that the fix is a `set_signer` call for
    // the workflow's own address.
    WorkflowNotRegisteredSigner = 34,
    // Per-window withdrawal limit was exceeded; see `set_withdrawal_limit` and
    // `enforce_withdrawal_limit` in `deposits.rs` (#455).
    WithdrawalLimitExceeded = 35,
    // Appended for `resolve_dispute_split` (#456): the provided claimant basis-points
    // ratio exceeds BPS_DENOMINATOR (10_000), making a valid split impossible.
    InvalidSplitRatio = 36,
    // Appended for `force_cancel_settlement`: the target settlement is already in a
    // terminal state (Executed, Cancelled, Expired) and cannot be force-cancelled.
    ForceCancelNotAllowed = 37,
    // Appended for #447: a timelocked signer/threshold change cannot be executed
    // before its minimum delay has elapsed.
    SignerChangeTooEarly = 38,
    // Appended for #447: no pending signer/threshold change exists with the given id.
    SignerChangeNotFound = 39,
    // Appended for #447: the referenced signer/threshold change has already been
    // executed or cancelled and cannot be acted on again.
    SignerChangeAlreadyFinalised = 40,
}

// Issue #48: reason codes attached to a held settlement; None means not on hold
/// Reason codes attached to a settlement that is currently on hold.
///
/// Attached to a `Settlement` when `hold_settlement` is called. `None` is the
/// default and means the settlement is not on hold. Other variants express the
/// semantic reason for the hold so downstream systems (compliance dashboards,
/// support tooling) can route the case appropriately.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SettlementHoldReason {
    None,
    ComplianceReview,
    FraudCheck,
    KycPending,
    AdminHold,
}

/// Lifecycle state of a treasury settlement proposal.
///
/// Transitions: `Pending` → `Executed` (threshold met), `Pending` →
/// `PartiallySettled` / `PartiallyExecuted` (partial flow), `Pending` →
/// `OnHold` (held by admin), `Pending` → `Cancelled` (admin cancel or force-
/// cancel), `Pending` → `Expired` (TTL elapsed without execution).
/// Terminal states are `Executed`, `Cancelled`, and `Expired`.
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

/// Lifecycle state of a raised dispute on a treasury settlement.
///
/// A dispute starts in `Raised` and transitions to one of the three resolved
/// states (`ResolvedClaimant`, `ResolvedCounterparty`, `ResolvedSplit`) once
/// enough resolution approvals have accumulated, or to `Expired` if the
/// `dispute_expires_at` timestamp has passed without a resolution.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DisputeStatus {
    Raised,
    ResolvedClaimant,
    ResolvedCounterparty,
    Expired,
    /// The disputed amount was split between claimant and counterparty; see
    /// `Dispute::claimant_share_bps` for the ratio and `resolve_dispute_split` (#456).
    ResolvedSplit,
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
    /// Claimant's share of `amount` in basis points (0..=10_000), set when `status` is
    /// `ResolvedSplit`; meaningless (always 0) for every other status. See #456.
    pub claimant_share_bps: u32,
}

/// Lifecycle state of a signer-rotation proposal.
///
/// A rotation starts `Pending` when proposed, transitions to `Executed` when
/// cumulative approval weight meets the threshold, or to `Cancelled` when an
/// admin explicitly cancels it. Both `Executed` and `Cancelled` are terminal.
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
    /// `old_signer`'s approval weight captured at the moment this rotation was
    /// *proposed*, not re-read at execution time. This is the weight that gets
    /// assigned to `new_signer` when the rotation executes.
    ///
    /// Without this snapshot, a separate `set_signer`/`remove_signer` call that
    /// lands between the proposal and its execution would change what weight
    /// `new_signer` ends up with — a time-of-check-to-time-of-use gap where the
    /// outcome of the rotation depends on unrelated transactions racing it.
    /// Pinning the weight at proposal time makes the rotation's effect fully
    /// determined by its own proposal, independent of what else happens to
    /// `old_signer` in the meantime.
    pub captured_old_weight: u32,
}

/// The kind of admin signer/threshold change captured in a `SignerChangeProposal`.
/// Each variant carries the parameters required to execute that specific change once
/// the timelock delay has elapsed.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignerChangeKind {
    /// Set (or update) a signer's approval weight.  Weight `0` deactivates the signer.
    SetSigner(Address, u32),
    /// Remove a signer from the active registry.
    RemoveSigner(Address),
    /// Change the multisig approval threshold.
    UpdateThreshold(u32),
}

/// Lifecycle state of a `SignerChangeProposal`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SignerChangeStatus {
    /// Queued but the delay has not yet elapsed; cannot be executed yet.
    Pending,
    /// The change has been applied; no further state transitions are possible.
    Executed,
    /// An admin cancelled the change before it was executed; permanently terminal.
    Cancelled,
}

/// A timelocked admin signer/threshold-configuration change.
///
/// Proposed via `propose_signer_change` and executed via `execute_signer_change`
/// only after `SIGNER_CHANGE_TIMELOCK_SECS` has elapsed since `proposed_at`.
/// Any admin may cancel it via `cancel_signer_change` while it is still `Pending`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignerChangeProposal {
    /// Monotonically-increasing unique identifier.
    pub id: u64,
    /// What signer-configuration change will be applied on execution.
    pub kind: SignerChangeKind,
    /// Ledger timestamp at which the proposal was created.
    pub proposed_at: u64,
    /// Earliest ledger timestamp at which `execute_signer_change` may succeed.
    pub executable_at: u64,
    /// Current lifecycle state.
    pub status: SignerChangeStatus,
}

/// Storage keys for all treasury contract state.
///
/// Used as keys for Soroban instance and persistent storage. Variants must not
/// be reordered or removed once deployed; new variants should be appended at
/// the end so that existing on-chain data (keyed by XDR-encoded discriminants)
/// continues to decode correctly.
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
    /// Admin-configured max amount withdrawable per rolling window; `0` means uncapped (#455).
    WithdrawalLimitPerWindow,
    /// Window length (seconds) paired with `WithdrawalLimitPerWindow`.
    WithdrawalWindowSecs,
    /// Start timestamp of the current withdrawal window for a given tracked address.
    WithdrawalWindowStart(Address),
    /// Amount withdrawn so far within the current window for a given tracked address.
    WithdrawnInWindow(Address),
    /// Monotonically-increasing counter for `SignerChangeProposal` identifiers (#447).
    SignerChangeCount,
    /// Persistent storage for a timelocked signer/threshold-change proposal (#447).
    SignerChange(u64),
}

/// Returns the approval weight assigned to `signer`, or `0` if not registered.
///
/// # Examples
///
/// ```rust,no_run
/// use soroban_sdk::{Address, Env};
/// use multisig::signer_weight;
///
/// // In a contract or test context where `env` and `signer` are available:
/// # let env: Env = unimplemented!();
/// # let signer: Address = unimplemented!();
/// let weight = signer_weight(&env, &signer);
/// if weight == 0 {
///     // signer is not registered; treat as unauthorized
/// } else {
///     // signer has `weight` votes toward threshold
/// }
/// ```
pub fn signer_weight(env: &Env, signer: &Address) -> u32 {
    env.storage()
        .instance()
        .get(&DataKey::Signer(signer.clone()))
        .unwrap_or(0)
}

/// Requires `signer` to authenticate and have a non-zero weight in the signer registry.
/// Panics: `UnauthorizedSigner`.
///
/// # Examples
///
/// ```rust,no_run
/// use soroban_sdk::{Address, Env, Vec};
/// use multisig::{require_authorized_signer, record_approval, meets_threshold};
///
/// // Typical usage inside a contract approval handler:
/// # let env: Env = unimplemented!();
/// # let signer: Address = unimplemented!();
/// # let mut approvals: Vec<Address> = unimplemented!();
/// # let mut weight: u32 = 0;
/// # let threshold: u32 = 2;
/// // 1. Authenticate and assert the signer is registered.
/// require_authorized_signer(&env, &signer);
///
/// // 2. Record the approval and accumulate weight.
/// record_approval(&env, &mut approvals, &mut weight, &signer);
///
/// // 3. Check whether quorum is now satisfied.
/// if meets_threshold(weight, threshold) {
///     // execute the guarded action
/// }
/// ```
pub fn require_authorized_signer(env: &Env, signer: &Address) {
    signer.require_auth();
    if signer_weight(env, signer) == 0 {
        soroban_sdk::panic_with_error!(env, TreasuryError::UnauthorizedSigner);
    }
}

/// Adds `signer`'s weight to `weight` and appends `signer` to `approvals`, unless `signer` has
/// already approved (in which case this is a no-op). Captures the dedup-then-accumulate pattern
/// used for settlement, dispute, and rotation approvals.
///
/// # Examples
///
/// ```rust,no_run
/// use soroban_sdk::{Address, Env, Vec};
/// use multisig::{record_approval, meets_threshold};
///
/// // Accumulate approvals from multiple signers toward a threshold of 3.
/// # let env: Env = unimplemented!();
/// # let signer_a: Address = unimplemented!();
/// # let signer_b: Address = unimplemented!();
/// # let mut approvals: Vec<Address> = unimplemented!();
/// # let mut weight: u32 = 0;
/// # let threshold: u32 = 3;
/// record_approval(&env, &mut approvals, &mut weight, &signer_a);
/// record_approval(&env, &mut approvals, &mut weight, &signer_b);
///
/// // Duplicate call from signer_a is a no-op — weight stays the same.
/// record_approval(&env, &mut approvals, &mut weight, &signer_a);
///
/// if meets_threshold(weight, threshold) {
///     // quorum reached — proceed with execution
/// }
/// ```
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
///
/// # Examples
///
/// ```rust
/// use multisig::meets_threshold;
///
/// // Exact threshold: quorum reached.
/// assert!(meets_threshold(3, 3));
///
/// // Above threshold: quorum reached.
/// assert!(meets_threshold(5, 3));
///
/// // Below threshold: quorum not reached.
/// assert!(!meets_threshold(2, 3));
///
/// // Zero threshold is trivially satisfied by any weight.
/// assert!(meets_threshold(0, 0));
/// ```
pub fn meets_threshold(weight: u32, threshold: u32) -> bool {
    weight >= threshold
}

#[cfg(feature = "testutils")]
#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Env;

    /// Tests the `.unwrap_or(0)` contract in `signer_weight`: an address that was
    /// never passed to any `set_signer` call (i.e. has no entry under
    /// `DataKey::Signer(addr)` in instance storage) returns `0` and does not panic.
    #[test]
    fn signer_weight_returns_zero_for_never_registered_address() {
        let env = Env::default();
        let never_registered = Address::generate(&env);
        let weight = signer_weight(&env, &never_registered);
        assert_eq!(weight, 0);
    }

    /// Tests that `record_approval` correctly accumulates weight up to exactly
    /// `u32::MAX` without panicking. This pins the upper boundary of the happy
    /// path: the final `checked_add` that produces `u32::MAX` must succeed.
    #[test]
    fn record_approval_accumulates_to_u32_max() {
        let env = Env::default();
        env.mock_all_auths();

        let signer_a = Address::generate(&env);
        let signer_b = Address::generate(&env);

        // Register signer_a with weight u32::MAX - 1 and signer_b with weight 1,
        // so their combined weight exactly equals u32::MAX.
        let weight_a: u32 = u32::MAX - 1;
        let weight_b: u32 = 1;
        env.storage()
            .instance()
            .set(&DataKey::Signer(signer_a.clone()), &weight_a);
        env.storage()
            .instance()
            .set(&DataKey::Signer(signer_b.clone()), &weight_b);

        let mut approvals: Vec<Address> = Vec::new(&env);
        let mut accumulated_weight: u32 = 0;

        record_approval(&env, &mut approvals, &mut accumulated_weight, &signer_a);
        assert_eq!(accumulated_weight, u32::MAX - 1);

        // Adding signer_b's weight of 1 should bring the total to exactly u32::MAX —
        // checked_add must succeed here; u32::MAX is a valid, non-overflowing result.
        record_approval(&env, &mut approvals, &mut accumulated_weight, &signer_b);
        assert_eq!(accumulated_weight, u32::MAX);
    }

    /// Tests that `record_approval` panics with `WeightOverflow` when accumulating
    /// signer weights would exceed `u32::MAX`. Uses `#[should_panic]` because
    /// `panic_with_error!` inside a no_std Soroban contract produces a host-level
    /// panic that propagates out of the call in test mode.
    ///
    /// Boundary being tested: the `checked_add` in `record_approval` returns `None`
    /// when `u32::MAX + 1` would wrap, and the `.unwrap_or_else` branch fires
    /// `panic_with_error!(env, TreasuryError::WeightOverflow)`.
    #[test]
    #[should_panic]
    fn record_approval_panics_on_weight_overflow() {
        let env = Env::default();
        env.mock_all_auths();

        let signer_a = Address::generate(&env);
        let signer_b = Address::generate(&env);
        let signer_c = Address::generate(&env);

        // signer_a holds u32::MAX - 1, signer_b holds 1 (sum = u32::MAX),
        // signer_c holds 1 (adding it would overflow past u32::MAX).
        let weight_a: u32 = u32::MAX - 1;
        let weight_b: u32 = 1;
        let weight_c: u32 = 1;
        env.storage()
            .instance()
            .set(&DataKey::Signer(signer_a.clone()), &weight_a);
        env.storage()
            .instance()
            .set(&DataKey::Signer(signer_b.clone()), &weight_b);
        env.storage()
            .instance()
            .set(&DataKey::Signer(signer_c.clone()), &weight_c);

        let mut approvals: Vec<Address> = Vec::new(&env);
        let mut accumulated_weight: u32 = 0;

        // Bring accumulated weight up to u32::MAX (no panic expected here).
        record_approval(&env, &mut approvals, &mut accumulated_weight, &signer_a);
        record_approval(&env, &mut approvals, &mut accumulated_weight, &signer_b);
        assert_eq!(accumulated_weight, u32::MAX);

        // This call attempts u32::MAX + 1, which overflows — must panic.
        record_approval(&env, &mut approvals, &mut accumulated_weight, &signer_c);
    }
}
