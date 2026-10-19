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
            .map(|(_, weight, _)| weight)
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

    /// Returns the current multisig approval threshold.
    pub fn get_threshold(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::Threshold)
            .unwrap_or(0)
    }

    /// Returns whether the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Returns the current admin address.
    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Admin).unwrap()
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

    /// Configures the settlement proposal expiry window (admin-only, #562).
    /// Emits: `settlement_expiry_updated`.
    pub fn set_settlement_expiry(env: Env, admin: Address, expiry_secs: u64) {
        require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::SettlementExpirySecs, &expiry_secs);
        env.events().publish(
            (Symbol::new(&env, "settlement_expiry_updated"),),
            expiry_secs,
        );
    }

    /// Returns the currently configured settlement expiry window in seconds.
    pub fn get_settlement_expiry(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::SettlementExpirySecs)
            .unwrap_or(7u64 * 24 * 60 * 60)
    }

    /// Pins the compliance contract instance `propose_settlement` consults (admin-only,
    /// #571). Optional and freely updatable: a treasury that never calls this does not
    /// gate proposals on compliance at all.
    /// Emits: `compliance_id_set`.
    pub fn set_compliance_id(env: Env, admin: Address, compliance_id: Address) {
        require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::ComplianceId, &compliance_id);
        env.events()
            .publish((Symbol::new(&env, "compliance_id_set"),), compliance_id);
    }

    /// Returns the pinned compliance contract instance, or `None` if proposals are
    /// not currently gated on compliance.
    pub fn get_compliance_id(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::ComplianceId)
    }

    /// Proposes a new admin for a two-step handover (admin-only, #741).
    /// The proposal must be accepted by `new_admin` via `accept_admin` before
    /// `expiry_secs` elapse, otherwise it expires and can be re-proposed.
    /// A new proposal supersedes any pending one.
    /// Emits: `admin_transfer_proposed`.
    pub fn transfer_admin(
        env: Env,
        admin: Address,
        new_admin: Address,
        expiry_secs: u64,
    ) -> Result<(), TreasuryError> {
        require_admin(&env, &admin);
        // A new proposal supersedes any pending one (a plain overwrite, as in the
        // settlement workflow's `transfer_admin`), so a mistyped or lost nominee
        // never blocks the handover until expiry.
        let expires_at = env.ledger().timestamp().saturating_add(expiry_secs);
        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &(new_admin.clone(), expires_at));
        env.events().publish(
            (Symbol::new(&env, "admin_transfer_proposed"),),
            (new_admin, expires_at),
        );
        Ok(())
    }

    /// Accepts a pending admin transfer (callable only by the proposed admin, #741).
    /// Errors: `NoPendingAdmin` if none exists or it has expired,
    /// `Unauthorized` if the caller is not the proposed admin.
    /// Emits: `admin_transferred`.
    pub fn accept_admin(env: Env, new_admin: Address) -> Result<(), TreasuryError> {
        let (pending, expires_at) = Self::get_pending_admin(env.clone())
            .ok_or(TreasuryError::NoPendingAdmin)?;
        if env.ledger().timestamp() >= expires_at {
            env.storage().instance().remove(&DataKey::PendingAdmin);
            return Err(TreasuryError::NoPendingAdmin);
        }
        new_admin.require_auth();
        if pending != new_admin {
            return Err(TreasuryError::Unauthorized);
        }
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.events()
            .publish((Symbol::new(&env, "admin_transferred"),), new_admin);
        Ok(())
    }

    /// Cancels a pending admin transfer (admin-only, #741).
    /// Errors: `NoPendingAdmin` if none exists.
    /// Emits: `admin_transfer_cancelled`.
    pub fn cancel_transfer_admin(env: Env, admin: Address) -> Result<(), TreasuryError> {
        require_admin(&env, &admin);
        if Self::get_pending_admin(env.clone()).is_none() {
            return Err(TreasuryError::NoPendingAdmin);
        }
        env.storage().instance().remove(&DataKey::PendingAdmin);
        env.events()
            .publish((Symbol::new(&env, "admin_transfer_cancelled"),), admin);
        Ok(())
    }

    /// Returns the pending `(new_admin, expires_at)` proposal, or `None` if there is
    /// no live proposal (expired proposals are treated as absent).
    pub fn get_pending_admin(env: Env) -> Option<(Address, u64)> {
        let pending: Option<(Address, u64)> = env.storage().instance().get(&DataKey::PendingAdmin);
        match pending {
            Some((new_admin, expires_at)) if env.ledger().timestamp() < expires_at => {
                Some((new_admin, expires_at))
            }
            _ => None,
        }
    }
}
