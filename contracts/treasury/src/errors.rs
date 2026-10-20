use soroban_sdk::contracterror;

/// Errors returned by the treasury contract.
#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreasuryError {
    /// The caller is not authorized to perform this action.
    Unauthorized = 1,
    /// The requested proposal or signer was not found.
    NotFound = 2,
    /// The proposal has already been executed.
    AlreadyExecuted = 3,
    /// The proposal has expired.
    Expired = 4,
    /// The provided amount is invalid.
    InvalidAmount = 5,
    /// The signer weight is invalid.
    InvalidWeight = 6,
    /// The signer label is invalid.
    InvalidLabel = 7,
    /// The signer already exists in the signer list.
    SignerAlreadyExists = 8,
    /// The signer list would exceed the maximum allowed signers.
    TooManySigners = 9,
    /// The signer weight must be greater than zero; use `remove_signer` to remove a signer.
    ZeroSignerWeight = 10,
}
