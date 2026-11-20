//! Fee manager contract for the COMEBACKHERE protocol.
//!
//! # Responsibilities
//!
//! - Maintain a configurable fee in basis points (`FeeBps`, 0–10 000).
//! - Maintain a `FeeRecipient` address that receives collected fees.
//! - Expose `compute_fee` and `compute_partial_fee` for settlement contracts to
//!   calculate their fee obligations without reading unrelated state.
//! - Guard all arithmetic against overflow and underflow (#896).
//! - Enforce granular role-based authorization: Admin or Operator may update
//!   `FeeBps`; only Admin may change all other settings (#897).
//! - Each entrypoint reads only the storage keys it actually needs, keeping
//!   deserialization cost proportional to the data consumed (#898).
//! - A two-step admin transfer (`transfer_admin` / `accept_admin`) prevents
//!   locking out of the contract from a typo or mis-paste.

#![no_std]

use protocol_math::{apply_bps_i128, is_valid_bps, BPS_DENOMINATOR};
use soroban_sdk::{contract, contractimpl, contracterror, contracttype, Address, Env};

// ── Storage keys ─────────────────────────────────────────────────────────────

/// Storage keys for the fee-manager contract.
///
/// Variants are append-only. Reordering them would break any stored data
/// keyed by ordinal position on a live deployment.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Current administrator address (instance storage).
    Admin,
    /// Destination address for collected fees (instance storage).
    FeeRecipient,
    /// Fee rate in basis points, 0–10 000 (instance storage).
    FeeBps,
    /// Circuit-breaker flag; when `true`, all mutating entrypoints are rejected.
    Paused,
    /// Staged address for the two-step admin handover (`transfer_admin` /
    /// `accept_admin`). Absent when no transfer is in flight.
    PendingAdmin,
    /// Presence of this key marks `addr` as an Operator — a limited role that
    /// may update `FeeBps` but cannot perform any other administrative action.
    /// Stored as `bool`; `true` = active operator. (#897)
    Operator(Address),
}

// ── Error codes ───────────────────────────────────────────────────────────────

/// Error codes returned by the fee-manager contract.
///
/// Values are stable: do not renumber existing variants.
#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FeeManagerError {
    /// `initialize` was called on an already-initialized contract.
    AlreadyInitialized = 1,
    /// The caller does not hold a role that permits this operation.
    Unauthorized = 2,
    /// `fee_bps` is out of range (must be `<= BPS_DENOMINATOR = 10_000`).
    InvalidFeeBps = 3,
    /// An intermediate multiplication exceeded `i128::MAX`. (#896)
    ArithmeticOverflow = 4,
    /// A subtraction would have produced a negative result or `settled_amount`
    /// exceeds `total_amount` in a partial-fee call. (#896)
    ArithmeticUnderflow = 5,
    /// The contract is paused; the requested mutation is not permitted.
    ContractPaused = 6,
    /// The entrypoint requires the contract to be initialized first.
    NotInitialized = 7,
    /// `amount` is `<= 0`; fee computation is undefined on non-positive values.
    InvalidAmount = 8,
    /// `accept_admin` was called but no transfer is pending.
    NoPendingAdmin = 9,
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct FeeManagerContract;

// ── Private helpers ───────────────────────────────────────────────────────────

/// Panics with `ContractPaused` if `DataKey::Paused` is `true`.
///
/// Reads *only* `DataKey::Paused` — no other state is deserialized. (#898)
fn require_not_paused(env: &Env) {
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false);
    if paused {
        soroban_sdk::panic_with_error!(env, FeeManagerError::ContractPaused);
    }
}

/// Returns `true` when `caller` is either the stored Admin or a registered
/// Operator. Each role check is a single-key read. (#897, #898)
fn is_caller_admin_or_operator(env: &Env, caller: &Address) -> bool {
    let admin: Option<Address> = env.storage().instance().get(&DataKey::Admin);
    if let Some(ref a) = admin {
        if a == caller {
            return true;
        }
    }
    env.storage()
        .instance()
        .get(&DataKey::Operator(caller.clone()))
        .unwrap_or(false)
}

/// Returns `true` when `caller` matches the stored Admin exactly. (#897)
fn is_caller_admin(env: &Env, caller: &Address) -> bool {
    env.storage()
        .instance()
        .get::<_, Address>(&DataKey::Admin)
        .map(|a| a == *caller)
        .unwrap_or(false)
}

// ── Entrypoints ───────────────────────────────────────────────────────────────

#[contractimpl]
impl FeeManagerContract {
    // ── Lifecycle ─────────────────────────────────────────────────────────

    /// Initialize the contract.
    ///
    /// Sets `Admin`, `FeeRecipient`, `FeeBps`, and `Paused = false`.
    ///
    /// # Errors
    /// - `AlreadyInitialized` if the contract has already been set up.
    /// - `InvalidFeeBps` if `fee_bps > BPS_DENOMINATOR`.
    pub fn initialize(
        env: Env,
        admin: Address,
        fee_recipient: Address,
        fee_bps: u32,
    ) -> Result<(), FeeManagerError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(FeeManagerError::AlreadyInitialized);
        }
        if !is_valid_bps(fee_bps) {
            return Err(FeeManagerError::InvalidFeeBps);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::FeeRecipient, &fee_recipient);
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    // ── Fee configuration ─────────────────────────────────────────────────

    /// Update the fee rate.
    ///
    /// `caller` must be Admin **or** an Operator. (#897)
    ///
    /// # Errors
    /// - `ContractPaused` if the contract is paused.
    /// - `InvalidFeeBps` if `fee_bps > BPS_DENOMINATOR`.
    /// - `Unauthorized` if `caller` holds neither the Admin nor Operator role.
    pub fn set_fee_bps(
        env: Env,
        caller: Address,
        fee_bps: u32,
    ) -> Result<(), FeeManagerError> {
        require_not_paused(&env);
        if !is_valid_bps(fee_bps) {
            return Err(FeeManagerError::InvalidFeeBps);
        }
        caller.require_auth();
        if !is_caller_admin_or_operator(&env, &caller) {
            return Err(FeeManagerError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::FeeBps, &fee_bps);
        Ok(())
    }

    /// Update the fee recipient address.
    ///
    /// `caller` must be Admin. (#897)
    ///
    /// # Errors
    /// - `ContractPaused` if the contract is paused.
    /// - `Unauthorized` if `caller` is not Admin.
    pub fn set_fee_recipient(
        env: Env,
        caller: Address,
        recipient: Address,
    ) -> Result<(), FeeManagerError> {
        require_not_paused(&env);
        caller.require_auth();
        if !is_caller_admin(&env, &caller) {
            return Err(FeeManagerError::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&DataKey::FeeRecipient, &recipient);
        Ok(())
    }

    // ── Operator management ───────────────────────────────────────────────

    /// Grant the Operator role to `operator`. Admin only. (#897)
    ///
    /// # Errors
    /// - `Unauthorized` if `admin` is not the stored Admin.
    pub fn add_operator(
        env: Env,
        admin: Address,
        operator: Address,
    ) -> Result<(), FeeManagerError> {
        admin.require_auth();
        if !is_caller_admin(&env, &admin) {
            return Err(FeeManagerError::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&DataKey::Operator(operator), &true);
        Ok(())
    }

    /// Revoke the Operator role from `operator`. Admin only. (#897)
    ///
    /// # Errors
    /// - `Unauthorized` if `admin` is not the stored Admin.
    pub fn remove_operator(
        env: Env,
        admin: Address,
        operator: Address,
    ) -> Result<(), FeeManagerError> {
        admin.require_auth();
        if !is_caller_admin(&env, &admin) {
            return Err(FeeManagerError::Unauthorized);
        }
        env.storage()
            .instance()
            .remove(&DataKey::Operator(operator));
        Ok(())
    }

    /// Returns `true` if `addr` currently holds the Operator role. (#897)
    ///
    /// Single-key read; no auth required. (#898)
    pub fn is_operator(env: Env, addr: Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Operator(addr))
            .unwrap_or(false)
    }

    // ── Fee computation ───────────────────────────────────────────────────

    /// Compute `(fee, net)` for a full settlement `amount`.
    ///
    /// Uses the stored `FeeBps` via `apply_bps_i128`. Reads *only*
    /// `DataKey::FeeBps` — no other state is touched. (#898)
    ///
    /// # Errors
    /// - `InvalidAmount` if `amount <= 0`.
    /// - `ArithmeticOverflow` if `amount * fee_bps` overflows `i128`. (#896)
    /// - `ArithmeticUnderflow` if `amount - fee < 0`. (#896)
    pub fn compute_fee(env: Env, amount: i128) -> Result<(i128, i128), FeeManagerError> {
        if amount <= 0 {
            return Err(FeeManagerError::InvalidAmount);
        }
        // Single-key read — no deserialization of Admin, FeeRecipient, or Paused. (#898)
        let fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FeeBps)
            .unwrap_or(0u32);
        let fee = apply_bps_i128(amount, fee_bps)
            .ok_or(FeeManagerError::ArithmeticOverflow)?; // #896
        let net = amount
            .checked_sub(fee)
            .ok_or(FeeManagerError::ArithmeticUnderflow)?; // #896
        Ok((fee, net))
    }

    /// Compute the fee owed on `settled_amount` out of a `total_amount` invoice.
    ///
    /// This covers partial-settlement scenarios where only part of an invoice
    /// has been paid so far. The fee is computed on the settled portion only.
    ///
    /// # Errors
    /// - `ArithmeticUnderflow` if `settled_amount > total_amount`. (#896)
    /// - `InvalidAmount` if `total_amount <= 0` or `settled_amount <= 0`.
    /// - `ArithmeticOverflow` if `settled_amount * fee_bps` overflows `i128`.
    /// - `ArithmeticUnderflow` if `settled_amount - fee < 0`.
    pub fn compute_partial_fee(
        env: Env,
        total_amount: i128,
        settled_amount: i128,
    ) -> Result<(i128, i128), FeeManagerError> {
        if total_amount <= 0 || settled_amount <= 0 {
            return Err(FeeManagerError::InvalidAmount);
        }
        // Guard: settled portion must not exceed the invoice total. (#896)
        if settled_amount > total_amount {
            return Err(FeeManagerError::ArithmeticUnderflow);
        }
        // Single-key read. (#898)
        let fee_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FeeBps)
            .unwrap_or(0u32);
        let fee = apply_bps_i128(settled_amount, fee_bps)
            .ok_or(FeeManagerError::ArithmeticOverflow)?; // #896
        let net = settled_amount
            .checked_sub(fee)
            .ok_or(FeeManagerError::ArithmeticUnderflow)?; // #896
        Ok((fee, net))
    }

    // ── State reads ───────────────────────────────────────────────────────

    /// Return the current fee rate in basis points.
    ///
    /// Single-key read — no other state is deserialized. (#898)
    pub fn get_fee_bps(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::FeeBps)
            .unwrap_or(0u32)
    }

    /// Return the current fee recipient address.
    ///
    /// # Panics
    /// Panics via `unwrap` if the contract is not yet initialized.
    ///
    /// Single-key read — no other state is deserialized. (#898)
    pub fn get_fee_recipient(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::FeeRecipient)
            .unwrap()
    }

    // ── Admin transfer ────────────────────────────────────────────────────

    /// Begin a two-step admin transfer by staging `new_admin` as `PendingAdmin`.
    ///
    /// The transfer only completes when `new_admin` calls `accept_admin`.
    ///
    /// # Errors
    /// - `Unauthorized` if `admin` is not the stored Admin.
    pub fn transfer_admin(
        env: Env,
        admin: Address,
        new_admin: Address,
    ) -> Result<(), FeeManagerError> {
        admin.require_auth();
        if !is_caller_admin(&env, &admin) {
            return Err(FeeManagerError::Unauthorized);
        }
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);
        Ok(())
    }

    /// Complete an admin transfer initiated by `transfer_admin`.
    ///
    /// `new_admin` must call this and their address must match `PendingAdmin`.
    ///
    /// # Errors
    /// - `NoPendingAdmin` if no transfer is currently staged.
    /// - `Unauthorized` if `new_admin` does not match the staged `PendingAdmin`.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), FeeManagerError> {
        new_admin.require_auth();
        let pending: Address = env
            .storage()
            .instance()
            .get(&DataKey::PendingAdmin)
            .ok_or(FeeManagerError::NoPendingAdmin)?;
        if pending != new_admin {
            return Err(FeeManagerError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        Ok(())
    }

    // ── Circuit breaker ───────────────────────────────────────────────────

    /// Pause the contract. Admin only.
    ///
    /// While paused, all mutating entrypoints return `ContractPaused`.
    ///
    /// # Errors
    /// - `Unauthorized` if `admin` is not the stored Admin.
    pub fn pause(env: Env, admin: Address) -> Result<(), FeeManagerError> {
        admin.require_auth();
        if !is_caller_admin(&env, &admin) {
            return Err(FeeManagerError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Paused, &true);
        Ok(())
    }

    /// Unpause the contract. Admin only.
    ///
    /// # Errors
    /// - `Unauthorized` if `admin` is not the stored Admin.
    pub fn unpause(env: Env, admin: Address) -> Result<(), FeeManagerError> {
        admin.require_auth();
        if !is_caller_admin(&env, &admin) {
            return Err(FeeManagerError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }
}
