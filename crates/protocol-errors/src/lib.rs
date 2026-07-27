#![no_std]

pub use compliance::ComplianceError;
pub use invoice::InvoiceError;
pub use treasury::TreasuryError;

/// Unified error type spanning all three COMEBACKHERE contracts.
///
/// Integration clients and cross-contract tests can import this single type
/// and handle errors from any contract with one `match` arm.
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
        assert_eq!(e, ProtocolError::Treasury(TreasuryError::SettlementNotFound));
        assert_eq!(e.contract_name(), "treasury");
    }

    #[test]
    fn from_compliance_error() {
        let e = ProtocolError::from(ComplianceError::AlreadyInitialized);
        assert_eq!(e, ProtocolError::Compliance(ComplianceError::AlreadyInitialized));
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
