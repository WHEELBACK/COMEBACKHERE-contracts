use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum TreasuryError {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InsufficientBalance = 4,
    InvalidAmount = 5,
    WithdrawalLimitExceeded = 6,
    InvalidWithdrawalLimit = 7,
    WithdrawalWindowLimitExceeded = 8,
    InvalidWithdrawalWindowLimit = 9,
}
