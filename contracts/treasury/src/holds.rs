use crate::{
    require_admin, DataKey, Settlement, SettlementHoldReason, SettlementStatus, TreasuryContract,
    TreasuryContractArgs, TreasuryContractClient, TreasuryError,
};
use soroban_sdk::{contractimpl, Address, Env, Symbol};

/// Returns `true` when a settlement's hold is still active at the current ledger
/// timestamp.  A hold with no expiry is always active.  A hold whose
/// `HoldExpiry` timestamp has been reached or passed is treated as released.
fn is_hold_active(env: &Env, settlement_id: u64) -> bool {
    if let Some(expires_at) = env
        .storage()
        .persistent()
        .get::<_, u64>(&DataKey::HoldExpiry(settlement_id))
    {
        env.ledger().timestamp() < expires_at
    } else {
        // No expiry recorded → hold is permanent until explicitly released.
        true
    }
}

#[contractimpl]
impl TreasuryContract {
    /// Places a pending settlement on hold with a `reason` code and an optional
    /// `expires_at` timestamp (admin-only).
    ///
    /// When `expires_at` is `Some(t)`, the hold automatically lapses once
    /// `env.ledger().timestamp() >= t`.  Expired holds are treated as released
    /// everywhere the hold state is checked.  Pass `None` for a permanent hold
    /// that only lifts via an explicit `release_hold` call.
    ///
    /// Errors: `SettlementNotFound`, `AlreadyOnHold`, `AlreadyExecuted`.
    /// Panics: `Unauthorized`.
    /// Emits: `settlement_held`.
    pub fn hold_settlement(
        env: Env,
        admin: Address,
        settlement_id: u64,
        reason: SettlementHoldReason,
        expires_at: Option<u64>,
    ) -> Result<(), TreasuryError> {
        require_admin(&env, &admin);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;

        // If the settlement is already on hold and the hold has not expired,
        // reject as a duplicate.
        if settlement.status == SettlementStatus::OnHold && is_hold_active(&env, settlement_id) {
            return Err(TreasuryError::AlreadyOnHold);
        }

        if settlement.status != SettlementStatus::Pending
            && settlement.status != SettlementStatus::OnHold
        {
            return Err(TreasuryError::AlreadyExecuted);
        }

        settlement.status = SettlementStatus::OnHold;
        settlement.hold_reason = reason.clone();
        env.storage()
            .persistent()
            .set(&DataKey::Settlement(settlement_id), &settlement);

        // Store or clear the expiry timestamp.
        if let Some(ts) = expires_at {
            env.storage()
                .persistent()
                .set(&DataKey::HoldExpiry(settlement_id), &ts);
        } else {
            // Permanent hold — remove any leftover expiry from a previous hold.
            env.storage()
                .persistent()
                .remove(&DataKey::HoldExpiry(settlement_id));
        }

        env.events().publish(
            (Symbol::new(&env, "settlement_held"), settlement_id),
            (reason, expires_at),
        );
        Ok(())
    }

    /// Returns the hold reason for a settlement.  If the hold has expired, returns
    /// `SettlementHoldReason::None` (expired holds are treated as released).
    /// No authentication required.
    /// Errors: `SettlementNotFound`.
    pub fn get_hold_reason(
        env: Env,
        settlement_id: u64,
    ) -> Result<SettlementHoldReason, TreasuryError> {
        let settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;

        // If the settlement is on hold but its expiry has passed, report None —
        // the hold is treated as if it was already released.
        if settlement.status == SettlementStatus::OnHold && !is_hold_active(&env, settlement_id) {
            return Ok(SettlementHoldReason::None);
        }

        Ok(settlement.hold_reason)
    }

    /// Returns the hold expiry timestamp for a settlement, or `None` if the hold
    /// has no expiry (permanent hold) or the settlement is not on hold.
    /// No authentication required.
    /// Errors: `SettlementNotFound`.
    pub fn get_hold_expiry(
        env: Env,
        settlement_id: u64,
    ) -> Result<Option<u64>, TreasuryError> {
        // Ensure the settlement exists.
        let _: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;
        Ok(env
            .storage()
            .persistent()
            .get::<_, u64>(&DataKey::HoldExpiry(settlement_id)))
    }

    /// Releases a held settlement back to `Pending` status (admin-only).
    ///
    /// Also succeeds when the hold has already expired, allowing admins to
    /// explicitly clean up the storage entry of a lapsed hold.
    ///
    /// Errors: `SettlementNotFound`, `NotOnHold`.
    /// Panics: `Unauthorized`.
    /// Emits: `settlement_released`.
    pub fn release_hold(env: Env, admin: Address, settlement_id: u64) -> Result<(), TreasuryError> {
        require_admin(&env, &admin);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;
        if settlement.status != SettlementStatus::OnHold {
            return Err(TreasuryError::NotOnHold);
        }
        settlement.status = SettlementStatus::Pending;
        settlement.hold_reason = SettlementHoldReason::None;
        env.storage()
            .persistent()
            .set(&DataKey::Settlement(settlement_id), &settlement);
        // Clean up expiry regardless of whether it was set.
        env.storage()
            .persistent()
            .remove(&DataKey::HoldExpiry(settlement_id));
        env.events().publish(
            (Symbol::new(&env, "settlement_released"), settlement_id),
            settlement,
        );
        Ok(())
    }
}
