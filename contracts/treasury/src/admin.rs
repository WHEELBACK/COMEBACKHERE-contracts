use crate::{
    require_admin, DataKey, TreasuryContract, TreasuryContractArgs, TreasuryContractClient,
    TreasuryError,
};
use soroban_sdk::{contractimpl, Address, Env, Symbol};

#[contractimpl]
impl TreasuryContract {
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
