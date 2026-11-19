#![no_std]

pub use compliance_errors::ComplianceError;
pub use event_emitter_errors::EventEmitterError;
pub use invoice_errors::InvoiceError;
pub use treasury::TreasuryError;
pub use workflow_errors::WorkflowError;

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
    EventEmitter(EventEmitterError),
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

impl From<EventEmitterError> for ProtocolError {
    fn from(e: EventEmitterError) -> Self {
        ProtocolError::EventEmitter(e)
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
            ProtocolError::EventEmitter(_) => "event-emitter",
        }
    }

    /// Returns a short, user-facing description of the error.
    pub fn description(&self) -> &'static str {
        match self {
            ProtocolError::Invoice(e) => invoice_description(*e),
            ProtocolError::Treasury(e) => treasury_description(*e),
            ProtocolError::Compliance(e) => compliance_description(*e),
            ProtocolError::Workflow(e) => workflow_description(*e),
            ProtocolError::EventEmitter(e) => event_emitter_description(*e),
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

fn event_emitter_description(e: EventEmitterError) -> &'static str {
    use EventEmitterError::*;
    match e {
        AlreadyInitialized => "The event-emitter contract is already initialized.",
        Unauthorized => "Caller is not authorized for this action.",
        ContractPaused => "The event-emitter contract is paused.",
        NotPaused => "The event-emitter contract is not paused.",
        InvalidEventName => "The event name is invalid.",
        EventNameTooLong => "The event name is too long.",
        InvalidTopic => "The event topic is invalid.",
        TopicTooLong => "The event topic is too long.",
        TooManyTopics => "Too many topics supplied for one event.",
        PayloadTooLarge => "The event payload is too large.",
        InvalidPayload => "The event payload is invalid.",
        NoPendingAdmin => "There is no pending admin transfer.",
        ArithmeticOverflow => "The calculation overflowed.",
        BatchTooLarge => "Too many items in one batch.",
        DuplicateSubscription => "The subscription already exists.",
        SubscriptionNotFound => "Subscription not found.",
        SubscriptionLimitReached => "The subscription limit has been reached.",
        InvalidSubscriptionFilter => "The subscription filter is invalid.",
        TtlNotElapsed => "The cooldown has not elapsed yet.",
        InvalidTtl => "The TTL value is outside the allowed bounds.",
        EventNotFound => "Event not found.",
        EventExpired => "The event has expired.",
        SequenceOverflow => "The event sequence number overflowed.",
        InvalidSequence => "The event sequence number is invalid.",
        ReplayDetected => "This event has already been processed.",
        InvalidSignature => "The event signature is invalid.",
        SignatureVerificationFailed => "The event signature could not be verified.",
        InvalidEmitter => "The emitting contract is not allowed.",
        EmitterNotAllowed => "The emitter address is not on the allowlist.",
        AllowlistFull => "The emitter allowlist is full.",
        NotOnAllowlist => "The emitter address is not on the allowlist.",
        InvalidConfig => "The event-emitter configuration is invalid.",
        ConfigLocked => "The configuration is locked and cannot be changed.",
        InvalidPageSize => "The requested page size is invalid.",
        PageOutOfRange => "The requested page is out of range.",
        InvalidTimestamp => "The timestamp is invalid.",
        TimestampInFuture => "The timestamp is in the future.",
        InvalidLedger => "The ledger sequence is invalid.",
        LedgerTooOld => "The ledger sequence is too old.",
        InvalidVersion => "The event schema version is invalid.",
        UnsupportedVersion => "The event schema version is not supported.",
        InvalidSchema => "The event schema is invalid.",
        SchemaMismatch => "The event schema does not match.",
        InvalidFilter => "The event filter is invalid.",
        FilterTooComplex => "The event filter is too complex.",
        InvalidCursor => "The pagination cursor is invalid.",
        CursorExpired => "The pagination cursor has expired.",
        StorageFull => "The event storage is full.",
        StorageCorrupt => "The stored event data is corrupt.",
        InvalidState => "The contract is in an invalid state for this action.",
        StateTransitionInvalid => "The requested state transition is invalid.",
        OperationNotAllowed => "This operation is not allowed.",
        OperationAlreadyCompleted => "This operation has already been completed.",
        OperationInProgress => "This operation is already in progress.",
        InvalidArgument => "An argument passed to the contract is invalid.",
        MissingArgument => "A required argument is missing.",
        InvalidAddress => "The address is invalid.",
        AddressNotAllowed => "The address is not allowed.",
        SelfReference => "The contract cannot reference itself here.",
        InvalidAmount => "The amount is invalid.",
        ZeroAmount => "The amount must be greater than zero.",
        InsufficientBalance => "Insufficient balance.",
        TokenNotAllowed => "This token is not allowed.",
        InvalidTokenContract => "The token contract is invalid.",
        TransferFailed => "The token transfer failed.",
        ExternalCallFailed => "An external contract call failed.",
        ReentrancyDetected => "A reentrancy attempt was detected.",
        DeadlineExceeded => "The operation deadline has passed.",
        NotYetActive => "The operation is not active yet.",
        AlreadyActive => "The operation is already active.",
        Cancelled => "The operation was cancelled.",
        NotCancellable => "The operation cannot be cancelled.",
        InvalidNonce => "The nonce is invalid.",
        DuplicateNonce => "This nonce has already been used.",
        InvalidProof => "The provided proof is invalid.",
        ProofVerificationFailed => "The proof could not be verified.",
        InvalidHash => "The provided hash is invalid.",
        HashMismatch => "The provided hash does not match.",
        InvalidMerkleRoot => "The Merkle root is invalid.",
        InvalidMerkleProof => "The Merkle proof is invalid.",
        InvalidRange => "The requested range is invalid.",
        RangeTooLarge => "The requested range is too large.",
        InvalidLimit => "The requested limit is invalid.",
        LimitExceeded => "The limit has been exceeded.",
        RateLimited => "Too many requests; please retry later.",
        QuotaExceeded => "The quota has been exceeded.",
        InvalidFee => "The fee is invalid.",
        FeeTooHigh => "The fee is too high.",
        InvalidPercentage => "The percentage is invalid.",
        PercentageOutOfRange => "The percentage is out of range.",
        InvalidDuration => "The duration is invalid.",
        ZeroDuration => "The duration must be greater than zero.",
        DurationTooLong => "The duration is too long.",
        InvalidInterval => "The interval is invalid.",
        InvalidThreshold => "The threshold is invalid.",
        ThresholdNotMet => "The threshold has not been met.",
        InvalidQuorum => "The quorum is invalid.",
        QuorumNotMet => "The quorum has not been met.",
        InvalidWeight => "The weight is invalid.",
        WeightOverflow => "The total weight is too large.",
        InvalidPriority => "The priority is invalid.",
        InvalidOrder => "The ordering is invalid.",
        OutOfOrder => "The items are out of order.",
        DuplicateEntry => "The entry already exists.",
        EntryNotFound => "The entry was not found.",
        EntryExpired => "The entry has expired.",
        EntryLocked => "The entry is locked.",
        EntryInUse => "The entry is currently in use.",
        InvalidEncoding => "The data encoding is invalid.",
        DecodingFailed => "The data could not be decoded.",
        EncodingFailed => "The data could not be encoded.",
        SerializationFailed => "The data could not be serialized.",
        DeserializationFailed => "The data could not be deserialized.",
        InvalidFormat => "The data format is invalid.",
        UnexpectedValue => "An unexpected value was encountered.",
        UnexpectedState => "An unexpected state was encountered.",
        InternalError => "An internal error occurred.",
        NotImplemented => "This operation is not implemented.",
        Unsupported => "This operation is not supported.",
        Deprecated => "This operation is deprecated.",
        Unknown => "An unknown error occurred.",
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
    fn from_event_emitter_error() {
        let e = ProtocolError::from(EventEmitterError::Unauthorized);
        assert_eq!(
            e,
            ProtocolError::EventEmitter(EventEmitterError::Unauthorized)
        );
        assert_eq!(e.contract_name(), "event-emitter");
    }

    #[test]
    fn into_coercion_from_event_emitter() {
        let e: ProtocolError = EventEmitterError::ContractPaused.into();
        assert_eq!(
            e,
            ProtocolError::EventEmitter(EventEmitterError::ContractPaused)
        );
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

    #[test]
    fn question_mark_operator_event_emitter() {
        fn fallible_event_emitter() -> Result<(), ProtocolError> {
            Err(EventEmitterError::Unauthorized)?
        }
        assert_eq!(
            fallible_event_emitter().unwrap_err(),
            ProtocolError::EventEmitter(EventEmitterError::Unauthorized)
        );
    }
}
