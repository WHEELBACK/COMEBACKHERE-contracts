use protocol_errors::{ComplianceError, InvoiceError, ProtocolError, TreasuryError};

/// Guards against silent ABI drift in `ProtocolError` itself, the same way
/// `contracts/treasury/tests/multisig_version_lock_test.rs` guards multisig's
/// ABI-relevant types for treasury.
///
/// `protocol-errors`' own doc comment describes `ProtocolError` as the type
/// off-chain integration clients are meant to use to handle errors from any
/// of this protocol's contracts. That makes its three-variant shape
/// (`Invoice`, `Treasury`, `Compliance`) part of the protocol's ABI surface,
/// but until now nothing in the workspace failed to compile if a variant
/// were removed, renamed, or a fourth contract's error type were added here
/// without review.
///
/// This match has no wildcard arm: adding, removing, or renaming a
/// `ProtocolError` variant fails this file to *compile*, forcing a
/// deliberate review here before the change ships.
fn assert_protocol_error_exhaustive(err: ProtocolError) {
    match err {
        ProtocolError::Invoice(_) => {}
        ProtocolError::Treasury(_) => {}
        ProtocolError::Compliance(_) => {}
    }
}

#[test]
fn protocol_error_variants_are_exhaustive() {
    assert_protocol_error_exhaustive(ProtocolError::Invoice(InvoiceError::NotFound));
    assert_protocol_error_exhaustive(ProtocolError::Treasury(TreasuryError::SettlementNotFound));
    assert_protocol_error_exhaustive(ProtocolError::Compliance(ComplianceError::AlreadyInitialized));
}

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
