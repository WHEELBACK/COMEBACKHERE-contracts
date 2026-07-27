use crate::{
    require_admin, DataKey, Settlement, SettlementHoldReason, SettlementStatus, TreasuryContract,
    TreasuryError,
};
use soroban_sdk::{contractimpl, Address, Env, Symbol};

#[contractimpl]
impl TreasuryContract {
    /// Places a pending settlement on hold with a `reason` code (admin-only).
    /// Errors: `SettlementNotFound`, `AlreadyOnHold`, `AlreadyExecuted`.
    /// Panics: `Unauthorized`.
    /// Emits: `settlement_held`.
    pub fn hold_settlement(
        env: Env,
        admin: Address,
        settlement_id: u64,
        reason: SettlementHoldReason,
    ) -> Result<(), TreasuryError> {
        require_admin(&env, &admin);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;
        if settlement.status == SettlementStatus::OnHold {
            return Err(TreasuryError::AlreadyOnHold);
        }
        if settlement.status != SettlementStatus::Pending {
            return Err(TreasuryError::AlreadyExecuted);
        }
        settlement.status = SettlementStatus::OnHold;
        settlement.hold_reason = reason.clone();
        env.storage()
            .persistent()
            .set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish(
            (Symbol::new(&env, "settlement_held"), settlement_id),
            reason,
        );
        Ok(())
    }

    /// Releases a held settlement back to `Pending` status (admin-only).
    /// Panics: `Unauthorized`, `SettlementNotFound`, `NotOnHold`.
    /// Emits: `settlement_released`.
    pub fn release_hold(env: Env, admin: Address, settlement_id: u64) {
        require_admin(&env, &admin);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::OnHold {
            panic!("NotOnHold");
        }
        settlement.status = SettlementStatus::Pending;
        settlement.hold_reason = SettlementHoldReason::None;
        env.storage()
            .persistent()
            .set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish(
            (Symbol::new(&env, "settlement_released"), settlement_id),
            settlement,
        );
    }
}
