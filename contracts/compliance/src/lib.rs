#![no_std]

mod allowlist;
pub use allowlist::{AddressState, AddressStatus, ComplianceError, DataKey};

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
    /// Initialize the compliance contract with an admin address.
    ///
    /// # Parameters
    /// - `admin`: The initial administrator. Must authorize this call.
    ///
    /// # Errors
    /// - [`ContractError::AlreadyInitialized`] if the contract has already been initialized.
    ///
    /// # Events
    /// None emitted on initialization.
    pub fn initialize(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(ContractError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::SchemaVersion, &1u32);
        Ok(())
    }

    pub fn bulk_check_addresses(env: Env, addresses: Vec<Address>) -> Vec<bool> {
        let mut results = Vec::new(&env);
        for address in addresses.iter() {
            results.push_back(Self::is_allowed(env.clone(), address));
        }
        results
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

    /// Permanently allow an address. Removes any existing expiry.
    ///
    /// # Parameters
    /// - `admin`: Current administrator. Must authorize this call.
    /// - `address`: The address to allow.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `admin` is not the stored administrator.
    /// - [`ContractError::ContractPaused`] if the contract is paused.
    ///
    /// # Events
    /// Publishes `("address_allowed",) → address`.
    pub fn allow_address(env: Env, admin: Address, address: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::require_not_paused(&env)?;
        let was_allowed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Allowed(address.clone()))
            .unwrap_or(false);
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(address.clone()), &true);
        // Remove any expiry so this becomes a permanent allow.
        env.storage()
            .persistent()
            .remove(&DataKey::AllowedUntil(address.clone()));
        if !was_allowed {
            let count: u64 = env
                .storage()
                .instance()
                .get(&DataKey::AllowCount)
                .unwrap_or(0u64);
            env.storage()
                .instance()
                .set(&DataKey::AllowCount, &(count + 1));
        }
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

    /// Returns the schema version set at initialization.
    pub fn get_schema_version(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::SchemaVersion)
            .unwrap_or(1)
    }

    /// Allow an address until a specific ledger timestamp (seconds since epoch).
    ///
    /// After `expires_at`, [`is_allowed`](Self::is_allowed) returns `false` even if
    /// the `Allowed` flag is set.
    ///
    /// # Parameters
    /// - `admin`: Current administrator. Must authorize this call.
    /// - `address`: The address to allow temporarily.
    /// - `expires_at`: Unix timestamp (seconds) after which the allowance expires.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `admin` is not the stored administrator.
    /// - [`ContractError::ContractPaused`] if the contract is paused.
    ///
    /// # Events
    /// Publishes `("address_allowed_until",) → (address, expires_at)`.
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

    /// Initiate a two-step admin transfer. The pending admin must call
    /// [`accept_admin`](Self::accept_admin) to complete the handover.
    ///
    /// # Parameters
    /// - `admin`: Current administrator. Must authorize this call.
    /// - `new_admin`: The address being nominated as the next administrator.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `admin` is not the stored administrator.
    ///
    /// # Events
    /// Publishes `("admin_transfer_initiated",) → new_admin`.
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

    /// Complete the admin transfer initiated by [`transfer_admin`](Self::transfer_admin).
    ///
    /// Must be called by the pending admin to activate the new admin role.
    ///
    /// # Parameters
    /// - `new_admin`: The pending administrator. Must authorize this call.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `new_admin` does not match the stored pending admin.
    ///
    /// # Panics
    /// Panics with `"NoPendingAdmin"` if [`transfer_admin`](Self::transfer_admin) was never called.
    ///
    /// # Events
    /// Publishes `("admin_transferred",) → new_admin`.
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

    /// Remove the block flag and explicitly allow an address.
    ///
    /// Permitted even while paused (emergency policy). Does **not** remove an existing
    /// `AllowedUntil` expiry; call [`allow_address`](Self::allow_address) for a
    /// permanent, expiry-free allow.
    ///
    /// # Parameters
    /// - `admin`: Current administrator. Must authorize this call.
    /// - `address`: The address to clear.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `admin` is not the stored administrator.
    ///
    /// # Events
    /// Publishes `("address_cleared",) → address`.
    pub fn clear_address(env: Env, admin: Address, address: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        let was_blocked: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Blocked(address.clone()))
            .unwrap_or(false);
        let was_allowed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Allowed(address.clone()))
            .unwrap_or(false);
        env.storage()
            .persistent()
            .set(&DataKey::Blocked(address.clone()), &false);
        env.storage()
            .persistent()
            .set(&DataKey::Allowed(address.clone()), &true);
        if was_blocked {
            let count: u64 = env
                .storage()
                .instance()
                .get(&DataKey::BlockCount)
                .unwrap_or(0u64);
            env.storage()
                .instance()
                .set(&DataKey::BlockCount, &count.saturating_sub(1));
        }
        if !was_allowed {
            let count: u64 = env
                .storage()
                .instance()
                .get(&DataKey::AllowCount)
                .unwrap_or(0u64);
            env.storage()
                .instance()
                .set(&DataKey::AllowCount, &(count + 1));
        }
        Self::track_address(&env, &address);
        env.events()
            .publish((Symbol::new(&env, "address_cleared"),), address);
        Ok(())
    }

    /// Remove the allowed status for an address without blocking it.
    /// This is a soft de-listing: the address is removed from the allowlist
    /// but not placed on the blocklist, so it can be re-allowed later.
    pub fn revoke_allow(env: Env, admin: Address, address: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        Self::require_not_paused(&env)?;
        env.storage()
            .persistent()
            .remove(&DataKey::Allowed(address.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::AllowedUntil(address.clone()));
        Self::track_address(&env, &address);
        env.events()
            .publish((Symbol::new(&env, "address_revoked"),), address);
        Ok(())
    }

    pub fn pause(env: Env, admin: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events()
            .publish((Symbol::new(&env, "compliance_paused"),), admin);
        Ok(())
    }

    /// Resume normal operation after a pause.
    ///
    /// # Parameters
    /// - `admin`: Current administrator. Must authorize this call.
    ///
    /// # Errors
    /// - [`ContractError::Unauthorized`] if `admin` is not the stored administrator.
    ///
    /// # Events
    /// Publishes `("compliance_unpaused",) → admin`.
    pub fn unpause(env: Env, admin: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events()
            .publish((Symbol::new(&env, "compliance_unpaused"),), admin);
        Ok(())
    }

    /// Assign an operator address. Only admin may call this.
    pub fn set_operator(env: Env, admin: Address, operator: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Operator, &operator);
        env.events()
            .publish((Symbol::new(&env, "operator_set"),), operator);
        Ok(())
    }

    /// Returns the raw expiry timestamp (seconds since epoch) for `address`, or
    /// `None` if the address has no time-limited allow entry (permanent allow or no allow).
    pub fn get_allow_expiry(env: Env, address: Address) -> Option<u64> {
        env.storage()
            .persistent()
            .get::<_, u64>(&DataKey::AllowedUntil(address))
    }

    /// Returns a paginated snapshot of all tracked addresses and their current state.
    /// Pass `offset=0, limit=0` to return all entries.
    pub fn export_snapshot(
        env: Env,
        admin: Address,
        offset: u32,
        limit: u32,
    ) -> Vec<(Address, AddressState)> {
        Self::require_admin(&env, &admin).unwrap();
        let index: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::AddressIndex)
            .unwrap_or(Vec::new(&env));
        let mut result = Vec::new(&env);
        let start = offset as usize;
        let end = if limit == 0 {
            index.len() as usize
        } else {
            (start + limit as usize).min(index.len() as usize)
        };
        for i in start..end {
            let addr = index.get(i as u32).unwrap();
            let state = Self::address_state(&env, &addr);
            result.push_back((addr, state));
        }
        result
    }

    /// Returns the compliance state for `address` (Allowed, Blocked, or Expired).
    /// Requires admin or operator authentication.
    pub fn address_status(
        env: Env,
        caller: Address,
        address: Address,
    ) -> Result<AddressState, ContractError> {
        Self::require_admin_or_operator(&env, &caller)?;
        let blocked: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Blocked(address.clone()))
            .unwrap_or(false);
        if blocked {
            return Ok(AddressState::Blocked);
        }
        let allowed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Allowed(address.clone()))
            .unwrap_or(false);
        if !allowed {
            return Ok(AddressState::Blocked);
        }
        if let Some(expires_at) = env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::AllowedUntil(address))
        {
            if env.ledger().timestamp() < expires_at {
                Ok(AddressState::Allowed)
            } else {
                Ok(AddressState::Expired)
            }
        } else {
            Ok(AddressState::Allowed)
        }
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), ContractError> {
        admin.require_auth();
        let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if stored != *admin {
            return Err(ContractError::Unauthorized);
        }
        Ok(())
    }

    fn require_admin_or_operator(env: &Env, caller: &Address) -> Result<(), ContractError> {
        caller.require_auth();
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if stored_admin == *caller {
            return Ok(());
        }
        if let Some(operator) = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::Operator)
        {
            if operator == *caller {
                return Ok(());
            }
        }
        Err(ContractError::Unauthorized)
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

    /// Compute the current [`AddressState`] for a single address without auth.
    fn address_state(env: &Env, addr: &Address) -> AddressState {
        let blocked: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Blocked(addr.clone()))
            .unwrap_or(false);
        if blocked {
            return AddressState::Blocked;
        }
        let allowed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Allowed(addr.clone()))
            .unwrap_or(false);
        if !allowed {
            return AddressState::Blocked;
        }
        if let Some(expires_at) = env
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
