#![no_std]

pub use multisig::{
    DataKey, Dispute, DisputeStatus, RotationStatus, Settlement, SettlementHoldReason,
    SettlementStatus, SignerChangeKind, SignerChangeProposal, SignerChangeStatus,
    SignerRotationProposal, TreasuryError,
};

use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, Vec};

mod deposits;
mod disputes;
mod holds;
mod settlements;
mod signers;
mod timelock;

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    /// Initialises the treasury with `admin` as owner and `threshold` as the multisig approval
    /// weight required to execute settlements. Accepts an initial `signers` list of `(Address, u32)`
    /// pairs to bootstrap the full signer set in a single transaction.
    /// Errors: `AlreadyInitialized`, `ZeroThreshold`.
    /// Emits: `treasury_initialized`.
    pub fn initialize(
        env: Env,
        admin: Address,
        threshold: u32,
        signers: Vec<(Address, u32)>,
    ) -> Result<(), TreasuryError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(TreasuryError::AlreadyInitialized);
        }
        if threshold == 0 {
            return Err(TreasuryError::ZeroThreshold);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::Threshold, &threshold);
        env.storage()
            .instance()
            .set(&DataKey::SettlementCount, &0u64);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::DisputeCount, &0u64);
        env.storage()
            .instance()
            .set(&DataKey::Signer(admin.clone()), &1u32);
        let mut signer_list = Vec::new(&env);
        signer_list.push_back(admin.clone());
        for (signer, weight) in signers.iter() {
            env.storage()
                .instance()
                .set(&DataKey::Signer(signer.clone()), &weight);
            if weight > 0 && !signer_list.contains(&signer) {
                signer_list.push_back(signer.clone());
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::SignerList, &signer_list);
        env.events()
            .publish((Symbol::new(&env, "treasury_initialized"),), admin);
        Ok(())
    }

    /// Updates the multisig approval threshold required to execute settlements (admin-only).
    /// Errors: `ZeroThreshold`, `ThresholdUnreachable`.
    /// Emits: `threshold_updated`.
    pub fn update_threshold(
        env: Env,
        admin: Address,
        new_threshold: u32,
    ) -> Result<(), TreasuryError> {
        require_admin(&env, &admin);
        if new_threshold == 0 {
            return Err(TreasuryError::ZeroThreshold);
        }
        let total_weight: u32 = Self::get_all_signers(env.clone())
            .iter()
            .map(|(_, weight)| weight)
            .sum();
        if new_threshold > total_weight {
            return Err(TreasuryError::ThresholdUnreachable);
        }
        env.storage()
            .instance()
            .set(&DataKey::Threshold, &new_threshold);
        env.events()
            .publish((Symbol::new(&env, "threshold_updated"),), new_threshold);
        Ok(())
    }

    /// Pauses the contract, blocking all state-mutating operations except admin functions (admin-only).
    /// Emits: `treasury_paused`.
    pub fn pause(env: Env, admin: Address) {
        require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events()
            .publish((Symbol::new(&env, "treasury_paused"),), admin);
    }

    /// Resumes normal operations after a pause (admin-only).
    /// Emits: `treasury_unpaused`.
    pub fn unpause(env: Env, admin: Address) {
        require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events()
            .publish((Symbol::new(&env, "treasury_unpaused"),), admin);
    }

    /// Configures the maximum amount withdrawable per rolling time window (admin-only).
    /// Applies to both `withdraw` (tracked per recipient `to`) and `withdraw_all` (tracked
    /// per `recipient`) — see `deposits.rs`. Passing `limit <= 0` disables the cap
    /// (the default at initialization is uncapped), trading off protection against a
    /// compromised-but-authorized withdrawer for the ability to move arbitrarily large
    /// legitimate withdrawals in a single call; admins needing large one-off withdrawals
    /// should raise the limit first rather than relying on an uncapped default long-term.
    /// Emits: `withdrawal_limit_set`.
    pub fn set_withdrawal_limit(env: Env, admin: Address, limit: i128, window_secs: u64) {
        require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::WithdrawalLimitPerWindow, &limit);
        env.storage()
            .instance()
            .set(&DataKey::WithdrawalWindowSecs, &window_secs);
        env.events().publish(
            (Symbol::new(&env, "withdrawal_limit_set"),),
            (limit, window_secs),
        );
    }

    /// Returns the currently configured `(limit, window_secs)`. `limit <= 0` means uncapped.
    pub fn get_withdrawal_limit(env: Env) -> (i128, u64) {
        let limit: i128 = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalLimitPerWindow)
            .unwrap_or(0);
        let window_secs: u64 = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalWindowSecs)
            .unwrap_or(0);
        (limit, window_secs)
    }
}

/// Maximum number of tokens allowed in the allowlist to prevent unbounded storage growth.
pub(crate) const MAX_ALLOWED_TOKENS: u32 = 20;

pub(crate) fn require_admin(env: &Env, admin: &Address) {
    admin.require_auth();
    let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
    if stored != *admin {
        soroban_sdk::panic_with_error!(env, TreasuryError::Unauthorized);
    }
}

pub(crate) fn require_not_paused(env: &Env) {
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false);
    if paused {
        soroban_sdk::panic_with_error!(env, TreasuryError::ContractPaused);
    }
}

#[cfg(test)]
extern crate std;
