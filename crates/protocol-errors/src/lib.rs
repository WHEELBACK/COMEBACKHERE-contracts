#![no_std]

pub use compliance_errors::ComplianceError;
pub use invoice_errors::InvoiceError;
pub use oracle_adapter_errors::OracleAdapterError;
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
    OracleAdapter(OracleAdapterError),
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

impl From<OracleAdapterError> for ProtocolError {
    fn from(e: OracleAdapterError) -> Self {
        ProtocolError::OracleAdapter(e)
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
            ProtocolError::OracleAdapter(_) => "oracle-adapter",
        }
    }

    /// Returns a short, user-facing description of the error.
    pub fn description(&self) -> &'static str {
        match self {
            ProtocolError::Invoice(e) => invoice_description(*e),
            ProtocolError::Treasury(e) => treasury_description(*e),
            ProtocolError::Compliance(e) => compliance_description(*e),
            ProtocolError::Workflow(e) => workflow_description(*e),
            ProtocolError::OracleAdapter(e) => e.message(),
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

/// Errors raised by the `oracle-adapter` contract.
///
/// Discriminants are documented and stable so off-chain clients can map a
/// numeric `u32` returned by the contract back to a specific failure without
/// depending on the contract crate itself.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum OracleAdapterError {
    /// 0 - The contract has already been initialized.
    AlreadyInitialized = 0,
    /// 1 - The contract has not been initialized yet.
    NotInitialized = 1,
    /// 2 - Caller is not authorized for this action.
    Unauthorized = 2,
    /// 3 - The contract is paused.
    ContractPaused = 3,
    /// 4 - The supplied oracle address is invalid.
    InvalidOracle = 4,
    /// 5 - The supplied feed id is invalid.
    InvalidFeedId = 5,
    /// 6 - The feed id is not registered.
    FeedNotFound = 6,
    /// 7 - The feed has already been registered.
    FeedAlreadyRegistered = 7,
    /// 8 - The reported price is invalid (zero or out of bounds).
    InvalidPrice = 8,
    /// 9 - The reported timestamp is in the future or otherwise invalid.
    InvalidTimestamp = 9,
    /// 10 - The price data is stale.
    StalePrice = 10,
    /// 11 - The price data is older than the requested observation.
    PriceTooOld = 11,
    /// 12 - The price deviation exceeds the configured tolerance.
    PriceDeviationTooHigh = 12,
    /// 13 - The configured deviation tolerance is invalid.
    InvalidDeviation = 13,
    /// 14 - The configured staleness threshold is invalid.
    InvalidStalenessThreshold = 14,
    /// 15 - The feed has no recorded price yet.
    NoPriceAvailable = 15,
    /// 16 - The number of decimals is out of the supported range.
    InvalidDecimals = 16,
    /// 17 - The calculation overflowed.
    ArithmeticOverflow = 17,
    /// 18 - The requested operation is not supported.
    UnsupportedOperation = 18,
    /// 19 - The oracle reported a failure.
    OracleFailure = 19,
    /// 20 - The supplied configuration is invalid.
    InvalidConfiguration = 20,
    /// 21 - Too many items in one batch.
    BatchTooLarge = 21,
    /// 22 - The feed is disabled.
    FeedDisabled = 22,
    /// 23 - The observation window is invalid.
    InvalidObservationWindow = 23,
    /// 24 - The price has not changed since the last update.
    PriceUnchanged = 24,
}

impl OracleAdapterError {
    /// Returns a short, user-facing description of the error.
    ///
    /// Exhaustive match: adding a variant without a description fails to
    /// compile, keeping client-facing messages in sync with the enum.
    pub fn message(&self) -> &'static str {
        use OracleAdapterError::*;
        match self {
            AlreadyInitialized => "The oracle adapter is already initialized.",
            NotInitialized => "The oracle adapter has not been initialized.",
            Unauthorized => "Caller is not authorized for this action.",
            ContractPaused => "The oracle adapter is paused.",
            InvalidOracle => "The oracle address is invalid.",
            InvalidFeedId => "The feed id is invalid.",
            FeedNotFound => "The feed is not registered.",
            FeedAlreadyRegistered => "The feed is already registered.",
            InvalidPrice => "The reported price is invalid.",
            InvalidTimestamp => "The reported timestamp is invalid.",
            StalePrice => "The price data is stale.",
            PriceTooOld => "The price data is older than the requested observation.",
            PriceDeviationTooHigh => "The price deviation exceeds the configured tolerance.",
            InvalidDeviation => "The deviation tolerance is invalid.",
            InvalidStalenessThreshold => "The staleness threshold is invalid.",
            NoPriceAvailable => "No price is available for this feed yet.",
            InvalidDecimals => "The number of decimals is out of range.",
            ArithmeticOverflow => "The calculation overflowed.",
            UnsupportedOperation => "The requested operation is not supported.",
            OracleFailure => "The oracle reported a failure.",
            InvalidConfiguration => "The supplied configuration is invalid.",
            BatchTooLarge => "Too many items in one batch.",
            FeedDisabled => "The feed is disabled.",
            InvalidObservationWindow => "The observation window is invalid.",
            PriceUnchanged => "The price has not changed since the last update.",
        }
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
    fn from_oracle_adapter_error() {
        let e = ProtocolError::from(OracleAdapterError::StalePrice);
        assert_eq!(e, ProtocolError::OracleAdapter(OracleAdapterError::StalePrice));
        assert_eq!(e.contract_name(), "oracle-adapter");
        assert_eq!(e.description(), "The price data is stale.");
    }

    #[test]
    fn oracle_adapter_messages_are_non_empty() {
        use OracleAdapterError::*;
        let all = [
            AlreadyInitialized,
            NotInitialized,
            Unauthorized,
            ContractPaused,
            InvalidOracle,
            InvalidFeedId,
            FeedNotFound,
            FeedAlreadyRegistered,
            InvalidPrice,
            InvalidTimestamp,
            StalePrice,
            PriceTooOld,
            PriceDeviationTooHigh,
            InvalidDeviation,
            InvalidStalenessThreshold,
            NoPriceAvailable,
            InvalidDecimals,
            ArithmeticOverflow,
            UnsupportedOperation,
            OracleFailure,
            InvalidConfiguration,
            BatchTooLarge,
            FeedDisabled,
            InvalidObservationWindow,
            PriceUnchanged,
        ];
        for e in all {
            assert!(!e.message().is_empty());
            assert_eq!(
                ProtocolError::OracleAdapter(e).description(),
                e.message()
            );
        }
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
