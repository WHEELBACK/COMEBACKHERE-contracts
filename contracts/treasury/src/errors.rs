use soroban_sdk::contracterror;

/// Errors returned by the treasury contract.
///
/// Discriminants are stable and must not be renumbered: clients rely on the
/// numeric codes for debugging and error handling. New variants must be
/// appended with fresh discriminants.
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
    /// A subtraction would underflow during a partial settlement computation.
    SettlementUnderflow = 11,
    /// An addition or multiplication would overflow during settlement math.
    ArithmeticOverflow = 12,
    /// A strict invariant check failed; state would become inconsistent.
    InvariantViolation = 13,
    /// The settlement amount exceeds the remaining unsettled balance.
    SettlementExceedsRemaining = 14,
    /// The settlement has already been fully processed.
    SettlementAlreadyComplete = 15,
    /// The settlement state is inconsistent with the recorded proposal.
    InvalidSettlementState = 16,
    /// A division by zero was attempted during settlement math.
    DivisionByZero = 17,
}

impl TreasuryError {
    /// Returns a non-empty, client-facing description of the error.
    ///
    /// These messages are intended for off-chain debugging and logging; they
    /// are not stored on-chain and do not affect the ledger footprint.
    pub fn message(&self) -> &'static str {
        match self {
            TreasuryError::Unauthorized => "caller is not authorized to perform this action",
            TreasuryError::NotFound => "requested proposal or signer was not found",
            TreasuryError::AlreadyExecuted => "proposal has already been executed",
            TreasuryError::Expired => "proposal has expired",
            TreasuryError::InvalidAmount => "provided amount is invalid",
            TreasuryError::InvalidWeight => "signer weight is invalid",
            TreasuryError::InvalidLabel => "signer label is invalid",
            TreasuryError::SignerAlreadyExists => "signer already exists in the signer list",
            TreasuryError::TooManySigners => "signer list would exceed the maximum allowed signers",
            TreasuryError::ZeroSignerWeight => {
                "signer weight must be greater than zero; use remove_signer to remove a signer"
            }
            TreasuryError::SettlementUnderflow => {
                "subtraction would underflow during partial settlement computation"
            }
            TreasuryError::ArithmeticOverflow => {
                "addition or multiplication would overflow during settlement math"
            }
            TreasuryError::InvariantViolation => {
                "strict invariant check failed; state would become inconsistent"
            }
            TreasuryError::SettlementExceedsRemaining => {
                "settlement amount exceeds the remaining unsettled balance"
            }
            TreasuryError::SettlementAlreadyComplete => {
                "settlement has already been fully processed"
            }
            TreasuryError::InvalidSettlementState => {
                "settlement state is inconsistent with the recorded proposal"
            }
            TreasuryError::DivisionByZero => {
                "division by zero was attempted during settlement math"
            }
        }
    }
}

// Re-export the canonical error type for backward compatibility with code that
// previously imported it from the `multisig` module.
pub use TreasuryError as MultisigError;