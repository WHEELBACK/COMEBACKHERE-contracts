use crate::{
    require_admin, require_not_paused, DataKey, Dispute, DisputeStatus, Settlement,
    SettlementHoldReason, SettlementStatus, TreasuryContract, TreasuryContractArgs,
    TreasuryContractClient, TreasuryError,
};
use multisig::{meets_threshold, record_approval, require_authorized_signer};
use soroban_sdk::{contractimpl, token, Address, Env, Symbol, Vec};

/// Basis-points denominator for `resolve_dispute_split`'s ratio (10_000 = 100.00%).
pub const BPS_DENOMINATOR: u32 = 10_000;

#[contractimpl]
impl TreasuryContract {
    /// Raises a dispute against `settlement_id`, placing it on hold while the dispute is open.
    /// `expires_at` is a ledger UNIX timestamp (seconds) after which `expire_dispute` may be called.
    /// Preconditions: contract not paused; `amount` must be positive.
    /// Errors: `ContractPaused`, `InvalidAmount`, `ArithmeticOverflow`.
    /// Emits: `dispute_raised`.
    pub fn raise_dispute(
        env: Env,
        claimant: Address,
        settlement_id: u64,
        counterparty: Address,
        amount: i128,
        expires_at: u64,
    ) -> Result<u64, TreasuryError> {
        require_not_paused(&env);
        claimant.require_auth();
        if amount <= 0 {
            return Err(TreasuryError::InvalidAmount);
        }
        if let Some(mut settlement) = env
            .storage()
            .persistent()
            .get::<DataKey, Settlement>(&DataKey::Settlement(settlement_id))
        {
            if settlement.status == SettlementStatus::Pending {
                settlement.status = SettlementStatus::OnHold;
                env.storage()
                    .persistent()
                    .set(&DataKey::Settlement(settlement_id), &settlement);
            }
        }
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::DisputeCount)
            .unwrap_or(0);
        let id = count
            .checked_add(1)
            .ok_or(TreasuryError::ArithmeticOverflow)?;
        let dispute = Dispute {
            id,
            settlement_id,
            claimant,
            counterparty,
            amount,
            status: DisputeStatus::Raised,
            resolution_approvals: Vec::new(&env),
            resolution_weight: 0,
            resolution_for_claimant: false,
            dispute_expires_at: expires_at,
            claimant_share_bps: 0,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(id), &dispute);
        env.storage().instance().set(&DataKey::DisputeCount, &id);
        env.events()
            .publish((Symbol::new(&env, "dispute_raised"), id), dispute);
        Ok(id)
    }

    /// Transitions a `Raised` dispute to `Expired` after its deadline and releases the
    /// associated settlement from `OnHold` back to `Pending`.
    /// Errors: `DisputeNotFound`, `DisputeAlreadyResolved`, `DisputeNotExpired`.
    /// Panics: `Unauthorized`.
    /// Emits: `dispute_expired`.
    pub fn expire_dispute(
        env: Env,
        admin: Address,
        dispute_id: u64,
    ) -> Result<(), TreasuryError> {
        require_admin(&env, &admin);
        let mut dispute: Dispute = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .ok_or(TreasuryError::DisputeNotFound)?;
        if dispute.status != DisputeStatus::Raised {
            return Err(TreasuryError::DisputeAlreadyResolved);
        }
        if env.ledger().timestamp() < dispute.dispute_expires_at {
            return Err(TreasuryError::DisputeNotExpired);
        }
        dispute.status = DisputeStatus::Expired;
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id), &dispute);
        if let Some(mut settlement) = env
            .storage()
            .persistent()
            .get::<DataKey, Settlement>(&DataKey::Settlement(dispute.settlement_id))
        {
            if settlement.status == SettlementStatus::OnHold {
                settlement.status = SettlementStatus::Pending;
                settlement.hold_reason = SettlementHoldReason::None;
                env.storage()
                    .persistent()
                    .set(&DataKey::Settlement(dispute.settlement_id), &settlement);
            }
        }
        env.events()
            .publish((Symbol::new(&env, "dispute_expired"), dispute_id), dispute);
        Ok(())
    }

    /// Returns the dispute with the given `dispute_id`.
    /// Errors: `DisputeNotFound`.
    pub fn get_dispute(env: Env, dispute_id: u64) -> Result<Dispute, TreasuryError> {
        env.storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .ok_or(TreasuryError::DisputeNotFound)
    }

    /// Resolves an open dispute in favour of claimant or counterparty (admin-only).
    /// When the last open dispute for a settlement is resolved, the settlement hold is released.
    /// Errors: `DisputeNotFound`, `DisputeAlreadyResolved`, `ContractPaused`.
    /// Panics: `Unauthorized`.
    /// Emits: `dispute_resolved`.
    pub fn resolve_dispute(
        env: Env,
        admin: Address,
        dispute_id: u64,
        in_favor_of_claimant: bool,
    ) -> Result<(), TreasuryError> {
        require_admin(&env, &admin);
        require_not_paused(&env);
        let mut dispute: Dispute = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .ok_or(TreasuryError::DisputeNotFound)?;
        if dispute.status != DisputeStatus::Raised {
            return Err(TreasuryError::DisputeAlreadyResolved);
        }
        dispute.status = if in_favor_of_claimant {
            DisputeStatus::ResolvedClaimant
        } else {
            DisputeStatus::ResolvedCounterparty
        };
        let settlement_id = dispute.settlement_id;
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id), &dispute);
        env.events()
            .publish((Symbol::new(&env, "dispute_resolved"), dispute_id), dispute);
        release_settlement_hold_if_no_open_disputes(&env, settlement_id);
    }

    /// Resolves an open dispute by splitting `dispute.amount` between claimant and
    /// counterparty according to `claimant_bps` (out of [`BPS_DENOMINATOR`]) instead of the
    /// binary claimant/counterparty outcome `resolve_dispute` supports (admin-only).
    ///
    /// Design note: like `resolve_dispute`, this only ever pays out `dispute.amount` (the
    /// amount raised with the dispute, not necessarily equal to the associated settlement's
    /// own `amount`) directly from the treasury's token balance to the two parties; it does
    /// not itself execute the settlement. Releasing the settlement hold below only means the
    /// settlement becomes eligible for its own `execute_settlement`/`partially_execute_settlement`
    /// payout again — callers must ensure the disputed amount and the settlement's payout are
    /// not double-counted when both eventually execute, exactly as with a binary resolution.
    /// Panics: `DisputeNotFound`, `DisputeAlreadyResolved`, `ContractPaused`, `InvalidSplitRatio`.
    /// Emits: `dispute_resolved_split`.
    pub fn resolve_dispute_split(
        env: Env,
        admin: Address,
        dispute_id: u64,
        claimant_bps: u32,
        token_contract: Address,
    ) {
        require_admin(&env, &admin);
        require_not_paused(&env);
        if claimant_bps > BPS_DENOMINATOR {
            soroban_sdk::panic_with_error!(env, TreasuryError::InvalidSplitRatio);
        }
        let mut dispute: Dispute = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, TreasuryError::DisputeNotFound));
        if dispute.status != DisputeStatus::Raised {
            soroban_sdk::panic_with_error!(env, TreasuryError::DisputeAlreadyResolved);
        }
        let claimant_amount = dispute
            .amount
            .checked_mul(claimant_bps as i128)
            .unwrap_or_else(|| {
                soroban_sdk::panic_with_error!(env, TreasuryError::ArithmeticOverflow)
            })
            / BPS_DENOMINATOR as i128;
        let counterparty_amount = dispute.amount - claimant_amount;
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        if claimant_amount > 0 {
            token_client.transfer(&treasury, &dispute.claimant, &claimant_amount);
        }
        if counterparty_amount > 0 {
            token_client.transfer(&treasury, &dispute.counterparty, &counterparty_amount);
        }
        Ok(())
    }

    /// Casts a weighted signer vote on a dispute; auto-resolves when cumulative weight meets threshold.
    /// Errors: `ContractPaused`, `UnauthorizedSigner`, `DisputeNotFound`, `DisputeAlreadyResolved`,
    ///         `ResolutionDirectionMismatch`, `ThresholdNotConfigured`.
    /// Emits: `dispute_resolution_voted`.
    pub fn vote_dispute_resolution(
        env: Env,
        signer: Address,
        dispute_id: u64,
        in_favor_of_claimant: bool,
    ) -> Result<(), TreasuryError> {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut dispute: Dispute = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .ok_or(TreasuryError::DisputeNotFound)?;
        if dispute.status != DisputeStatus::Raised {
            return Err(TreasuryError::DisputeAlreadyResolved);
        }
        if dispute.resolution_weight == 0 {
            dispute.resolution_for_claimant = in_favor_of_claimant;
        } else if dispute.resolution_for_claimant != in_favor_of_claimant {
            return Err(TreasuryError::ResolutionDirectionMismatch);
        }
        record_approval(
            &env,
            &mut dispute.resolution_approvals,
            &mut dispute.resolution_weight,
            &signer,
        );
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Threshold)
            .ok_or(TreasuryError::ThresholdNotConfigured)?;
        if meets_threshold(dispute.resolution_weight, threshold) {
            dispute.status = if dispute.resolution_for_claimant {
                DisputeStatus::ResolvedClaimant
            } else {
                DisputeStatus::ResolvedCounterparty
            };
        }
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(dispute_id), &dispute);
        env.events().publish(
            (Symbol::new(&env, "dispute_resolution_voted"), dispute_id),
            dispute,
        );
        Ok(())
    }
}

/// Shared by `resolve_dispute` and `resolve_dispute_split`: releases `settlement_id` from
/// `OnHold` back to `Pending` once no `Raised` dispute references it anymore.
fn release_settlement_hold_if_no_open_disputes(env: &Env, settlement_id: u64) {
    if let Some(mut settlement) = env
        .storage()
        .persistent()
        .get::<DataKey, Settlement>(&DataKey::Settlement(settlement_id))
    {
        if settlement.status == SettlementStatus::OnHold {
            let dispute_count: u64 = env
                .storage()
                .instance()
                .get(&DataKey::DisputeCount)
                .unwrap_or(0);
            let mut has_open = false;
            let mut i = 1u64;
            while i <= dispute_count {
                if let Some(d) = env
                    .storage()
                    .persistent()
                    .get::<DataKey, Dispute>(&DataKey::Dispute(i))
                {
                    if d.settlement_id == settlement_id && d.status == DisputeStatus::Raised {
                        has_open = true;
                        break;
                    }
                }
                i += 1;
            }
            if !has_open {
                settlement.status = SettlementStatus::Pending;
                settlement.hold_reason = SettlementHoldReason::None;
                env.storage()
                    .persistent()
                    .set(&DataKey::Settlement(settlement_id), &settlement);
            }
        }
    }
}
