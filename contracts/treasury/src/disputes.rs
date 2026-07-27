use crate::{
    require_admin, require_not_paused, DataKey, Dispute, DisputeStatus, Settlement,
    SettlementHoldReason, SettlementStatus, TreasuryContract,
};
use multisig::{require_authorized_signer, signer_weight};
use soroban_sdk::{contractimpl, Address, Env, Symbol, Vec};

#[contractimpl]
impl TreasuryContract {
    /// Raises a dispute against `settlement_id`, placing it on hold while the dispute is open.
    /// `expires_at` is a ledger UNIX timestamp (seconds) after which `expire_dispute` may be called.
    /// Preconditions: contract not paused; `amount` must be positive.
    /// Panics: `ContractPaused`, `InvalidAmount`.
    /// Emits: `dispute_raised`.
    pub fn raise_dispute(
        env: Env,
        claimant: Address,
        settlement_id: u64,
        counterparty: Address,
        amount: i128,
        expires_at: u64,
    ) -> u64 {
        require_not_paused(&env);
        claimant.require_auth();
        if amount <= 0 {
            panic!("InvalidAmount");
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
        let id = count + 1;
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
        };
        env.storage()
            .persistent()
            .set(&DataKey::Dispute(id), &dispute);
        env.storage().instance().set(&DataKey::DisputeCount, &id);
        env.events()
            .publish((Symbol::new(&env, "dispute_raised"), id), dispute);
        id
    }

    /// Transitions a `Raised` dispute to `Expired` after its deadline and releases the
    /// associated settlement from `OnHold` back to `Pending`.
    /// Panics: `Unauthorized`, `DisputeNotFound`, `DisputeAlreadyResolved`, `DisputeNotExpired`.
    /// Emits: `dispute_expired`.
    pub fn expire_dispute(env: Env, admin: Address, dispute_id: u64) {
        require_admin(&env, &admin);
        let mut dispute: Dispute = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| panic!("DisputeNotFound"));
        if dispute.status != DisputeStatus::Raised {
            panic!("DisputeAlreadyResolved");
        }
        if env.ledger().timestamp() < dispute.dispute_expires_at {
            panic!("DisputeNotExpired");
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
    }

    /// Returns the dispute with the given `dispute_id`.
    /// Panics: `DisputeNotFound`.
    pub fn get_dispute(env: Env, dispute_id: u64) -> Dispute {
        env.storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| panic!("DisputeNotFound"))
    }

    /// Resolves an open dispute in favour of claimant or counterparty (admin-only).
    /// When the last open dispute for a settlement is resolved, the settlement hold is released.
    /// Panics: `DisputeNotFound`, `DisputeAlreadyResolved`, `ContractPaused`.
    /// Emits: `dispute_resolved`.
    pub fn resolve_dispute(env: Env, admin: Address, dispute_id: u64, in_favor_of_claimant: bool) {
        require_admin(&env, &admin);
        require_not_paused(&env);
        let mut dispute: Dispute = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| panic!("DisputeNotFound"));
        if dispute.status != DisputeStatus::Raised {
            panic!("DisputeAlreadyResolved");
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

    /// Casts a weighted signer vote on a dispute; auto-resolves when cumulative weight meets threshold.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `DisputeNotFound`, `DisputeAlreadyResolved`,
    ///         `ResolutionDirectionMismatch`, `ThresholdNotConfigured`.
    /// Emits: `dispute_resolution_voted`.
    pub fn vote_dispute_resolution(
        env: Env,
        signer: Address,
        dispute_id: u64,
        in_favor_of_claimant: bool,
    ) {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut dispute: Dispute = env
            .storage()
            .persistent()
            .get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| panic!("DisputeNotFound"));
        if dispute.status != DisputeStatus::Raised {
            panic!("DisputeAlreadyResolved");
        }
        if dispute.resolution_weight == 0 {
            dispute.resolution_for_claimant = in_favor_of_claimant;
        } else if dispute.resolution_for_claimant != in_favor_of_claimant {
            panic!("ResolutionDirectionMismatch");
        }
        if !dispute.resolution_approvals.contains(&signer) {
            dispute.resolution_weight += signer_weight(&env, &signer);
            dispute.resolution_approvals.push_back(signer);
        }
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Threshold)
            .unwrap_or_else(|| panic!("ThresholdNotConfigured"));
        if dispute.resolution_weight >= threshold {
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
    }
}
