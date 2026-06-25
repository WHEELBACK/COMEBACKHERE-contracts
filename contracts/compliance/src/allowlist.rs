use soroban_sdk::{contracterror, contracttype, Address};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    PendingAdmin,
    Operator,
    Allowed(Address),
    Blocked(Address),
    AllowedUntil(Address),
    BlockReason(Address),
    SchemaVersion,
    Paused,
    AddressIndex,
    AllowCount,
    BlockCount,
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
#[repr(u32)]
pub enum ComplianceError {
    AlreadyInitialized = 1,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AddressStatus {
    pub allowed: bool,
    pub blocked: bool,
    pub expires_at: Option<u64>,
    pub is_currently_allowed: bool,
}
