use protocol_errors::{ComplianceError, InvoiceError, ProtocolError, TreasuryError};

#[test]
fn all_contract_names_are_distinct() {
    let names = [
        ProtocolError::Invoice(InvoiceError::NotFound).contract_name(),
        ProtocolError::Treasury(TreasuryError::SettlementNotFound).contract_name(),
        ProtocolError::Compliance(ComplianceError::AlreadyInitialized).contract_name(),
    ];
    let unique: std::collections::HashSet<_> = names.iter().collect();
    assert_eq!(unique.len(), 3, "contract_name() values must be unique");
}

#[test]
fn round_trip_from_impls() {
    let pairs: &[(ProtocolError, &str)] = &[
        (InvoiceError::InvalidAmount.into(), "invoice"),
        (TreasuryError::ZeroThreshold.into(), "treasury"),
        (ComplianceError::AlreadyInitialized.into(), "compliance"),
    ];
    for (err, expected_name) in pairs {
        assert_eq!(err.contract_name(), *expected_name);
    }
}

#[test]
fn question_mark_propagation() {
    fn try_invoice() -> Result<(), ProtocolError> {
        Err(InvoiceError::NotPending)?
    }
    fn try_treasury() -> Result<(), ProtocolError> {
        Err(TreasuryError::AlreadyExecuted)?
    }
    fn try_compliance() -> Result<(), ProtocolError> {
        Err(ComplianceError::AlreadyInitialized)?
    }

    assert_eq!(
        try_invoice().unwrap_err(),
        ProtocolError::Invoice(InvoiceError::NotPending)
    );
    assert_eq!(
        try_treasury().unwrap_err(),
        ProtocolError::Treasury(TreasuryError::AlreadyExecuted)
    );
    assert_eq!(
        try_compliance().unwrap_err(),
        ProtocolError::Compliance(ComplianceError::AlreadyInitialized)
    );
}
