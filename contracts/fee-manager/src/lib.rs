#![no_std]

mod errors;
mod storage;

pub use errors::FeeManagerError;
pub use storage::{bump_instance_ttl, bump_persistent_ttl, MIN_TTL_THRESHOLD, TTL_BUMP_AMOUNT};

use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol};

// ── Storage key enum ─────────────────────────────────────────────────────────

/// Storage keys used by the fee-manager contract.
///
/// Instance-storage keys: `Admin`, `FeeRateBps`, `Paused`.
/// Persistent-storage keys: `Balance`, `AccumulatedFees`.
///
/// Add new variants at the end only — reordering breaks on-chain state.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The administrator `Address` (instance storage).
    Admin,
    /// Fee rate in basis points, 0–10_000 (instance storage).
    FeeRateBps,
    /// Circuit-breaker flag (instance storage).
    Paused,
    /// Net balance for `(depositor, token)` after fee deduction (persistent).
    Balance(Address, Address),
    /// Total uncollected protocol fees for `token` (persistent).
    AccumulatedFees(Address),
}

// ── Contract struct ───────────────────────────────────────────────────────────

#[contract]
pub struct FeeManagerContract;

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Authenticate `admin` and confirm it matches the stored administrator.
/// Panics with [`FeeManagerError::Unauthorized`] if the check fails.
fn require_admin(env: &Env, admin: &Address) {
    admin.require_auth();
    let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
    if stored != *admin {
        soroban_sdk::panic_with_error!(env, FeeManagerError::Unauthorized);
    }
}

/// Reject the call if the contract is paused.
/// Panics with [`FeeManagerError::ContractPaused`] when paused.
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

/// Issue #900 — Strict invariant verification.
///
/// Called after every state-mutating operation.  Panics if any fundamental
/// invariant is violated:
/// - The `Admin` key must be present in instance storage.
/// - The fee rate must be within [0, 10_000] basis points.
fn verify_invariants(env: &Env) {
    // Admin must always be set after initialization.
    if !env.storage().instance().has(&DataKey::Admin) {
        soroban_sdk::panic_with_error!(env, FeeManagerError::Unauthorized);
    }

    // Fee rate must remain within the valid basis-point range.
    let rate: u32 = env
        .storage()
        .instance()
        .get(&DataKey::FeeRateBps)
        .unwrap_or(0);
    if rate > 10_000 {
        soroban_sdk::panic_with_error!(env, FeeManagerError::InvalidFeeRate);
    }
}

// ── Contract entrypoints ──────────────────────────────────────────────────────

#[contractimpl]
impl FeeManagerContract {
    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Initialize the contract with `admin` and a fee rate in basis points.
    ///
    /// # Errors
    /// - [`FeeManagerError::AlreadyInitialized`] if already set up.
    /// - [`FeeManagerError::InvalidFeeRate`] if `fee_rate_bps > 10_000`.
    ///
    /// # Events
    /// None.
    pub fn initialize(env: Env, admin: Address, fee_rate_bps: u32) -> Result<(), FeeManagerError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(FeeManagerError::AlreadyInitialized);
        }
        if fee_rate_bps > 10_000 {
            return Err(FeeManagerError::InvalidFeeRate);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::FeeRateBps, &fee_rate_bps);
        env.storage().instance().set(&DataKey::Paused, &false);

        // Issue #902 — bump instance TTL after initialization.
        bump_instance_ttl(&env);

        // Issue #900 — verify invariants hold immediately after setup.
        verify_invariants(&env);

        Ok(())
    }

    // ── Deposit ───────────────────────────────────────────────────────────────

    /// Deposit `amount` tokens from `from`, record the net balance (after fee),
    /// and accumulate the protocol fee for `token`.
    ///
    /// Issue #903 — zero amounts are rejected explicitly rather than silently
    /// succeeding.
    ///
    /// # Errors
    /// - [`FeeManagerError::ContractPaused`] when paused.
    /// - [`FeeManagerError::ZeroAmount`] if `amount == 0`.
    /// - [`FeeManagerError::ArithmeticOverflow`] on overflow.
    ///
    /// # Events
    /// Publishes `("deposited",) → (from, token, amount)`.
    pub fn deposit(
        env: Env,
        from: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), FeeManagerError> {
        require_not_paused(&env);
        from.require_auth();

        // Issue #903 — reject zero-amount deposits explicitly.
        if amount == 0 {
            return Err(FeeManagerError::ZeroAmount);
        }

        // Pull the tokens into the contract first.
        let contract_addr = env.current_contract_address();
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&from, &contract_addr, &amount);

        // Calculate the fee.
        let fee_rate: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FeeRateBps)
            .unwrap_or(0);
        // fee = amount * fee_rate / 10_000, using i128 arithmetic.
        let fee: i128 = amount
            .checked_mul(fee_rate as i128)
            .ok_or(FeeManagerError::ArithmeticOverflow)?
            / 10_000;
        let net: i128 = amount
            .checked_sub(fee)
            .ok_or(FeeManagerError::ArithmeticOverflow)?;

        // Update net balance for depositor × token.  Issue #902: bump TTL.
        let balance_key = DataKey::Balance(from.clone(), token.clone());
        // Issue #903 — unwrap_or(0) so absent storage never panics.
        let prev_balance: i128 = env
            .storage()
            .persistent()
            .get(&balance_key)
            .unwrap_or(0i128);
        let new_balance = prev_balance
            .checked_add(net)
            .ok_or(FeeManagerError::ArithmeticOverflow)?;
        env.storage().persistent().set(&balance_key, &new_balance);
        bump_persistent_ttl(&env, &balance_key);

        // Update accumulated fees for this token.  Issue #902: bump TTL.
        let fees_key = DataKey::AccumulatedFees(token.clone());
        // Issue #903 — unwrap_or(0) so absent storage never panics.
        let prev_fees: i128 = env.storage().persistent().get(&fees_key).unwrap_or(0i128);
        let new_fees = prev_fees
            .checked_add(fee)
            .ok_or(FeeManagerError::ArithmeticOverflow)?;
        env.storage().persistent().set(&fees_key, &new_fees);
        bump_persistent_ttl(&env, &fees_key);

        // Issue #902 — bump instance TTL after mutating state.
        bump_instance_ttl(&env);

        // Issue #900 — verify invariants after every state mutation.
        verify_invariants(&env);

        env.events()
            .publish((Symbol::new(&env, "deposited"),), (from, token, amount));
        Ok(())
    }

    // ── Fee collection ────────────────────────────────────────────────────────

    /// Transfer all accumulated fees for `token` to `recipient` (admin-only).
    ///
    /// Issue #903 — returns [`FeeManagerError::ZeroBalance`] when there are no
    /// fees to collect rather than transferring 0.
    ///
    /// # Errors
    /// - [`FeeManagerError::Unauthorized`] if `admin` is not the stored admin.
    /// - [`FeeManagerError::ZeroBalance`] if accumulated fees are 0.
    ///
    /// # Events
    /// Publishes `("fees_collected",) → (token, recipient, amount)`.
    pub fn collect_fees(
        env: Env,
        admin: Address,
        token: Address,
        recipient: Address,
    ) -> Result<(), FeeManagerError> {
        require_admin(&env, &admin);

        let fees_key = DataKey::AccumulatedFees(token.clone());
        // Issue #903 — unwrap_or(0) so absent storage never panics.
        let fees: i128 = env.storage().persistent().get(&fees_key).unwrap_or(0i128);

        // Issue #903 — explicit rejection of zero-balance collect.
        if fees == 0 {
            return Err(FeeManagerError::ZeroBalance);
        }

        // Reset before transferring (checks-effects-interactions).
        env.storage().persistent().set(&fees_key, &0i128);
        bump_persistent_ttl(&env, &fees_key);

        let contract_addr = env.current_contract_address();
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&contract_addr, &recipient, &fees);

        // Issue #902 — bump instance TTL after state mutation.
        bump_instance_ttl(&env);

        // Issue #900 — verify invariants after every state mutation.
        verify_invariants(&env);

        env.events().publish(
            (Symbol::new(&env, "fees_collected"),),
            (token, recipient, fees),
        );
        Ok(())
    }

    // ── Read-only queries ─────────────────────────────────────────────────────

    /// Return the depositor's net balance (deposit minus fee) for `token`.
    ///
    /// Issue #903 — returns 0 when no entry exists; never panics on absent
    /// storage.
    pub fn get_balance(env: Env, depositor: Address, token: Address) -> i128 {
        let key = DataKey::Balance(depositor, token);
        // Issue #902 — bump TTL on every persistent read.
        let val: i128 = env.storage().persistent().get(&key).unwrap_or(0i128);
        if val != 0 {
            bump_persistent_ttl(&env, &key);
        }
        val
    }

    /// Return total uncollected fees for `token`.
    ///
    /// Issue #903 — returns 0 when no entry exists; never panics on absent
    /// storage.
    pub fn get_accumulated_fees(env: Env, token: Address) -> i128 {
        let key = DataKey::AccumulatedFees(token);
        // Issue #902 — bump TTL on every persistent read.
        let val: i128 = env.storage().persistent().get(&key).unwrap_or(0i128);
        if val != 0 {
            bump_persistent_ttl(&env, &key);
        }
        val
    }

    /// Return the current fee rate in basis points.
    pub fn get_fee_rate(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::FeeRateBps)
            .unwrap_or(0)
    }

    // ── Admin operations ──────────────────────────────────────────────────────

    /// Update the fee rate (admin-only).
    ///
    /// # Errors
    /// - [`FeeManagerError::Unauthorized`] if `admin` is not the stored admin.
    /// - [`FeeManagerError::InvalidFeeRate`] if `new_rate > 10_000`.
    ///
    /// # Events
    /// Publishes `("fee_rate_updated",) → new_rate`.
    pub fn update_fee_rate(env: Env, admin: Address, new_rate: u32) -> Result<(), FeeManagerError> {
        require_admin(&env, &admin);
        if new_rate > 10_000 {
            return Err(FeeManagerError::InvalidFeeRate);
        }
        env.storage()
            .instance()
            .set(&DataKey::FeeRateBps, &new_rate);

        // Issue #902 — bump instance TTL after state mutation.
        bump_instance_ttl(&env);

        // Issue #900 — verify invariants after every state mutation.
        verify_invariants(&env);

        env.events()
            .publish((Symbol::new(&env, "fee_rate_updated"),), new_rate);
        Ok(())
    }

    /// Pause all state-mutating operations.
    ///
    /// # Events
    /// Publishes `("fee_manager_paused",) → admin`.
    pub fn pause(env: Env, admin: Address) -> Result<(), FeeManagerError> {
        require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Paused, &true);

        bump_instance_ttl(&env);
        verify_invariants(&env);

        env.events()
            .publish((Symbol::new(&env, "fee_manager_paused"),), admin);
        Ok(())
    }

    /// Resume operations after a pause.
    ///
    /// # Events
    /// Publishes `("fee_manager_unpaused",) → admin`.
    pub fn unpause(env: Env, admin: Address) -> Result<(), FeeManagerError> {
        require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);

        bump_instance_ttl(&env);
        verify_invariants(&env);

        env.events()
            .publish((Symbol::new(&env, "fee_manager_unpaused"),), admin);
        Ok(())
    }

    // ── TTL maintenance ───────────────────────────────────────────────────────

    /// Issue #902 — Public entrypoint that lets keeper bots (or any caller) bump
    /// the instance-storage TTL without requiring any state change.
    pub fn bump_state_ttl(env: Env) {
        bump_instance_ttl(&env);
    }
}

#[cfg(test)]
extern crate std;
