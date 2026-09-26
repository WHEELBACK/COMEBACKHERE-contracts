#![no_std]

pub use compliance_errors::ComplianceError;
pub use invoice_errors::InvoiceError;
pub use treasury::TreasuryError;

/// Unified error type spanning all three COMEBACKHERE contracts.
///
/// Integration clients and cross-contract tests can import this single type
/// and handle errors from any contract with one `match` arm.
///
/// This crate must never be a dependency of a contract crate itself (only of
/// off-chain clients and cross-contract test fixtures) - see the
/// settlement-workflow link failure this repo hit from exactly that mistake.
/// `protocol-errors` exists to sit outside the on-chain dependency graph, not
/// inside it, and that stays true even once this crate depends on nothing but
/// the three error enums themselves.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ProtocolError {
    Invoice(InvoiceError),
    Treasury(TreasuryError),
    Compliance(ComplianceError),
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

impl ProtocolError {
    /// Returns the originating contract name as a static string slice.
    pub fn contract_name(&self) -> &'static str {
        match self {
            ProtocolError::Invoice(_) => "invoice",
            ProtocolError::Treasury(_) => "treasury",
            ProtocolError::Compliance(_) => "compliance",
        }
    }

    /// Returns a short, user-facing description of the error.
    pub fn description(&self) -> &'static str {
        match self {
            ProtocolError::Invoice(e) => invoice_description(*e),
            ProtocolError::Treasury(e) => treasury_description(*e),
            ProtocolError::Compliance(e) => compliance_description(*e),
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
    }
}

fn compliance_description(e: ComplianceError) -> &'static str {
    match e {
        ComplianceError::AlreadyInitialized => "The compliance contract is already initialized.",
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
