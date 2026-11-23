// contracts/treasury/src/lib.rs - Issue #856
#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, symbol_short};

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    pub fn initialize(env: Env, admin: Address) {
        env.storage().instance().set(&symbol_short!("admin"), &admin);
        env.storage().instance().set(&symbol_short!("paused"), &false);
    }

    pub fn emergency_pause(env: Env, admin: Address) -> Result<(), Symbol> {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&symbol_short!("admin")).ok_or(symbol_short!("noadmin"))?;
        if admin != stored_admin { return Err(symbol_short!("unauth")); }
        env.storage().instance().set(&symbol_short!("paused"), &true);
        env.events().publish((symbol_short!("pause"),), &admin);
        Ok(())
    }

    pub fn schedule_unpause(env: Env, admin: Address) -> Result<(), Symbol> {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&symbol_short!("admin")).ok_or(symbol_short!("noadmin"))?;
        if admin != stored_admin { return Err(symbol_short!("unauth")); }
        let unlock_time = env.ledger().timestamp() + 48 * 3600;
        env.storage().instance().set(&symbol_short!("unltime"), &unlock_time);
        Ok(())
    }

    pub fn unpause(env: Env, admin: Address) -> Result<(), Symbol> {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&symbol_short!("admin")).ok_or(symbol_short!("noadmin"))?;
        if admin != stored_admin { return Err(symbol_short!("unauth")); }
        let unlock_time: u64 = env.storage().instance().get(&symbol_short!("unltime")).ok_or(symbol_short!("notime"))?;
        if env.ledger().timestamp() < unlock_time { return Err(symbol_short!("locked")); }
        env.storage().instance().set(&symbol_short!("paused"), &false);
        Ok(())
    }

    pub fn transfer(env: Env, to: Address, amount: i128) -> Result<(), Symbol> {
        let paused: bool = env.storage().instance().get(&symbol_short!("paused")).unwrap_or(false);
        if paused { return Err(symbol_short!("paused")); }
        if amount <= 0 { return Err(symbol_short!("badamt")); }
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&symbol_short!("paused")).unwrap_or(false)
    }
}
