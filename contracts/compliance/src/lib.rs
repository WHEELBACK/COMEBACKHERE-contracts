#![no_std]

mod allowlist;
pub use allowlist::{AddressState, ComplianceError, DataKey};

use soroban_sdk::{contract, contracterror, contractimpl, Address, Bytes, Env, Symbol, Vec};

#[contracterror]
#[derive(Copy, Clone, Debug, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    Unauthorized = 1,
    ContractPaused = 2,
    AlreadyInitialized = 3,
}

#[contract]
pub struct ComplianceContract;

#[contractimpl]
impl ComplianceContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    pub fn is_allowed(env: Env, address: Address) -> bool {
        let blocked: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Blocked(address.clone()))
            .unwrap_or(false);
        if blocked {
            return false;
        }
        let allowed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Allowed(address.clone()))
            .unwrap_or(false);
        if !allowed {
            return false;
        }
        // Check optional expiry
        if let Some(expires_at) = env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::AllowedUntil(address))
        {
            return env.ledger().timestamp() < expires_at;
        }
        true
    }

    pub fn allow_address(env: Env, admin: Address, address: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::require_not_paused(&env)?;
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(address.clone()), &true);
        // Remove any expiry so this becomes a permanent allow.
        env.storage()
            .persistent()
            .remove(&DataKey::AllowedUntil(address.clone()));
        Self::track_address(&env, &address);
        env.events()
            .publish((Symbol::new(&env, "address_allowed"),), address);
        Ok(())
    }

    // Emergency policy: block_address and clear_address are permitted while paused
    // so the admin can remediate compromised addresses without unpausing first.
    pub fn block_address(
        env: Env,
        admin: Address,
        address: Address,
        reason: Option<Bytes>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Blocked(address.clone()), &true);
        if let Some(r) = reason {
            env.storage()
                .persistent()
                .set(&DataKey::BlockReason(address.clone()), &r);
        }
        Self::track_address(&env, &address);
        env.events()
            .publish((Symbol::new(&env, "address_blocked"),), address);
        Ok(())
    }

    /// Block an address until a specific ledger timestamp. Permitted while paused (emergency policy).
    pub fn block_address_until(
        env: Env,
        admin: Address,
        address: Address,
        expires_at: u64,
        reason: Option<Bytes>,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Blocked(address.clone()), &true);
        env.storage()
            .persistent()
            .set(&DataKey::AllowedUntil(address.clone()), &expires_at);
        if let Some(r) = reason {
            env.storage()
                .persistent()
                .set(&DataKey::BlockReason(address.clone()), &r);
        }
        Self::track_address(&env, &address);
        env.events().publish(
            (Symbol::new(&env, "address_blocked_until"),),
            (address, expires_at),
        );
        Ok(())
    }

    /// Returns the stored block reason for an address, if any.
    pub fn get_block_reason(env: Env, address: Address) -> Option<Bytes> {
        env.storage()
            .persistent()
            .get(&DataKey::BlockReason(address))
    }

    /// Allow an address until a specific ledger timestamp (seconds since epoch).
    /// After expiry, `is_allowed` returns false even if the Allowed flag is set.
    pub fn allow_address_until(
        env: Env,
        admin: Address,
        address: Address,
        expires_at: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::require_not_paused(&env)?;
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(address.clone()), &true);
        env.storage()
            .persistent()
            .set(&DataKey::AllowedUntil(address.clone()), &expires_at);
        Self::track_address(&env, &address);
        env.events().publish(
            (Symbol::new(&env, "address_allowed_until"),),
            (address, expires_at),
        );
        Ok(())
    }

    /// Initiate a two-step admin transfer. The pending admin must call accept_admin.
    pub fn transfer_admin(
        env: Env,
        admin: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        env.events()
            .publish((Symbol::new(&env, "admin_transfer_initiated"),), new_admin);
        Ok(())
    }

    /// Complete the admin transfer. Must be called by the pending admin.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), ContractError> {
        new_admin.require_auth();
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .expect("NoPendingAdmin");
        if pending != new_admin {
            return Err(ContractError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.events()
            .publish((Symbol::new(&env, "admin_transferred"),), new_admin);
        Ok(())
    }

    pub fn clear_address(env: Env, admin: Address, address: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::Blocked(address.clone()), &false);
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(address.clone()), &true);
        Self::track_address(&env, &address);
        env.events()
            .publish((Symbol::new(&env, "address_cleared"),), address);
        Ok(())
    }

    pub fn pause(env: Env, admin: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events()
            .publish((Symbol::new(&env, "compliance_paused"),), admin);
        Ok(())
    }

    pub fn unpause(env: Env, admin: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events()
            .publish((Symbol::new(&env, "compliance_unpaused"),), admin);
        Ok(())
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
        admin.require_auth();
        let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if stored != *admin {
            return Err(ContractError::Unauthorized);
        }
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), ContractError> {
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            return Err(ContractError::ContractPaused);
        }
        Ok(())
    }

    /// Returns the current compliance state for every tracked address.
    /// Requires admin authentication for audit-trail accountability.
    pub fn export_snapshot(env: Env, admin: Address) -> Vec<(Address, AddressState)> {
        Self::require_admin(&env, &admin).expect("Unauthorized");
        let index: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AddressIndex)
            .unwrap_or(Vec::new(&env));
        let mut out: Vec<(Address, AddressState)> = Vec::new(&env);
        for addr in index.iter() {
            let blocked: bool = env
                .storage()
                .persistent()
                .get(&DataKey::Blocked(addr.clone()))
                .unwrap_or(false);
            let state = if blocked {
                AddressState::Blocked
            } else {
                let allowed: bool = env
                    .storage()
                    .persistent()
                    .get(&DataKey::Allowed(addr.clone()))
                    .unwrap_or(false);
                if !allowed {
                    AddressState::Blocked
                } else if let Some(expires_at) = env
                    .storage()
                    .persistent()
                    .get::<_, u64>(&DataKey::AllowedUntil(addr.clone()))
                {
                    if env.ledger().timestamp() < expires_at {
                        AddressState::Allowed
                    } else {
                        AddressState::Expired
                    }
                } else {
                    AddressState::Allowed
                }
            };
            out.push_back((addr, state));
        }
        out
    }

    /// Adds `address` to the instance-level AddressIndex if not already present.
    fn track_address(env: &Env, address: &Address) {
        let mut index: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AddressIndex)
            .unwrap_or(Vec::new(env));
        if !index.contains(address) {
            index.push_back(address.clone());
            env.storage()
                .instance()
                .set(&DataKey::AddressIndex, &index);
        }
    }
}

#[cfg(test)]
extern crate std;
