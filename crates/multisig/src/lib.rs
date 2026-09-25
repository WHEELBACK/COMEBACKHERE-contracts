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
    // Appended for the settlement-workflow two-step admin transfer (#621):
    // `accept_admin` was called with no `transfer_admin` nomination outstanding.
    // Lives here rather than in a workflow-local enum for the same reason
    // `ComplianceCheckFailed` does — the workflow contract reuses
    // `TreasuryError` as its single error type, so every code it can return is
    // declared in this one append-only enum.
    NoPendingAdmin = 41,
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

/// Expiry metadata for an approval collected toward a multisig action.
///
/// `expires_at == 0` means the approval does not expire. Otherwise callers should
/// compare the value with `Env::ledger().timestamp()` before counting the signer
/// weight toward quorum.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalExpiry {
    pub signer: Address,
    pub approved_at: u64,
    pub expires_at: u64,
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

/// Builds expiry metadata for a newly collected approval.
pub fn approval_expiry(env: &Env, signer: &Address, ttl_seconds: u64) -> ApprovalExpiry {
    let approved_at = env.ledger().timestamp();
    let expires_at = if ttl_seconds == 0 {
        0
    } else {
        approved_at.checked_add(ttl_seconds).unwrap_or_else(|| {
            soroban_sdk::panic_with_error!(env, TreasuryError::ArithmeticOverflow)
        })
    };
    ApprovalExpiry {
        signer: signer.clone(),
        approved_at,
        expires_at,
    }
}

/// Returns whether approval metadata is still countable at the current ledger time.
pub fn approval_is_active(env: &Env, approval: &ApprovalExpiry) -> bool {
    approval.expires_at == 0 || env.ledger().timestamp() <= approval.expires_at
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

// Unit tests for the multisig crate's shared logic (#623).
//
// `signer_weight`, `require_authorized_signer`, `record_approval` and
// `meets_threshold` are the quorum primitives every treasury entrypoint leans
// on. Until now they were only reachable indirectly through the treasury
// contract's integration tests, so a failure pointed at "treasury" rather than at
// the crate that actually broke, and every edge case had to be re-derived as a
// full contract deployment to pin down.
//
// Two things are worth calling out:
//
// * Deliberately gated on `#[cfg(test)]` only — NOT on `feature = "testutils"`.
//   Nothing in the workspace enables `multisig/testutils`, and a crate's own
//   feature cannot be switched on for its own unit tests without a non-default
//   `cargo test --features ...` command, so a feature gate here meant the tests
//   silently did not run at all. `soroban-sdk/testutils` comes in through the
//   dev-dependency in Cargo.toml instead, which affects test builds only and
//   leaves wasm32 release builds untouched.
//
// * The functions under test are instance-storage readers, and
//   `soroban-sdk` refuses instance-storage access outside a contract frame
//   ("this function is not accessible outside of a contract"). They are
//   therefore driven through `MultisigHarness`, a throwaway `#[contract]` defined
//   below that does nothing but forward to the real functions. That also means
//   each call goes through a real contract invocation, so `require_auth` is
//   recorded the same way it is in production and `env.auths()` can be inspected.
#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{contract, contractimpl, Env};

    /// Test-only pass-through contract. Every entrypoint forwards to the
    /// function under test, so the assertions below exercise the real
    /// implementation rather than a copy of it.
    #[contract]
    struct MultisigHarness;

    #[contractimpl]
    impl MultisigHarness {
        /// Mirrors the write `Treasury::set_signer` performs. The crate under
        /// test exposes no setter — signers are managed by the treasury contract
        /// — so these two entrypoints stand in for it.
        pub fn set_signer_weight(env: Env, signer: Address, weight: u32) {
            env.storage()
                .instance()
                .set(&DataKey::Signer(signer.clone()), &weight);
        }

        /// Mirrors the delete `Treasury::remove_signer` performs: the entry is
        /// removed outright rather than zeroed.
        pub fn remove_signer_weight(env: Env, signer: Address) {
            env.storage()
                .instance()
                .remove(&DataKey::Signer(signer.clone()));
        }

        pub fn signer_weight(env: Env, signer: Address) -> u32 {
            crate::signer_weight(&env, &signer)
        }

        pub fn require_authorized_signer(env: Env, signer: Address) {
            crate::require_authorized_signer(&env, &signer);
        }

        /// `record_approval` takes `&mut` accumulators, which contract
        /// entrypoints cannot express, so they are passed and returned by value.
        pub fn record_approval(
            env: Env,
            approvals: Vec<Address>,
            weight: u32,
            signer: Address,
        ) -> (Vec<Address>, u32) {
            let mut approvals = approvals;
            let mut weight = weight;
            crate::record_approval(&env, &mut approvals, &mut weight, &signer);
            (approvals, weight)
        }

        pub fn meets_threshold(weight: u32, threshold: u32) -> bool {
            crate::meets_threshold(weight, threshold)
        }
    }

    /// Sets up an env with the harness registered. Signer weights are written via
    /// the harness so they live in the same contract instance the helpers read.
    fn setup() -> (Env, MultisigHarnessClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let id = env.register(MultisigHarness, ());
        let client = MultisigHarnessClient::new(&env, &id);
        (env, client)
    }

    /// Asserts `approvals` holds exactly `expected`, in order.
    ///
    /// Takes a slice rather than returning a collection because this crate is
    /// `#![no_std]`: there is no `vec!` macro or `std::vec::Vec` in scope, and
    /// the test module deliberately avoids needing an `extern crate std`.
    fn assert_approvals(approvals: &Vec<Address>, expected: &[Address]) {
        assert_eq!(
            approvals.len(),
            expected.len() as u32,
            "unexpected number of recorded approvals"
        );
        for (i, addr) in expected.iter().enumerate() {
            assert_eq!(
                approvals.get(i as u32).unwrap(),
                *addr,
                "approval at position {i} is the wrong address"
            );
        }
    }

    // ---------------------------------------------------------------------
    // signer_weight
    // ---------------------------------------------------------------------

    /// Tests the `.unwrap_or(0)` contract in `signer_weight`: an address that was
    /// never passed to any `set_signer` call (i.e. has no entry under
    /// `DataKey::Signer(addr)` in instance storage) returns `0` and does not panic.
    #[test]
    fn signer_weight_returns_zero_for_never_registered_address() {
        let (_env, client) = setup();
        let never_registered = Address::generate(&_env);
        let weight = client.signer_weight(&never_registered);
        assert_eq!(weight, 0);
    }

    /// A registered signer's weight is read back verbatim — the value written by
    /// `set_signer` is exactly what quorum accounting sees.
    #[test]
    fn signer_weight_returns_the_registered_weight() {
        let (_env, client) = setup();
        let signer = Address::generate(&_env);
        client.set_signer_weight(&signer, &7);
        assert_eq!(client.signer_weight(&signer), 7);
    }

    /// Weights are per-address, not global: registering one signer must not
    /// change what any other address reads.
    #[test]
    fn signer_weight_is_independent_per_address() {
        let (_env, client) = setup();
        let alice = Address::generate(&_env);
        let bob = Address::generate(&_env);
        client.set_signer_weight(&alice, &3);
        client.set_signer_weight(&bob, &11);

        assert_eq!(client.signer_weight(&alice), 3);
        assert_eq!(client.signer_weight(&bob), 11);
    }

    /// Re-registering a signer with a new weight takes effect immediately. This
    /// is the `set_signer` upsert path, not the rotation path.
    #[test]
    fn signer_weight_reflects_the_latest_registration() {
        let (_env, client) = setup();
        let signer = Address::generate(&_env);
        client.set_signer_weight(&signer, &5);
        assert_eq!(client.signer_weight(&signer), 5);

        client.set_signer_weight(&signer, &9);
        assert_eq!(client.signer_weight(&signer), 9);

        client.set_signer_weight(&signer, &1);
        assert_eq!(client.signer_weight(&signer), 1);
    }

    /// A signer whose entry was deleted (`remove_signer`) reads back as `0`,
    /// which is what makes `require_authorized_signer` reject it. Deletion and an
    /// explicit zero weight are different writes but must be indistinguishable to
    /// callers.
    #[test]
    fn signer_weight_returns_zero_after_removal() {
        let (_env, client) = setup();
        let signer = Address::generate(&_env);
        client.set_signer_weight(&signer, &4);
        assert_eq!(client.signer_weight(&signer), 4);

        client.remove_signer_weight(&signer);
        assert_eq!(client.signer_weight(&signer), 0);
    }

    /// A signer explicitly deactivated by `set_signer(_, 0)` reads back as `0`,
    /// matching the removed-signer case above.
    #[test]
    fn signer_weight_returns_zero_for_zero_weight_signer() {
        let (_env, client) = setup();
        let signer = Address::generate(&_env);
        client.set_signer_weight(&signer, &0);
        assert_eq!(client.signer_weight(&signer), 0);
    }

    /// The largest representable weight round-trips rather than saturating or
    /// wrapping.
    #[test]
    fn signer_weight_supports_u32_max() {
        let (_env, client) = setup();
        let signer = Address::generate(&_env);
        client.set_signer_weight(&signer, &u32::MAX);
        assert_eq!(client.signer_weight(&signer), u32::MAX);
    }

    // ---------------------------------------------------------------------
    // meets_threshold
    // ---------------------------------------------------------------------

    /// Core `>=` semantics: exact match, above, and below.
    #[test]
    fn meets_threshold_compares_weight_against_threshold() {
        let (_env, client) = setup();
        assert!(client.meets_threshold(&3, &3), "exact must be satisfied");
        assert!(client.meets_threshold(&4, &3), "above must be satisfied");
        assert!(
            !client.meets_threshold(&2, &3),
            "below must not be satisfied"
        );
    }

    /// One short of the threshold is not enough — the off-by-one that would let a
    /// settlement execute a vote early.
    #[test]
    fn meets_threshold_is_false_one_below() {
        let (_env, client) = setup();
        assert!(!client.meets_threshold(&2, &3));
        assert!(client.meets_threshold(&3, &3));
    }

    /// A zero threshold is trivially satisfied, including by zero weight. This is
    /// why `Treasury::initialize` rejects a zero threshold with `ZeroThreshold`
    /// rather than relying on quorum logic to catch it.
    #[test]
    fn meets_threshold_with_zero_threshold_is_always_satisfied() {
        let (_env, client) = setup();
        assert!(client.meets_threshold(&0, &0));
        assert!(client.meets_threshold(&1, &0));
        assert!(client.meets_threshold(&u32::MAX, &0));
    }

    /// Zero weight never satisfies a non-zero threshold, so a zero-weight signer
    /// can never be the deciding vote.
    #[test]
    fn meets_threshold_with_zero_weight_and_nonzero_threshold_is_false() {
        let (_env, client) = setup();
        assert!(!client.meets_threshold(&0, &1));
        assert!(!client.meets_threshold(&0, &u32::MAX));
    }

    /// Boundary sweep across the whole `u32` range: the only pair that must
    /// return `false` is `weight < threshold`.
    #[test]
    fn meets_threshold_boundaries() {
        let (_env, client) = setup();
        assert!(client.meets_threshold(&1, &1));
        assert!(client.meets_threshold(&u32::MAX, &u32::MAX));
        assert!(client.meets_threshold(&u32::MAX, &(u32::MAX - 1)));
        assert!(!client.meets_threshold(&(u32::MAX - 1), &u32::MAX));
        assert!(!client.meets_threshold(&0, &u32::MAX));
    }

    /// `meets_threshold` stays consistent when driven by the weight
    /// `record_approval` actually accumulates, not by hand-supplied literals.
    /// Pins the "threshold equal to total weight of all signers" case the issue
    /// calls out: every signer must approve, and only then is it satisfied.
    #[test]
    fn meets_threshold_at_exactly_total_registered_weight() {
        let (env, client) = setup();

        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let carol = Address::generate(&env);
        client.set_signer_weight(&alice, &2);
        client.set_signer_weight(&bob, &3);
        client.set_signer_weight(&carol, &5);
        let total: u32 = 2 + 3 + 5;

        let approvals = Vec::new(&env);
        let weight: u32 = 0;

        let (a, w) = client.record_approval(&approvals, &weight, &alice);
        let (a, w) = client.record_approval(&a, &w, &bob);
        assert!(
            !client.meets_threshold(&w, &total),
            "5 of 10 is not a quorum"
        );

        let (_a, w) = client.record_approval(&a, &w, &carol);
        assert_eq!(w, total);
        assert!(
            client.meets_threshold(&w, &total),
            "the full set of signers must satisfy a threshold set to their total weight"
        );
        assert!(!client.meets_threshold(&w, &(total + 1)));
    }

    // ---------------------------------------------------------------------
    // record_approval
    // ---------------------------------------------------------------------

    /// Tests that `record_approval` correctly accumulates weight up to exactly
    /// `u32::MAX` without panicking. This pins the upper boundary of the happy
    /// path: the final `checked_add` that produces `u32::MAX` must succeed.
    #[test]
    fn record_approval_accumulates_to_u32_max() {
        let (env, client) = setup();
        let signer_a = Address::generate(&env);
        let signer_b = Address::generate(&env);

        // Register signer_a with weight u32::MAX - 1 and signer_b with weight 1,
        // so their combined weight exactly equals u32::MAX.
        client.set_signer_weight(&signer_a, &(u32::MAX - 1));
        client.set_signer_weight(&signer_b, &1);

        let approvals = Vec::new(&env);
        let (approvals, weight) = client.record_approval(&approvals, &0, &signer_a);
        assert_eq!(weight, u32::MAX - 1);

        // Adding signer_b's weight of 1 should bring the total to exactly u32::MAX —
        // checked_add must succeed here; u32::MAX is a valid, non-overflowing result.
        let (_approvals, weight) = client.record_approval(&approvals, &weight, &signer_b);
        assert_eq!(weight, u32::MAX);
    }

    /// `record_approval` fails with a typed `WeightOverflow` when accumulating
    /// signer weights would exceed `u32::MAX`.
    ///
    /// Boundary being tested: the `checked_add` in `record_approval` returns `None`
    /// when `u32::MAX + 1` would wrap, and the `.unwrap_or_else` branch fires
    /// `panic_with_error!(env, TreasuryError::WeightOverflow)`.
    #[test]
    fn record_approval_errors_on_weight_overflow() {
        let (env, client) = setup();
        let signer_a = Address::generate(&env);
        let signer_b = Address::generate(&env);
        let signer_c = Address::generate(&env);

        // signer_a holds u32::MAX - 1, signer_b holds 1 (sum = u32::MAX),
        // signer_c holds 1 (adding it would overflow past u32::MAX).
        client.set_signer_weight(&signer_a, &(u32::MAX - 1));
        client.set_signer_weight(&signer_b, &1);
        client.set_signer_weight(&signer_c, &1);

        let approvals = Vec::new(&env);
        // Bring accumulated weight up to u32::MAX (no error expected here).
        let (approvals, weight) = client.record_approval(&approvals, &0, &signer_a);
        let (_approvals, weight) = client.record_approval(&approvals, &weight, &signer_b);
        assert_eq!(weight, u32::MAX);

        // This call attempts u32::MAX + 1, which must fail rather than wrap.
        let err = client
            .try_record_approval(&Vec::new(&env), &weight, &signer_c)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, TreasuryError::WeightOverflow.into());
    }

    /// A first approval records both the weight and the address.
    #[test]
    fn record_approval_records_weight_and_address() {
        let (env, client) = setup();
        let signer = Address::generate(&env);
        client.set_signer_weight(&signer, &6);

        let approvals = Vec::new(&env);
        let (approvals, weight) = client.record_approval(&approvals, &0, &signer);

        assert_eq!(weight, 6);
        assert_approvals(&approvals, &[signer]);
    }

    /// A repeated approval from the same signer is a complete no-op: weight is not
    /// double-counted and the address is not appended twice. Without the
    /// `contains` guard a single signer could approve `n` times to reach any
    /// threshold, so this is the most security-relevant behaviour here.
    #[test]
    fn record_approval_deduplicates_repeat_approvals() {
        let (env, client) = setup();
        let signer = Address::generate(&env);
        client.set_signer_weight(&signer, &2);

        let approvals = Vec::new(&env);
        let mut approvals = approvals;
        let mut weight: u32 = 0;
        for _ in 0..5 {
            let (a, w) = client.record_approval(&approvals, &weight, &signer);
            approvals = a;
            weight = w;
        }

        assert_eq!(weight, 2, "repeat approvals must not accumulate weight");
        assert_eq!(
            approvals.len(),
            1,
            "repeat approvals must not append the address again"
        );
    }

    /// Deduplication is by address and applies regardless of position in the
    /// sequence, not just for consecutive calls.
    #[test]
    fn record_approval_deduplicates_non_adjacent_approvals() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        client.set_signer_weight(&alice, &1);
        client.set_signer_weight(&bob, &1);

        let approvals = Vec::new(&env);
        let (approvals, weight) = client.record_approval(&approvals, &0, &alice);
        let (approvals, weight) = client.record_approval(&approvals, &weight, &bob);
        let (approvals, weight) = client.record_approval(&approvals, &weight, &alice);
        let (approvals, weight) = client.record_approval(&approvals, &weight, &alice);

        assert_eq!(weight, 2);
        assert_approvals(&approvals, &[alice, bob]);
    }

    /// A zero-weight signer is still appended to `approvals` and contributes `0`
    /// to the weight. It cannot help reach a threshold, but it is still recorded,
    /// and — critically — it can never be counted twice either.
    #[test]
    fn record_approval_handles_zero_weight_signers() {
        let (env, client) = setup();
        let zero_weight = Address::generate(&env);
        let real_signer = Address::generate(&env);
        client.set_signer_weight(&zero_weight, &0);
        client.set_signer_weight(&real_signer, &4);

        let approvals = Vec::new(&env);
        let (approvals, weight) = client.record_approval(&approvals, &0, &zero_weight);
        assert_eq!(weight, 0, "a zero-weight signer must add nothing");
        assert_approvals(&approvals, &[zero_weight.clone()]);

        let (approvals, weight) = client.record_approval(&approvals, &weight, &zero_weight);
        assert_eq!(weight, 0);
        assert_eq!(approvals.len(), 1);

        let (approvals, weight) = client.record_approval(&approvals, &weight, &real_signer);
        assert_eq!(weight, 4);
        assert_approvals(&approvals, &[zero_weight, real_signer]);
    }

    /// A signer that was never registered has weight `0` and is recorded the same
    /// way as an explicit zero-weight signer. `record_approval` is not an
    /// authorization check — `require_authorized_signer` is the gate. This test
    /// documents that the two are independent, so a future refactor does not fold
    /// one into the other by accident.
    #[test]
    fn record_approval_accepts_unregistered_signer_with_zero_weight() {
        let (env, client) = setup();
        let unregistered = Address::generate(&env);

        let approvals = Vec::new(&env);
        let (approvals, weight) = client.record_approval(&approvals, &0, &unregistered);

        assert_eq!(weight, 0);
        assert_approvals(&approvals, &[unregistered]);
    }

    /// A signer removed *after* approving keeps the weight it contributed: the
    /// approval snapshot is not recomputed. This matches treasury's documented
    /// behaviour that removing a signer does not retroactively invalidate in-flight
    /// approvals.
    #[test]
    fn record_approval_keeps_weight_of_a_signer_removed_after_approving() {
        let (env, client) = setup();
        let signer = Address::generate(&env);
        client.set_signer_weight(&signer, &5);

        let approvals = Vec::new(&env);
        let (approvals, weight) = client.record_approval(&approvals, &0, &signer);
        assert_eq!(weight, 5);

        client.remove_signer_weight(&signer);
        assert_eq!(client.signer_weight(&signer), 0);
        assert_eq!(weight, 5, "already-recorded approval weight is unchanged");
        assert_approvals(&approvals, &[signer]);
    }

    /// A signer re-weighted before approving contributes their *new* weight. This
    /// is the direct contrast with the removed-signer case above: removal reads back
    /// as zero, but a re-weight to a live value is read at approval time.
    #[test]
    fn record_approval_uses_current_weight_for_signers_not_yet_approving() {
        let (env, client) = setup();
        let approver = Address::generate(&env);
        let later = Address::generate(&env);
        client.set_signer_weight(&approver, &1);
        client.set_signer_weight(&later, &2);

        let approvals = Vec::new(&env);
        let (approvals, weight) = client.record_approval(&approvals, &0, &approver);
        assert_eq!(weight, 1);

        // Re-weight `later` before it approves; its contribution follows the new
        // value.
        client.set_signer_weight(&later, &8);
        let (_approvals, weight) = client.record_approval(&approvals, &weight, &later);
        assert_eq!(weight, 9);
    }

    /// Distinct signers accumulate in insertion order, and the total is the sum of
    /// their weights.
    #[test]
    fn record_approval_accumulates_distinct_signers_in_order() {
        let (env, client) = setup();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        let c = Address::generate(&env);
        client.set_signer_weight(&a, &10);
        client.set_signer_weight(&b, &20);
        client.set_signer_weight(&c, &30);

        let approvals = Vec::new(&env);
        let (approvals, weight) = client.record_approval(&approvals, &0, &a);
        let (approvals, weight) = client.record_approval(&approvals, &weight, &b);
        let (approvals, weight) = client.record_approval(&approvals, &weight, &c);

        assert_eq!(weight, 60);
        assert_approvals(&approvals, &[a, b, c]);
    }

    /// A pre-populated `approvals` vector is respected: an address already in it is
    /// not appended again and contributes no weight, so resuming a partially
    /// recorded approval round is safe.
    #[test]
    fn record_approval_respects_preexisting_approvals() {
        let (env, client) = setup();
        let a = Address::generate(&env);
        let b = Address::generate(&env);
        client.set_signer_weight(&a, &3);
        client.set_signer_weight(&b, &4);

        let mut approvals = Vec::new(&env);
        approvals.push_back(a.clone());

        let (approvals, weight) = client.record_approval(&approvals, &0, &a);
        assert_eq!(weight, 0, "an address already present adds no weight");
        assert_eq!(approvals.len(), 1);

        let (approvals, weight) = client.record_approval(&approvals, &weight, &b);
        assert_eq!(weight, 4);
        assert_approvals(&approvals, &[a, b]);
    }

    // ---------------------------------------------------------------------
    // require_authorized_signer
    // ---------------------------------------------------------------------

    /// A registered signer with non-zero weight is authorized and the call returns
    /// normally.
    #[test]
    fn require_authorized_signer_allows_a_registered_signer() {
        let (env, client) = setup();
        let signer = Address::generate(&env);
        client.set_signer_weight(&signer, &1);

        client.require_authorized_signer(&signer);
    }

    /// The signer must authenticate: `require_auth` is invoked, and it is the
    /// signer itself — not the caller or some ambient account — that is recorded as
    /// having authorized. This is what stops a caller from approving on someone
    /// else's behalf.
    #[test]
    fn require_authorized_signer_requests_the_signers_own_auth() {
        let (env, client) = setup();
        let signer = Address::generate(&env);
        client.set_signer_weight(&signer, &1);

        client.require_authorized_signer(&signer);

        let auths = env.auths();
        assert_eq!(auths.len(), 1, "expected exactly one require_auth");
        let (address, _invocation) = auths.first().unwrap();
        assert_eq!(
            *address, signer,
            "the signer must be the authorizing address"
        );
    }

    /// An unregistered address is rejected with `UnauthorizedSigner` rather than
    /// being treated as a weight-0 signer that is merely unhelpful.
    #[test]
    fn require_authorized_signer_rejects_an_unregistered_address() {
        let (env, client) = setup();
        let stranger = Address::generate(&env);

        let err = client
            .try_require_authorized_signer(&stranger)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, TreasuryError::UnauthorizedSigner.into());
    }

    /// A signer removed via `remove_signer` (its storage entry deleted) is rejected
    /// — the issue's "signers that have been removed" case. Authorization is lost
    /// immediately, on the very next call.
    #[test]
    fn require_authorized_signer_rejects_a_removed_signer() {
        let (env, client) = setup();
        let signer = Address::generate(&env);
        client.set_signer_weight(&signer, &9);
        // Authorized while registered...
        client.require_authorized_signer(&signer);

        // ...and rejected immediately after removal.
        client.remove_signer_weight(&signer);
        let err = client
            .try_require_authorized_signer(&signer)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, TreasuryError::UnauthorizedSigner.into());
    }

    /// A signer deactivated by `set_signer(_, 0)` is rejected too:
    /// `signer_weight` cannot distinguish "weight 0" from "absent", and both must
    /// fail the gate.
    #[test]
    fn require_authorized_signer_rejects_a_zero_weight_signer() {
        let (env, client) = setup();
        let signer = Address::generate(&env);
        client.set_signer_weight(&signer, &0);

        let err = client
            .try_require_authorized_signer(&signer)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, TreasuryError::UnauthorizedSigner.into());
    }

    /// A signer downgraded to weight 0 *after* being registered loses authorization
    /// immediately.
    #[test]
    fn require_authorized_signer_rejects_after_weight_is_zeroed() {
        let (env, client) = setup();
        let signer = Address::generate(&env);
        client.set_signer_weight(&signer, &1);
        client.require_authorized_signer(&signer);

        client.set_signer_weight(&signer, &0);
        let err = client
            .try_require_authorized_signer(&signer)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, TreasuryError::UnauthorizedSigner.into());
    }

    /// `u32::MAX` weight is still "non-zero" and therefore authorized — the gate is
    /// `== 0`, not "is some plausible value". Pins that the check does not silently
    /// grow an upper bound.
    #[test]
    fn require_authorized_signer_allows_u32_max_weight() {
        let (env, client) = setup();
        let signer = Address::generate(&env);
        client.set_signer_weight(&signer, &u32::MAX);

        client.require_authorized_signer(&signer);
    }
}
