use crate::{DataKey, InvoiceContract, InvoiceError};
use crate::events;
use crate::validation::require_admin;
use soroban_sdk::{contractimpl, Address, Env};

#[contractimpl]
impl InvoiceContract {
    pub fn initialize(env: Env, admin: Address) -> Result<(), InvoiceError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(InvoiceError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::InvoiceCount, &0u64);
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    // --- #55: configurable grace window ---

    /// Set the grace window (seconds) added to expires_at when checking payment validity.
    /// Allows a short buffer after quote expiry for in-flight payments.
    pub fn set_grace_window(env: Env, admin: Address, seconds: u64) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::GraceWindow, &seconds);
        Ok(())
    }

    /// Return the current grace window in seconds (0 if not set).
    pub fn get_grace_window(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::GraceWindow)
            .unwrap_or(0u64)
    }

    // --- #15: two-step admin transfer ---

    /// Initiate admin transfer. Current admin nominates `new_admin`.
    pub fn transfer_admin(
        env: Env,
        admin: Address,
        new_admin: Address,
    ) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        Ok(())
    }

    /// Complete admin transfer. Must be called by the pending admin.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), InvoiceError> {
        new_admin.require_auth();
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(InvoiceError::NoPendingAdmin)?;
        if pending != new_admin {
            return Err(InvoiceError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        Ok(())
    }

    pub fn pause(env: Env, admin: Address) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        events::contract_paused(&env, &admin);
        Ok(())
    }

    pub fn unpause(env: Env, admin: Address) -> Result<(), InvoiceError> {
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        events::contract_unpaused(&env, &admin);
        Ok(())
    }
}
