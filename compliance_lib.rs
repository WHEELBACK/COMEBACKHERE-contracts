// contracts/compliance/src/lib.rs - Issue #858
#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, symbol_short};

#[derive(Clone)]
pub struct MerchantVerificationEvent {
    pub merchant: Address,
    pub new_status: u32,
    pub timestamp: u64,
}

#[contract]
pub struct ComplianceContract;

#[contractimpl]
impl ComplianceContract {
    pub fn update_verification_status(env: Env, merchant: Address, new_status: u32) -> Result<(), Symbol> {
        if new_status > 2 {
            return Err(symbol_short!("badstat"));
        }
        let timestamp = env.ledger().timestamp();
        env.events().publish((symbol_short!("merchant"),), MerchantVerificationEvent { merchant: merchant.clone(), new_status, timestamp });
        env.storage().persistent().set(&merchant, &new_status);
        Ok(())
    }

    pub fn get_verification_status(env: Env, merchant: Address) -> Option<u32> {
        env.storage().persistent().get(&merchant)
    }
}
