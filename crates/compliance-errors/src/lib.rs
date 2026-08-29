#![no_std]

use soroban_sdk::contracterror;

#[contracterror]
#[derive(Clone, Copy, Debug, PartialEq)]
#[repr(u32)]
pub enum ComplianceError {
    AlreadyInitialized = 1,
}
