#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Vec};

mod errors;
pub use errors::Error;

#[cfg(test)]
mod test;

/// Status of a dispute.
#[contracttype]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisputeStatus {
    Open = 0,
    Resolved = 1,
    Cancelled = 2,
}

/// A dispute record.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dispute {
    pub id: u64,
    pub claimant: Address,
    pub respondent: Address,
    pub arbiter: Address,
    pub amount: i128,
    pub status: DisputeStatus,
    pub reason: String,
}

/// Storage keys for the contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    NextId,
    Dispute(u64),
}

const MAX_REASON_LEN: u32 = 256;
const MAX_AMOUNT: i128 = i128::MAX;

#[contract]
pub struct DisputeResolver;

#[contractimpl]
impl DisputeResolver {
    /// Initialize the contract with an admin. Can only be called once.
    pub fn initialize(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextId, &0u64);
        Ok(())
    }

    /// Returns the admin address.
    pub fn admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    /// Open a new dispute. The claimant must authorize the call.
    pub fn open_dispute(
        env: Env,
        claimant: Address,
        respondent: Address,
        arbiter: Address,
        amount: i128,
        reason: String,
    ) -> Result<u64, Error> {
        Self::require_initialized(&env)?;
        claimant.require_auth();

        if claimant == respondent {
            return Err(Error::SelfDispute);
        }
        if claimant == arbiter || respondent == arbiter {
            return Err(Error::InvalidArbiter);
        }
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if amount > MAX_AMOUNT {
            return Err(Error::AmountOverflow);
        }
        if reason.len() > MAX_REASON_LEN {
            return Err(Error::ReasonTooLong);
        }

        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextId)
            .ok_or(Error::NotInitialized)?;

        let dispute = Dispute {
            id,
            claimant,
            respondent,
            arbiter,
            amount,
            status: DisputeStatus::Open,
            reason,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Dispute(id), &dispute);

        let next = id.checked_add(1).ok_or(Error::IdOverflow)?;
        env.storage().instance().set(&DataKey::NextId, &next);

        Ok(id)
    }

    /// Retrieve a dispute by id.
    pub fn get_dispute(env: Env, id: u64) -> Result<Dispute, Error> {
        Self::require_initialized(&env)?;
        env.storage()
            .persistent()
            .get(&DataKey::Dispute(id))
            .ok_or(Error::DisputeNotFound)
    }

    /// Resolve a dispute in favor of the claimant. Only the arbiter may call.
    pub fn resolve_for_claimant(env: Env, arbiter: Address, id: u64) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        arbiter.require_auth();

        let mut dispute = Self::load_dispute(&env, id)?;
        if dispute.arbiter != arbiter {
            return Err(Error::UnauthorizedArbiter);
        }
        if dispute.status != DisputeStatus::Open {
            return Err(Error::InvalidStateTransition);
        }

        dispute.status = DisputeStatus::Resolved;
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(id), &dispute);
        Ok(())
    }

    /// Resolve a dispute in favor of the respondent. Only the arbiter may call.
    pub fn resolve_for_respondent(env: Env, arbiter: Address, id: u64) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        arbiter.require_auth();

        let mut dispute = Self::load_dispute(&env, id)?;
        if dispute.arbiter != arbiter {
            return Err(Error::UnauthorizedArbiter);
        }
        if dispute.status != DisputeStatus::Open {
            return Err(Error::InvalidStateTransition);
        }

        dispute.status = DisputeStatus::Resolved;
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(id), &dispute);
        Ok(())
    }

    /// Cancel an open dispute. Only the claimant may call.
    pub fn cancel_dispute(env: Env, claimant: Address, id: u64) -> Result<(), Error> {
        Self::require_initialized(&env)?;
        claimant.require_auth();

        let mut dispute = Self::load_dispute(&env, id)?;
        if dispute.claimant != claimant {
            return Err(Error::UnauthorizedClaimant);
        }
        if dispute.status != DisputeStatus::Open {
            return Err(Error::InvalidStateTransition);
        }

        dispute.status = DisputeStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(id), &dispute);
        Ok(())
    }

    /// List all dispute ids. Admin only.
    pub fn list_disputes(env: Env, admin: Address) -> Result<Vec<u64>, Error> {
        Self::require_initialized(&env)?;
        admin.require_auth();

        let stored_admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        if stored_admin != admin {
            return Err(Error::UnauthorizedAdmin);
        }

        let next: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextId)
            .ok_or(Error::NotInitialized)?;

        let mut ids = Vec::new(&env);
        let mut i: u64 = 0;
        while i < next {
            ids.push_back(i);
            i += 1;
        }
        Ok(ids)
    }

    fn require_initialized(env: &Env) -> Result<(), Error> {
        if !env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::NotInitialized);
        }
        Ok(())
    }

    fn load_dispute(env: &Env, id: u64) -> Result<Dispute, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Dispute(id))
            .ok_or(Error::DisputeNotFound)
    }
}