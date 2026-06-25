#![no_std]

/// Re-export the auto-generated cross-contract client for the compliance contract.
///
/// Other Soroban contracts (e.g. treasury) depend on this crate to call
/// [`is_allowed`] via cross-contract invocation without duplicating the ABI
/// binding boilerplate.
///
/// # Example
/// ```ignore
/// use compliance_client::ComplianceClient;
///
/// let client = ComplianceClient::new(&env, &compliance_contract_id);
/// if !client.is_allowed(&address) {
///     panic!("address not compliant");
/// }
/// ```
pub use compliance::ComplianceContractClient as ComplianceClient;
