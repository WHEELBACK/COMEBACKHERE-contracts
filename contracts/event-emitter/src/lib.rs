#![no_std]

mod errors;

pub use errors::EventEmitterError;

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, Symbol};

/// The emission record stored per settlement ID.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmissionRecord {
    /// Settlement ID the record belongs to.
    pub settlement_id: u64,
    /// Amount emitted so far (`settlement_total` of the last successful emit).
    pub emitted_amount: i128,
    /// Ledger timestamp of the last update.
    pub last_updated: u64,
}

/// Storage keys for the event-emitter contract.
///
/// Used as keys for Soroban instance storage. Variants must not be reordered
/// or removed once deployed; new variants should be appended at the end so
/// that existing on-chain data (keyed by XDR-encoded discriminants) continues
/// to decode correctly.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Stored administrator (instance storage).
    Admin,
    /// Emission record per settlement (instance storage).
    EmissionRecord(u64),
}

#[contract]
pub struct EventEmitterContract;

fn require_admin(env: &Env, admin: &Address) -> Result<(), EventEmitterError> {
    admin.require_auth();
    match env.storage().instance().get(&DataKey::Admin) {
        Some(stored) if stored == *admin => Ok(()),
        _ => Err(EventEmitterError::Unauthorized),
    }
}

#[contractimpl]
impl EventEmitterContract {
    /// One-time initialization: stores `admin`.
    ///
    /// Errors: `AlreadyInitialized`.
    /// Emits: `event_emitter_initialized`.
    pub fn initialize(env: Env, admin: Address) -> Result<(), EventEmitterError> {
        admin.require_auth();
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(EventEmitterError::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.events()
            .publish((Symbol::new(&env, "event_emitter_initialized"),), admin);
        Ok(())
    }

    /// Sets the recorded emitted amount for `settlement_id` (admin-only).
    ///
    /// Strict validation: negative amounts are rejected with `InvalidAmount`.
    /// Emits: `emitted_amount_set`.
    pub fn set_emitted_amount(
        env: Env,
        admin: Address,
        settlement_id: u64,
        amount: i128,
    ) -> Result<(), EventEmitterError> {
        require_admin(&env, &admin)?;
        if amount < 0 {
            return Err(EventEmitterError::InvalidAmount);
        }
        let record = EmissionRecord {
            settlement_id,
            emitted_amount: amount,
            last_updated: env.ledger().timestamp(),
        };
        env.storage()
            .instance()
            .set(&DataKey::EmissionRecord(settlement_id), &record);
        env.events()
            .publish((Symbol::new(&env, "emitted_amount_set"), settlement_id), amount);
        Ok(())
    }

    /// Emits a partial-settlement event for `settlement_id` and returns the
    /// still-unemitted remainder.
    ///
    /// The remainder is computed as `settlement_total - emitted_amount` with
    /// **checked** arithmetic; if `emitted_amount > settlement_total` the call
    /// returns `Err(EventEmitterError::Underflow)` instead of panicking — this
    /// is the #950 defensive fix for partial-settlement computations. The
    /// stored record is updated to `emitted_amount = settlement_total` on
    /// success. Strict validation: negative `settlement_total` →
    /// `InvalidAmount`. Emits: `partial_settlement_emitted`.
    pub fn emit_partial_settlement(
        env: Env,
        settlement_id: u64,
        settlement_total: i128,
    ) -> Result<i128, EventEmitterError> {
        if settlement_total < 0 {
            return Err(EventEmitterError::InvalidAmount);
        }
        let mut record = env
            .storage()
            .instance()
            .get(&DataKey::EmissionRecord(settlement_id))
            .unwrap_or(EmissionRecord {
                settlement_id,
                emitted_amount: 0,
                last_updated: 0,
            });
        let remaining = settlement_total
            .checked_sub(record.emitted_amount)
            .ok_or(EventEmitterError::Underflow)?;
        record.emitted_amount = settlement_total;
        record.last_updated = env.ledger().timestamp();
        env.storage()
            .instance()
            .set(&DataKey::EmissionRecord(settlement_id), &record);
        env.events().publish(
            (Symbol::new(&env, "partial_settlement_emitted"), settlement_id),
            remaining,
        );
        Ok(remaining)
    }

    /// Returns the recorded emitted amount for `settlement_id` (0 if none).
    pub fn get_emitted_amount(env: Env, settlement_id: u64) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::EmissionRecord(settlement_id))
            .map(|record| record.emitted_amount)
            .unwrap_or(0)
    }
}
