use soroban_sdk::{contracterror, contracttype, Address};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    PendingAdmin,
    Allowed(Address),
    Blocked(Address),
    AllowedUntil(Address),
    BlockReason(Address),
    Paused,
    AddressIndex,
}

#[contracttype]
#[derive(Clone, PartialEq, Debug)]
pub enum AddressState {
    Allowed,
    Blocked,
    Expired,
}

#[contracterror]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ComplianceError {
    AlreadyInitialized = 1,
}
