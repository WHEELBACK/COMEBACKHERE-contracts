#![no_std]

pub use compliance_errors::ComplianceError;
pub use invoice_errors::InvoiceError;
pub use treasury::TreasuryError;
pub use workflow_errors::WorkflowError;

/// Typed error enum for the `escrow-vault` contract.
///
/// Declared through the shared `declare_contract_error!` macro so every
/// variant carries a unique, append-only discriminant and a doc comment.
/// New variants MUST be appended at the end to keep existing codes stable
/// for off-chain clients that persist them.
declare_contract_error! {
    /// Errors returned by the `escrow-vault` contract.
    pub enum EscrowVaultError {
        /// The contract has already been initialized.
        AlreadyInitialized = 1,
        /// The contract has not been initialized yet.
        NotInitialized = 2,
        /// Caller is not authorized for this action.
        Unauthorized = 3,
        /// The contract is paused.
        ContractPaused = 4,
        /// The contract is not paused.
        NotPaused = 5,
        /// An amount argument was zero or otherwise invalid.
        InvalidAmount = 6,
        /// The escrow was not found.
        EscrowNotFound = 7,
        /// The escrow is not in the state required by this action.
        InvalidEscrowState = 8,
        /// The escrow has already been funded.
        AlreadyFunded = 9,
        /// The escrow has not been funded yet.
        NotFunded = 10,
        /// The escrow has already been released.
        AlreadyReleased = 11,
        /// The escrow has not been released yet.
        NotReleased = 12,
        /// The escrow has already been refunded.
        AlreadyRefunded = 13,
        /// The escrow has not been refunded yet.
        NotRefunded = 14,
        /// The escrow has expired.
        EscrowExpired = 15,
        /// The escrow has not expired yet.
        EscrowNotExpired = 16,
        /// The escrow deadline is in the past or out of range.
        InvalidDeadline = 17,
        /// The supplied token contract is invalid.
        InvalidToken = 18,
        /// The supplied token does not match the escrow's token.
        TokenMismatch = 19,
        /// The beneficiary address is invalid.
        InvalidBeneficiary = 20,
        /// The depositor address is invalid.
        InvalidDepositor = 21,
        /// The arbiter address is invalid.
        InvalidArbiter = 22,
        /// The caller is not the arbiter of this escrow.
        NotArbiter = 23,
        /// The caller is not the depositor of this escrow.
        NotDepositor = 24,
        /// The caller is not the beneficiary of this escrow.
        NotBeneficiary = 25,
        /// A dispute has already been raised for this escrow.
        DisputeAlreadyRaised = 26,
        /// No dispute has been raised for this escrow.
        DisputeNotRaised = 27,
        /// The dispute has already been resolved.
        DisputeAlreadyResolved = 28,
        /// The dispute resolution direction is invalid.
        InvalidResolution = 29,
        /// The dispute window has not elapsed yet.
        DisputeWindowActive = 30,
        /// The fee exceeds the allowed maximum.
        FeeTooHigh = 31,
        /// The fee recipient address is invalid.
        InvalidFeeRecipient = 32,
        /// The refund window has closed.
        RefundWindowClosed = 33,
        /// The release window has closed.
        ReleaseWindowClosed = 34,
        /// A required hash argument is malformed.
        InvalidHash = 35,
        /// A supplied hash exceeds the maximum length.
        HashTooLong = 36,
        /// A supplied memo exceeds the maximum length.
        MemoTooLong = 37,
        /// A supplied label exceeds the maximum length.
        LabelTooLong = 38,
        /// Too many items were supplied in a single batch.
        BatchTooLarge = 39,
        /// The batch contains duplicate entries.
        DuplicateEntry = 40,
        /// The supplied nonce has already been used.
        DuplicateNonce = 41,
        /// The supplied reference id has already been used.
        DuplicateReferenceId = 42,
        /// The escrow count limit has been reached.
        EscrowCountOverflow = 43,
        /// An arithmetic operation overflowed.
        ArithmeticOverflow = 44,
        /// An arithmetic operation underflowed.
        ArithmeticUnderflow = 45,
        /// A division by zero was attempted.
        DivisionByZero = 46,
        /// The amount has too many decimal places.
        AmountPrecision = 47,
        /// The escrow balance is insufficient for this action.
        InsufficientBalance = 48,
        /// The token transfer failed.
        TransferFailed = 49,
        /// The refund transfer failed.
        RefundTransferFailed = 50,
        /// The escrow has no token configured to refund through.
        RefundTokenNotSet = 51,
        /// There is no pending admin transfer.
        NoPendingAdmin = 52,
        /// The timelock delay is outside the allowed bounds.
        InvalidTimelockDelay = 53,
        /// The timelock has not elapsed yet.
        TimelockNotElapsed = 54,
        /// The execution deadline has passed.
        ExecutionDeadlineExceeded = 55,
        /// The escrow cannot be cancelled in its current state.
        NotCancellable = 56,
        /// The escrow cannot be force-cancelled.
        ForceCancelNotAllowed = 57,
        /// The escrow is on hold.
        EscrowOnHold = 58,
        /// The escrow is not on hold.
        NotOnHold = 59,
        /// The destination address is not allowed.
        DestinationNotAllowed = 60,
        /// The address failed the compliance check.
        ComplianceCheckFailed = 61,
        /// The contract is already in the requested state.
        AlreadyInState = 62,
        /// The contract is not in the required state.
        NotInState = 63,
        /// The operation is not supported.
        UnsupportedOperation = 64,
        /// The supplied configuration is invalid.
        InvalidConfiguration = 65,
        /// The contract's storage is inconsistent.
        StorageInconsistent = 66,
        /// The escrow is self-referential (depositor equals beneficiary).
        SelfEscrowNotAllowed = 67,
        /// The escrow metadata does not match.
        MetadataMismatch = 68,
        /// The escrow template is disabled.
        TemplateDisabled = 69,
        /// The escrow has no pending release.
        NoPendingRelease = 70,
        /// The escrow has no pending refund.
        NoPendingRefund = 71,
        /// The escrow has no pending dispute.
        NoPendingDispute = 72,
        /// The escrow has no pending admin transfer.
        NoPendingTransfer = 73,
        /// The escrow has no pending signer change.
        NoPendingSignerChange = 74,
        /// The signer change delay has not elapsed yet.
        SignerChangeTooEarly = 75,
        /// The signer change has already been finalised.
        SignerChangeAlreadyFinalised = 76,
        /// The signer change was not found.
        SignerChangeNotFound = 77,
        /// The signer threshold is not configured.
        ThresholdNotConfigured = 78,
        /// The signer threshold is unreachable.
        ThresholdUnreachable = 79,
        /// The signer threshold has not been met.
        ThresholdNotMet = 80,
        /// The signer has not approved this action.
        ApprovalNotFound = 81,
        /// The signer has already voted on this action.
        DuplicateVote = 82,
        /// The signer label is too long.
        LabelOverflow = 83,
        /// The total signer weight is too large.
        WeightOverflow = 84,
        /// The allowlist is full.
        AllowlistFull = 85,
        /// The token still has pending escrows.
        TokenHasPendingEscrows = 86,
        /// The token still has depositor balances.
        TokenHasBalance = 87,
        /// The withdrawal limit has been exceeded.
        WithdrawalLimitExceeded = 88,
        /// The dispute split ratio is invalid.
        InvalidSplitRatio = 89,
        /// The late fee exceeds the allowed maximum.
        InvalidLateFee = 90,
        /// The discount exceeds the escrow amount.
        DiscountExceedsAmount = 91,
        /// The discount deadline is after the escrow expiry.
        DiscountDeadlineAfterExpiry = 92,
        /// The refund fee is too high.
        RefundFeeTooHigh = 93,
        /// The escrow does not describe a completed payment.
        PaymentStateInconsistent = 94,
        /// The payment token does not match the escrow.
        PaymentTokenMismatch = 95,
        /// The payment link hash is invalid.
        InvalidPaymentLinkHash = 96,
        /// The escrow has no refund requested.
        NotRefundRequested = 97,
        /// The escrow has not been paid.
        NotPaid = 98,
        /// The escrow is not pending.
        NotPending = 99,
        /// The escrow has expired and cannot be acted on.
        Expired = 100,
        /// The escrow duration must be greater than zero.
        ZeroDuration = 101,
        /// The escrow expiry time is out of range.
        ExpiryOverflow = 102,
        /// The escrow expiry is too far in the future.
        ExpiryTooLong = 103,
        /// A cooldown is active; retry later.
        CooldownActive = 104,
        /// The settlement workflow is not a registered signer.
        WorkflowNotRegisteredSigner = 105,
        /// The settlement is on hold.
        SettlementOnHold = 106,
        /// The dispute window has not ended yet.
        DisputeNotExpired = 107,
        /// The settlement cannot be cancelled.
        SettlementNotCancellable = 108,
        /// The settlement has not expired yet.
        TtlNotElapsed = 109,
        /// The settlement has already been executed.
        AlreadyExecuted = 110,
        /// The settlement has already been put on hold.
        AlreadyOnHold = 111,
        /// The settlement was not found.
        SettlementNotFound = 112,
        /// The dispute was not found.
        DisputeNotFound = 113,
        /// The signer rotation was not found.
        RotationNotFound = 114,
        /// The signer rotation has already been executed.
        RotationAlreadyExecuted = 115,
        /// The signer rotation proposal cooldown is active.
        RotationProposalCooldown = 116,
        /// The signer is not authorized.
        UnauthorizedSigner = 117,
        /// The token is not allowed.
        TokenNotAllowed = 118,
        /// The token contract is invalid.
        InvalidTokenContract = 119,
        /// The signer threshold must be greater than zero.
        ZeroThreshold = 120,
        /// The dispute resolution direction is invalid.
        ResolutionDirectionMismatch = 121,
        /// The signer change would break the quorum.
        QuorumBreak = 122,
        /// The signer has already approved this action.
        AlreadyApproved = 123,
        /// The escrow is already in the requested state.
        AlreadyInRequestedState = 124,
        /// The escrow is not in the requested state.
        NotInRequestedState = 125,
        /// The escrow has no pending action.
        NoPendingAction = 126,
        /// The escrow has a pending action that must be resolved first.
        PendingActionExists = 127,
        /// The escrow has been finalized and cannot be modified.
        AlreadyFinalized = 128,
        /// The escrow has not been finalized yet.
        NotFinalized = 129,
        /// The escrow is locked.
        Locked = 130,
        /// The escrow is not locked.
        NotLocked = 131,
        /// The escrow is closed.
        Closed = 132,
        /// The escrow is not closed.
        NotClosed = 133,
        /// The escrow is archived.
        Archived = 134,
        /// The escrow is not archived.
        NotArchived = 135,
        /// The escrow is frozen.
        Frozen = 136,
        /// The escrow is not frozen.
        NotFrozen = 137,
        /// The escrow is suspended.
        Suspended = 138,
        /// The escrow is not suspended.
        NotSuspended = 139,
        /// The escrow is disabled.
        Disabled = 140,
        /// The escrow is not disabled.
        NotDisabled = 141,
        /// The escrow is active.
        Active = 142,
        /// The escrow is not active.
        NotActive = 143,
        /// The escrow is in an unknown state.
        UnknownState = 144,
        /// The escrow state transition is invalid.
        InvalidStateTransition = 145,
        /// The escrow state is inconsistent.
        StateInconsistent = 146,
        /// The escrow state is corrupted.
        StateCorrupted = 147,
        /// The escrow state is missing.
        StateMissing = 148,
        /// The escrow state is stale.
        StateStale = 149,
        /// The escrow state is outdated.
        StateOutdated = 150,
        /// The escrow state is unsupported.
        StateUnsupported = 151,
        /// The escrow state is invalid.
        StateInvalid = 152,
        /// The escrow state is unknown.
        StateUnknown = 153,
        /// The escrow state is ambiguous.
        StateAmbiguous = 154,
        /// The escrow state is conflicting.
        StateConflicting = 155,
        /// The escrow state is inconsistent with the ledger.
        StateLedgerMismatch = 156,
        /// The escrow state is inconsistent with the token.
        StateTokenMismatch = 157,
        /// The escrow state is inconsistent with the beneficiary.
        StateBeneficiaryMismatch = 158,
        /// The escrow state is inconsistent with the depositor.
        StateDepositorMismatch = 159,
        /// The escrow state is inconsistent with the arbiter.
        StateArbiterMismatch = 160,
        /// The escrow state is inconsistent with the amount.
        StateAmountMismatch = 161,
        /// The escrow state is inconsistent with the deadline.
        StateDeadlineMismatch = 162,
        /// The escrow state is inconsistent with the fee.
        StateFeeMismatch = 163,
        /// The escrow state is inconsistent with the dispute.
        StateDisputeMismatch = 164,
        /// The escrow state is inconsistent with the release.
        StateReleaseMismatch = 165,
        /// The escrow state is inconsistent with the refund.
        StateRefundMismatch = 166,
        /// The escrow state is inconsistent with the cancellation.
        StateCancellationMismatch = 167,
        /// The escrow state is inconsistent with the hold.
        StateHoldMismatch = 168,
        /// The escrow state is inconsistent with the pause.
        StatePauseMismatch = 169,
        /// The escrow state is inconsistent with the admin.
        StateAdminMismatch = 170,
        /// The escrow state is inconsistent with the signer.
        StateSignerMismatch = 171,
        /// The escrow state is inconsistent with the threshold.
        StateThresholdMismatch = 172,
        /// The escrow state is inconsistent with the rotation.
        StateRotationMismatch = 173,
        /// The escrow state is inconsistent with the workflow.
        StateWorkflowMismatch = 174,
        /// The escrow state is inconsistent with the compliance.
        StateComplianceMismatch = 175,
        /// The escrow state is inconsistent with the treasury.
        StateTreasuryMismatch = 176,
        /// The escrow state is inconsistent with the invoice.
        StateInvoiceMismatch = 177,
        /// The escrow state is inconsistent with the settlement.
        StateSettlementMismatch = 178,
        /// The escrow state is inconsistent with the dispute resolution.
        StateResolutionMismatch = 179,
        /// The escrow state is inconsistent with the timelock.
        StateTimelockMismatch = 180,
        /// The escrow state is inconsistent with the cooldown.
        StateCooldownMismatch = 181,
        /// The escrow state is inconsistent with the nonce.
        StateNonceMismatch = 182,
        /// The escrow state is inconsistent with the reference id.
        StateReferenceIdMismatch = 183,
        /// The escrow state is inconsistent with the hash.
        StateHashMismatch = 184,
        /// The escrow state is inconsistent with the memo.
        StateMemoMismatch = 185,
        /// The escrow state is inconsistent with the label.
        StateLabelMismatch = 186,
        /// The escrow state is inconsistent with the batch.
        StateBatchMismatch = 187,
        /// The escrow state is inconsistent with the template.
        StateTemplateMismatch = 188,
        /// The escrow state is inconsistent with the metadata.
        StateMetadataMismatch = 189,
        /// The escrow state is inconsistent with the payment.
        StatePaymentMismatch = 190,
        /// The escrow state is inconsistent with the refund request.
        StateRefundRequestMismatch = 191,
        /// The escrow state is inconsistent with the release request.
        StateReleaseRequestMismatch = 192,
        /// The escrow state is inconsistent with the dispute request.
        StateDisputeRequestMismatch = 193,
        /// The escrow state is inconsistent with the cancellation request.
        StateCancellationRequestMismatch = 194,
        /// The escrow state is inconsistent with the hold request.
        StateHoldRequestMismatch = 195,
        /// The escrow state is inconsistent with the pause request.
        StatePauseRequestMismatch = 196,
        /// The escrow state is inconsistent with the admin request.
        StateAdminRequestMismatch = 197,
        /// The escrow state is inconsistent with the signer request.
        StateSignerRequestMismatch = 198,
        /// The escrow state is inconsistent with the threshold request.
        StateThresholdRequestMismatch = 199,
        /// The escrow state is inconsistent with the rotation request.
        StateRotationRequestMismatch = 200,
    }
}

/// Unified error type spanning all COMEBACKHERE contracts.
///
/// Integration clients and cross-contract tests can import this single type
/// and handle errors from any contract with one `match` arm.
///
/// This crate must never be a dependency of a contract crate itself (only of
/// off-chain clients and cross-contract test fixtures) - see the
/// settlement-workflow link failure this repo hit from exactly that mistake.
/// `protocol-errors` exists to sit outside the on-chain dependency graph, not
/// inside it, and that stays true even once this crate depends on nothing but
/// the four error enums themselves.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ProtocolError {
    Invoice(InvoiceError),
    Treasury(TreasuryError),
    Compliance(ComplianceError),
    Workflow(WorkflowError),
    EscrowVault(EscrowVaultError),
}

impl From<InvoiceError> for ProtocolError {
    fn from(e: InvoiceError) -> Self {
        ProtocolError::Invoice(e)
    }
}

impl From<TreasuryError> for ProtocolError {
    fn from(e: TreasuryError) -> Self {
        ProtocolError::Treasury(e)
    }
}

impl From<ComplianceError> for ProtocolError {
    fn from(e: ComplianceError) -> Self {
        ProtocolError::Compliance(e)
    }
}

impl From<WorkflowError> for ProtocolError {
    fn from(e: WorkflowError) -> Self {
        ProtocolError::Workflow(e)
    }
}

impl From<EscrowVaultError> for ProtocolError {
    fn from(e: EscrowVaultError) -> Self {
        ProtocolError::EscrowVault(e)
    }
}

impl ProtocolError {
    /// Returns the originating contract name as a static string slice.
    pub fn contract_name(&self) -> &'static str {
        match self {
            ProtocolError::Invoice(_) => "invoice",
            ProtocolError::Treasury(_) => "treasury",
            ProtocolError::Compliance(_) => "compliance",
            ProtocolError::Workflow(_) => "settlement-workflow",
            ProtocolError::EscrowVault(_) => "escrow-vault",
        }
    }

    /// Returns a short, user-facing description of the error.
    pub fn description(&self) -> &'static str {
        match self {
            ProtocolError::Invoice(e) => invoice_description(*e),
            ProtocolError::Treasury(e) => treasury_description(*e),
            ProtocolError::Compliance(e) => compliance_description(*e),
            ProtocolError::Workflow(e) => workflow_description(*e),
            ProtocolError::EscrowVault(e) => escrow_vault_description(*e),
        }
    }
}

// Exhaustive matches: adding a variant without a description fails to compile.
fn invoice_description(e: InvoiceError) -> &'static str {
    use InvoiceError::*;
    match e {
        Unauthorized => "Caller is not authorized for this action.",
        ContractPaused => "The invoice contract is paused.",
        InvalidAmount => "The invoice amount is invalid.",
        NotPending => "The invoice is not pending.",
        Expired => "The invoice has expired.",
        NotFound => "Invoice not found.",
        AlreadyInitialized => "The invoice contract is already initialized.",
        ZeroDuration => "The invoice duration must be greater than zero.",
        ExpiryOverflow => "The invoice expiry time is out of range.",
        NotPaid => "The invoice has not been paid.",
        NotReleased => "The invoice escrow has not been released.",
        AmountPrecision => "The amount has too many decimal places.",
        DuplicateNonce => "This invoice nonce has already been used.",
        ExpiryTooLong => "The invoice expiry is too far in the future.",
        MetadataMismatch => "The invoice metadata does not match.",
        NoPendingAdmin => "There is no pending admin transfer.",
        InvalidPaymentLinkHash => "The payment link hash is invalid.",
        NotRefundRequested => "No refund has been requested for this invoice.",
        TokenMismatch => "The payment token does not match the invoice.",
        BatchTooLarge => "Too many items in one batch.",
        CooldownActive => "Please wait before trying this action again.",
        InvoiceCountOverflow => "The invoice limit has been reached.",
        HashTooLong => "The provided hash is too long.",
        PaymentStateInconsistent => "The invoice does not describe a completed payment.",
        RefundTransferFailed => "The refund transfer failed.",
        RefundTokenNotSet => "The invoice has no token to refund through.",
        RefundFeeTooHigh => "The refund fee is too high.",
        ArithmeticOverflow => "The calculation overflowed.",
        DiscountExceedsAmount => "The discount amount exceeds the invoice amount.",
        DiscountDeadlineAfterExpiry => "The discount deadline is after the invoice expiry.",
        MemoTooLong => "The memo field is too long.",
        RefundWindowClosed => "The refund window for this invoice has closed.",
        SelfPayNotAllowed => "A merchant cannot pay its own invoice.",
        TemplateDisabled => "The invoice template is disabled.",
        InvalidLateFee => "The late fee exceeds 100%.",
    }
}

fn treasury_description(e: TreasuryError) -> &'static str {
    use TreasuryError::*;
    match e {
        AlreadyInitialized => "The treasury contract is already initialized.",
        ZeroThreshold => "The signer threshold must be greater than zero.",
        SettlementNotFound => "Settlement not found.",
        AlreadyExecuted => "The settlement has already been executed.",
        ThresholdNotMet => "Not enough signer approvals yet.",
        ThresholdNotConfigured => "The signer threshold is not configured.",
        InvalidAmount => "The amount is invalid.",
        ContractPaused => "The treasury contract is paused.",
        Unauthorized => "Caller is not authorized for this action.",
        UnauthorizedSigner => "Caller is not an authorized signer.",
        InvalidTokenContract => "The token contract is invalid.",
        TokenNotAllowed => "This token is not allowed.",
        RotationNotFound => "Signer rotation not found.",
        RotationAlreadyExecuted => "The signer rotation has already been executed.",
        SettlementOnHold => "The settlement is on hold.",
        DisputeNotExpired => "The dispute window has not ended yet.",
        AlreadyOnHold => "The settlement is already on hold.",
        ThresholdUnreachable => "The threshold exceeds the available signer weight.",
        ComplianceCheckFailed => "The address failed the compliance check.",
        ArithmeticOverflow => "The calculation overflowed.",
        DisputeNotFound => "Dispute not found.",
        DisputeAlreadyResolved => "The dispute has already been resolved.",
        ResolutionDirectionMismatch => "The dispute resolution direction is invalid.",
        BatchTooLarge => "Too many items in one batch.",
        WeightOverflow => "The total signer weight is too large.",
        SettlementNotCancellable => "The settlement cannot be cancelled.",
        TtlNotElapsed => "The settlement has not expired yet.",
        AllowlistFull => "The allowlist is full.",
        NotOnHold => "The settlement is not on hold.",
        DestinationNotAllowed => "The destination address is not allowed.",
        InsufficientBalance => "Insufficient treasury balance.",
        NotPaused => "The treasury contract is not paused.",
        RotationProposalCooldown => "Please wait before proposing another signer rotation.",
        WorkflowNotRegisteredSigner => "The settlement workflow is not a registered signer.",
        WithdrawalLimitExceeded => "The withdrawal limit has been exceeded.",
        InvalidSplitRatio => "The dispute split ratio is invalid.",
        ForceCancelNotAllowed => "The settlement cannot be force-cancelled.",
        SignerChangeTooEarly => "The signer change delay has not elapsed yet.",
        SignerChangeNotFound => "Signer change not found.",
        SignerChangeAlreadyFinalised => "The signer change has already been finalised.",
        ExecutionDeadlineExceeded => "The settlement execution deadline has passed.",
        QuorumBreak => "Removing this signer would make the threshold unreachable.",
        TokenHasPendingSettlements => "The token still has pending settlements.",
        ApprovalNotFound => "The signer has not approved this settlement.",
        DuplicateVote => "The signer has already voted on this dispute.",
        NoPendingAdmin => "There is no pending admin transfer.",
        LabelTooLong => "The signer label is too long.",
        InvalidTimelockDelay => "The timelock delay is outside the allowed bounds.",
        DuplicateReferenceId => "This deposit reference id was already used recently.",
        TokenHasBalance => "The token still has depositor balances and cannot be removed.",
    }
}

fn compliance_description(e: ComplianceError) -> &'static str {
    match e {
        ComplianceError::AlreadyInitialized => "The compliance contract is already initialized.",
    }
}

fn workflow_description(e: WorkflowError) -> &'static str {
    use WorkflowError::*;
    match e {
        Unauthorized => "Caller is not the emergency-pause admin.",
        NotConfigured => "Emergency pause has not been configured.",
        PauseTargetFailed => "A target refused to pause; no targets were paused.",
        UnpauseTargetFailed => "A target refused to unpause; no targets were resumed.",
        NoPauseTargets => "An empty pause target set was supplied.",
        TooManyPauseTargets => "Too many pause targets supplied.",
        DuplicatePauseTarget => "The pause target list contains a duplicate address.",
        SelfPauseTarget => "The workflow contract cannot be its own pause target.",
        EmergencyPauseActive => "Cannot reconfigure targets while an emergency pause is active.",
        NotEmergencyPaused => "No emergency pause is currently active.",
    }
}

fn escrow_vault_description(e: EscrowVaultError) -> &'static str {
    use EscrowVaultError::*;
    match e {
        AlreadyInitialized => "The escrow-vault contract is already initialized.",
        NotInitialized => "The escrow-vault contract has not been initialized yet.",
        Unauthorized => "Caller is not authorized for this action.",
        ContractPaused => "The escrow-vault contract is paused.",
        NotPaused => "The escrow-vault contract is not paused.",
        InvalidAmount => "The amount is invalid.",
        EscrowNotFound => "Escrow not found.",
        InvalidEscrowState => "The escrow is not in the state required by this action.",
        AlreadyFunded => "The escrow has already been funded.",
        NotFunded => "The escrow has not been funded yet.",
        AlreadyReleased => "The escrow has already been released.",
        NotReleased => "The escrow has not been released yet.",
        AlreadyRefunded => "The escrow has already been refunded.",
        NotRefunded => "The escrow has not been refunded yet.",
        EscrowExpired => "The escrow has expired.",
        EscrowNotExpired => "The escrow has not expired yet.",
        InvalidDeadline => "The escrow deadline is invalid.",
        InvalidToken => "The token contract is invalid.",
        TokenMismatch => "The token does not match the escrow.",
        InvalidBeneficiary => "The beneficiary address is invalid.",
        InvalidDepositor => "The depositor address is invalid.",
        InvalidArbiter => "The arbiter address is invalid.",
        NotArbiter => "Caller is not the arbiter of this escrow.",
        NotDepositor => "Caller is not the depositor of this escrow.",
        NotBeneficiary => "Caller is not the beneficiary of this escrow.",
        DisputeAlreadyRaised => "A dispute has already been raised for this escrow.",
        DisputeNotRaised => "No dispute has been raised for this escrow.",
        DisputeAlreadyResolved => "The dispute has already been resolved.",
        InvalidResolution => "The dispute resolution direction is invalid.",
        DisputeWindowActive => "The dispute window has not ended yet.",
        FeeTooHigh => "The fee exceeds the allowed maximum.",
        InvalidFeeRecipient => "The fee recipient address is invalid.",
        RefundWindowClosed => "The refund window has closed.",
        ReleaseWindowClosed => "The release window has closed.",
        InvalidHash => "The provided hash is invalid.",
        HashTooLong => "The provided hash is too long.",
        MemoTooLong => "The memo field is too long.",
        LabelTooLong => "The label is too long.",
        BatchTooLarge => "Too many items in one batch.",
        DuplicateEntry => "The batch contains duplicate entries.",
        DuplicateNonce => "This nonce has already been used.",
        DuplicateReferenceId => "This reference id has already been used.",
        EscrowCountOverflow => "The escrow limit has been reached.",
        ArithmeticOverflow => "The calculation overflowed.",
        ArithmeticUnderflow => "The calculation underflowed.",
        DivisionByZero => "A division by zero was attempted.",
        AmountPrecision => "The amount has too many decimal places.",
        InsufficientBalance => "The escrow balance is insufficient.",
        TransferFailed => "The token transfer failed.",
        RefundTransferFailed => "The refund transfer failed.",
        RefundTokenNotSet => "The escrow has no token to refund through.",
        NoPendingAdmin => "There is no pending admin transfer.",
        InvalidTimelockDelay => "The timelock delay is outside the allowed bounds.",
        TimelockNotElapsed => "The timelock has not elapsed yet.",
        ExecutionDeadlineExceeded => "The execution deadline has passed.",
        NotCancellable => "The escrow cannot be cancelled in its current state.",
        ForceCancelNotAllowed => "The escrow cannot be force-cancelled.",
        EscrowOnHold => "The escrow is on hold.",
        NotOnHold => "The escrow is not on hold.",
        DestinationNotAllowed => "The destination address is not allowed.",
        ComplianceCheckFailed => "The address failed the compliance check.",
        AlreadyInState => "The contract is already in the requested state.",
        NotInState => "The contract is not in the required state.",
        UnsupportedOperation => "The operation is not supported.",
        InvalidConfiguration => "The supplied configuration is invalid.",
        StorageInconsistent => "The contract storage is inconsistent.",
        SelfEscrowNotAllowed => "A depositor cannot be its own beneficiary.",
        MetadataMismatch => "The escrow metadata does not match.",
        TemplateDisabled => "The escrow template is disabled.",
        NoPendingRelease => "There is no pending release.",
        NoPendingRefund => "There is no pending refund.",
        NoPendingDispute => "There is no pending dispute.",
        NoPendingTransfer => "There is no pending admin transfer.",
        NoPendingSignerChange => "There is no pending signer change.",
        SignerChangeTooEarly => "The signer change delay has not elapsed yet.",
        SignerChangeAlreadyFinalised => "The signer change has already been finalised.",
        SignerChangeNotFound => "Signer change not found.",
        ThresholdNotConfigured => "The signer threshold is not configured.",
        ThresholdUnreachable => "The threshold exceeds the available signer weight.",
        ThresholdNotMet => "Not enough signer approvals yet.",
        ApprovalNotFound => "The signer has not approved this action.",
        DuplicateVote => "The signer has already voted on this action.",
        LabelOverflow => "The signer label is too long.",
        WeightOverflow => "The total signer weight is too large.",
        AllowlistFull => "The allowlist is full.",
        TokenHasPendingEscrows => "The token still has pending escrows.",
        TokenHasBalance => "The token still has depositor balances.",
        WithdrawalLimitExceeded => "The withdrawal limit has been exceeded.",
        InvalidSplitRatio => "The dispute split ratio is invalid.",
        InvalidLateFee => "The late fee exceeds the allowed maximum.",
        DiscountExceedsAmount => "The discount exceeds the escrow amount.",
        DiscountDeadlineAfterExpiry => "The discount deadline is after the escrow expiry.",
        RefundFeeTooHigh => "The refund fee is too high.",
        PaymentStateInconsistent => "The escrow does not describe a completed payment.",
        PaymentTokenMismatch => "The payment token does not match the escrow.",
        InvalidPaymentLinkHash => "The payment link hash is invalid.",
        NotRefundRequested => "No refund has been requested for this escrow.",
        NotPaid => "The escrow has not been paid.",
        NotPending => "The escrow is not pending.",
        Expired => "The escrow has expired.",
        ZeroDuration => "The escrow duration must be greater than zero.",
        ExpiryOverflow => "The escrow expiry time is out of range.",
        ExpiryTooLong => "The escrow expiry is too far in the future.",
        CooldownActive => "Please wait before trying this action again.",
        WorkflowNotRegisteredSigner => "The settlement workflow is not a registered signer.",
        SettlementOnHold => "The settlement is on hold.",
        DisputeNotExpired => "The dispute window has not ended yet.",
        SettlementNotCancellable => "The settlement cannot be cancelled.",
        TtlNotElapsed => "The settlement has not expired yet.",
        AlreadyExecuted => "The settlement has already been executed.",
        AlreadyOnHold => "The settlement is already on hold.",
        SettlementNotFound => "Settlement not found.",
        DisputeNotFound => "Dispute not found.",
        RotationNotFound => "Signer rotation not found.",
        RotationAlreadyExecuted => "The signer rotation has already been executed.",
        RotationProposalCooldown => "Please wait before proposing another signer rotation.",
        UnauthorizedSigner => "Caller is not an authorized signer.",
        TokenNotAllowed => "This token is not allowed.",
        InvalidTokenContract => "The token contract is invalid.",
        ZeroThreshold => "The signer threshold must be greater than zero.",
        ResolutionDirectionMismatch => "The dispute resolution direction is invalid.",
        QuorumBreak => "Removing this signer would make the threshold unreachable.",
        AlreadyApproved => "The signer has already approved this action.",
        AlreadyInRequestedState => "The escrow is already in the requested state.",
        NotInRequestedState => "The escrow is not in the requested state.",
        NoPendingAction => "There is no pending action.",
        PendingActionExists => "A pending action must be resolved first.",
        AlreadyFinalized => "The escrow has already been finalized.",
        NotFinalized => "The escrow has not been finalized yet.",
        Locked => "The escrow is locked.",
        NotLocked => "The escrow is not locked.",
        Closed => "The escrow is closed.",
        NotClosed => "The escrow is not closed.",
        Archived => "The escrow is archived.",
        NotArchived => "The escrow is not archived.",
        Frozen => "The escrow is frozen.",
        NotFrozen => "The escrow is not frozen.",
        Suspended => "The escrow is suspended.",
        NotSuspended => "The escrow is not suspended.",
        Disabled => "The escrow is disabled.",
        NotDisabled => "The escrow is not disabled.",
        Active => "The escrow is active.",
        NotActive => "The escrow is not active.",
        UnknownState => "The escrow is in an unknown state.",
        InvalidStateTransition => "The escrow state transition is invalid.",
        StateInconsistent => "The escrow state is inconsistent.",
        StateCorrupted => "The escrow state is corrupted.",
        StateMissing => "The escrow state is missing.",
        StateStale => "The escrow state is stale.",
        StateOutdated => "The escrow state is outdated.",
        StateUnsupported => "The escrow state is unsupported.",
        StateInvalid => "The escrow state is invalid.",
        StateUnknown => "The escrow state is unknown.",
        StateAmbiguous => "The escrow state is ambiguous.",
        StateConflicting => "The escrow state is conflicting.",
        StateLedgerMismatch => "The escrow state is inconsistent with the ledger.",
        StateTokenMismatch => "The escrow state is inconsistent with the token.",
        StateBeneficiaryMismatch => "The escrow state is inconsistent with the beneficiary.",
        StateDepositorMismatch => "The escrow state is inconsistent with the depositor.",
        StateArbiterMismatch => "The escrow state is inconsistent with the arbiter.",
        StateAmountMismatch => "The escrow state is inconsistent with the amount.",
        StateDeadlineMismatch => "The escrow state is inconsistent with the deadline.",
        StateFeeMismatch => "The escrow state is inconsistent with the fee.",
        StateDisputeMismatch => "The escrow state is inconsistent with the dispute.",
        StateReleaseMismatch => "The escrow state is inconsistent with the release.",
        StateRefundMismatch => "The escrow state is inconsistent with the refund.",
        StateCancellationMismatch => "The escrow state is inconsistent with the cancellation.",
        StateHoldMismatch => "The escrow state is inconsistent with the hold.",
        StatePauseMismatch => "The escrow state is inconsistent with the pause.",
        StateAdminMismatch => "The escrow state is inconsistent with the admin.",
        StateSignerMismatch => "The escrow state is inconsistent with the signer.",
        StateThresholdMismatch => "The escrow state is inconsistent with the threshold.",
        StateRotationMismatch => "The escrow state is inconsistent with the rotation.",
        StateWorkflowMismatch => "The escrow state is inconsistent with the workflow.",
        StateComplianceMismatch => "The escrow state is inconsistent with the compliance.",
        StateTreasuryMismatch => "The escrow state is inconsistent with the treasury.",
        StateInvoiceMismatch => "The escrow state is inconsistent with the invoice.",
        StateSettlementMismatch => "The escrow state is inconsistent with the settlement.",
        StateResolutionMismatch => "The escrow state is inconsistent with the dispute resolution.",
        StateTimelockMismatch => "The escrow state is inconsistent with the timelock.",
        StateCooldownMismatch => "The escrow state is inconsistent with the cooldown.",
        StateNonceMismatch => "The escrow state is inconsistent with the nonce.",
        StateReferenceIdMismatch => "The escrow state is inconsistent with the reference id.",
        StateHashMismatch => "The escrow state is inconsistent with the hash.",
        StateMemoMismatch => "The escrow state is inconsistent with the memo.",
        StateLabelMismatch => "The escrow state is inconsistent with the label.",
        StateBatchMismatch => "The escrow state is inconsistent with the batch.",
        StateTemplateMismatch => "The escrow state is inconsistent with the template.",
        StateMetadataMismatch => "The escrow state is inconsistent with the metadata.",
        StatePaymentMismatch => "The escrow state is inconsistent with the payment.",
        StateRefundRequestMismatch => "The escrow state is inconsistent with the refund request.",
        StateReleaseRequestMismatch => "The escrow state is inconsistent with the release request.",
        StateDisputeRequestMismatch => "The escrow state is inconsistent with the dispute request.",
        StateCancellationRequestMismatch => "The escrow state is inconsistent with the cancellation request.",
        StateHoldRequestMismatch => "The escrow state is inconsistent with the hold request.",
        StatePauseRequestMismatch => "The escrow state is inconsistent with the pause request.",
        StateAdminRequestMismatch => "The escrow state is inconsistent with the admin request.",
        StateSignerRequestMismatch => "The escrow state is inconsistent with the signer request.",
        StateThresholdRequestMismatch => "The escrow state is inconsistent with the threshold request.",
        StateRotationRequestMismatch => "The escrow state is inconsistent with the rotation request.",
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_invoice_error() {
        let e = ProtocolError::from(InvoiceError::NotFound);
        assert_eq!(e, ProtocolError::Invoice(InvoiceError::NotFound));
        assert_eq!(e.contract_name(), "invoice");
    }

    #[test]
    fn from_treasury_error() {
        let e = ProtocolError::from(TreasuryError::SettlementNotFound);
        assert_eq!(
            e,
            ProtocolError::Treasury(TreasuryError::SettlementNotFound)
        );
        assert_eq!(e.contract_name(), "treasury");
    }

    #[test]
    fn from_compliance_error() {
        let e = ProtocolError::from(ComplianceError::AlreadyInitialized);
        assert_eq!(
            e,
            ProtocolError::Compliance(ComplianceError::AlreadyInitialized)
        );
        assert_eq!(e.contract_name(), "compliance");
    }

    #[test]
    fn into_coercion_from_invoice() {
        let e: ProtocolError = InvoiceError::Unauthorized.into();
        assert_eq!(e, ProtocolError::Invoice(InvoiceError::Unauthorized));
    }

    #[test]
    fn into_coercion_from_treasury() {
        let e: ProtocolError = TreasuryError::ThresholdNotMet.into();
        assert_eq!(e, ProtocolError::Treasury(TreasuryError::ThresholdNotMet));
    }

    #[test]
    fn question_mark_operator() {
        fn fallible_invoice() -> Result<(), ProtocolError> {
            Err(InvoiceError::Expired)?
        }
        fn fallible_treasury() -> Result<(), ProtocolError> {
            Err(TreasuryError::UnauthorizedSigner)?
        }
        assert_eq!(
            fallible_invoice().unwrap_err(),
            ProtocolError::Invoice(InvoiceError::Expired)
        );
        assert_eq!(
            fallible_treasury().unwrap_err(),
            ProtocolError::Treasury(TreasuryError::UnauthorizedSigner)
        );
    }
}
