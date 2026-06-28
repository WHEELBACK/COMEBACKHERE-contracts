#![no_std]

pub use multisig::{
    DataKey, Dispute, DisputeStatus, RotationStatus, Settlement, SettlementHoldReason,
    SettlementStatus, SignerRotationProposal, TreasuryError,
};

use multisig::{require_authorized_signer, signer_weight};
use soroban_sdk::{contract, contractimpl, token, Address, Env, Symbol, Vec};

const SETTLEMENT_TTL: u64 = 7 * 24 * 60 * 60;

impl TreasuryError {
    fn panic(&self) -> ! {
        match self {
            TreasuryError::AlreadyInitialized => panic!("AlreadyInitialized"),
            TreasuryError::ZeroThreshold => panic!("ZeroThreshold"),
            TreasuryError::SettlementNotFound => panic!("SettlementNotFound"),
            TreasuryError::AlreadyExecuted => panic!("AlreadyExecuted"),
            TreasuryError::ThresholdNotMet => panic!("ThresholdNotMet"),
            TreasuryError::ThresholdNotConfigured => panic!("ThresholdNotConfigured"),
            TreasuryError::InvalidAmount => panic!("InvalidAmount"),
            TreasuryError::ContractPaused => panic!("ContractPaused"),
            TreasuryError::Unauthorized => panic!("Unauthorized"),
            TreasuryError::UnauthorizedSigner => panic!("UnauthorizedSigner"),
            TreasuryError::InvalidTokenContract => panic!("InvalidTokenContract"),
            TreasuryError::TokenNotAllowed => panic!("TokenNotAllowed"),
            TreasuryError::RotationNotFound => panic!("RotationNotFound"),
            TreasuryError::RotationAlreadyExecuted => panic!("RotationAlreadyExecuted"),
            TreasuryError::SettlementOnHold => panic!("SettlementOnHold"),
            TreasuryError::DisputeNotExpired => panic!("DisputeNotExpired"),
        }
    }
}

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
    /// Initialises the treasury with `admin` as owner and `threshold` as the multisig approval
    /// weight required to execute settlements.
    /// Errors: `AlreadyInitialized`, `ZeroThreshold`.
    /// Emits: `treasury_initialized`.
    pub fn initialize(env: Env, admin: Address, threshold: u32) -> Result<(), TreasuryError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(TreasuryError::AlreadyInitialized);
        }
        if threshold == 0 {
            return Err(TreasuryError::ZeroThreshold);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Threshold, &threshold);
        env.storage().instance().set(&DataKey::SettlementCount, &0u64);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::DisputeCount, &0u64);
        env.storage().instance().set(&DataKey::Signer(admin.clone()), &1u32);
        let mut signer_list = Vec::new(&env);
        signer_list.push_back(admin.clone());
        env.storage().instance().set(&DataKey::SignerList, &signer_list);
        env.events().publish((Symbol::new(&env, "treasury_initialized"),), admin);
        Ok(())
    }

    /// Registers or updates the approval weight of `signer` (admin-only). Weight 0 deactivates the signer.
    /// Emits: `signer_weight_set`.
    pub fn set_signer(env: Env, admin: Address, signer: Address, weight: u32) {
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Signer(signer.clone()), &weight);
        let mut list: Vec<Address> = env.storage().instance()
            .get(&DataKey::SignerList).unwrap_or_else(|| Vec::new(&env));
        if weight > 0 {
            if !list.contains(&signer) {
                list.push_back(signer.clone());
                env.storage().instance().set(&DataKey::SignerList, &list);
            }
        } else {
            let mut updated = Vec::new(&env);
            for s in list.iter() {
                if s != signer { updated.push_back(s); }
            }
            env.storage().instance().set(&DataKey::SignerList, &updated);
        }
        env.events().publish((Symbol::new(&env, "signer_weight_set"), signer), weight);
    }

    /// Proposes a new settlement of `amount` tokens payable to `merchant_address`.
    /// Preconditions: contract not paused; `signer` must be an authorised signer with non-zero weight.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `InvalidAmount`.
    /// Emits: `settlement_proposed`.
    pub fn propose_settlement(env: Env, signer: Address, merchant_address: Address, amount: i128) -> u64 {
        Self::require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        if amount <= 0 { panic!("InvalidAmount"); }
        let count: u64 = env.storage().instance().get(&DataKey::SettlementCount).unwrap_or(0);
        let id = count + 1;
        let mut approvals = Vec::new(&env);
        let weight = signer_weight(&env, &signer);
        approvals.push_back(signer);
        let settlement = Settlement {
            id, merchant_address, amount, approvals,
            approval_weight: weight,
            status: SettlementStatus::Pending,
            hold_reason: SettlementHoldReason::None,
            proposed_at: env.ledger().timestamp(),
        };
        env.storage().persistent().set(&DataKey::Settlement(id), &settlement);
        env.storage().instance().set(&DataKey::SettlementCount, &id);
        env.events().publish((Symbol::new(&env, "settlement_proposed"), id), settlement);
        id
    }

    /// Alias of `propose_settlement` for partial-settlement workflows.
    pub fn propose_partial_settlement(env: Env, signer: Address, merchant_address: Address, amount: i128) -> u64 {
        Self::propose_settlement(env, signer, merchant_address, amount)
    }

    /// Adds `signer`'s weight to the approval set of a pending settlement.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`.
    /// Emits: `settlement_approved`.
    pub fn approve_settlement(env: Env, signer: Address, settlement_id: u64) -> Settlement {
        Self::require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env.storage().persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::Pending { panic!("AlreadyExecuted"); }
        if !settlement.approvals.contains(&signer) {
            settlement.approval_weight += signer_weight(&env, &signer);
            settlement.approvals.push_back(signer);
        }
        env.storage().persistent().set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish((Symbol::new(&env, "settlement_approved"), settlement_id), settlement.clone());
        settlement
    }

    /// Approves a pending settlement with a `partial_amount` cap; accumulates `signer`'s weight.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`, `InvalidAmount`.
    /// Emits: `settlement_partial_approved`.
    pub fn approve_partial_settlement(env: Env, signer: Address, settlement_id: u64, partial_amount: i128) -> Settlement {
        Self::require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env.storage().persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::Pending { panic!("AlreadyExecuted"); }
        if partial_amount <= 0 || partial_amount >= settlement.amount { panic!("InvalidAmount"); }
        if !settlement.approvals.contains(&signer) {
            settlement.approval_weight += signer_weight(&env, &signer);
            settlement.approvals.push_back(signer);
        }
        env.storage().persistent().set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish((Symbol::new(&env, "settlement_partial_approved"), settlement_id), settlement.clone());
        settlement
    }

    /// Transfers the settlement amount to the merchant via `token_contract`.
    /// Preconditions: not paused; approval weight meets threshold; token is on allowlist (if non-empty).
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `SettlementOnHold`,
    ///         `AlreadyExecuted`, `ThresholdNotConfigured`, `ThresholdNotMet`,
    ///         `InvalidTokenContract`, `TokenNotAllowed`.
    /// Emits: `settlement_executed`.
    pub fn execute_settlement(env: Env, signer: Address, settlement_id: u64, token_contract: Address) {
        Self::require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env.storage().persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status == SettlementStatus::OnHold { panic!("SettlementOnHold"); }
        if settlement.status != SettlementStatus::Pending { panic!("AlreadyExecuted"); }
        let threshold: u32 = env.storage().instance().get(&DataKey::Threshold)
            .unwrap_or_else(|| panic!("ThresholdNotConfigured"));
        if threshold == 0 { panic!("ThresholdNotConfigured"); }
        if settlement.approval_weight < threshold { panic!("ThresholdNotMet"); }
        if token_contract == env.current_contract_address() { panic!("InvalidTokenContract"); }
        let allowlist: Vec<Address> = env.storage().instance()
            .get(&DataKey::TokenAllowlist).unwrap_or_else(|| Vec::new(&env));
        if !allowlist.is_empty() && !allowlist.contains(&token_contract) { panic!("TokenNotAllowed"); }
        let payout_address = env.storage().instance().get::<DataKey, Address>(&DataKey::MerchantPayoutAddress(settlement.merchant_address.clone()))
            .unwrap_or_else(|| settlement.merchant_address.clone());
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        token_client.transfer(&treasury, &payout_address, &settlement.amount);
        settlement.status = SettlementStatus::Executed;
        env.storage().persistent().set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish((Symbol::new(&env, "settlement_executed"), settlement_id), settlement);
    }

    /// Transfers `partial_amount` tokens to the merchant and marks the settlement as `PartiallyExecuted`.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`,
    ///         `InvalidAmount`, `ThresholdNotConfigured`, `ThresholdNotMet`.
    /// Emits: `settlement_partial_executed`.
    pub fn partially_execute_settlement(env: Env, signer: Address, settlement_id: u64, partial_amount: i128, token_contract: Address) {
        Self::require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env.storage().persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::Pending { panic!("AlreadyExecuted"); }
        if partial_amount <= 0 || partial_amount >= settlement.amount { panic!("InvalidAmount"); }
        let threshold: u32 = env.storage().instance().get(&DataKey::Threshold)
            .unwrap_or_else(|| panic!("ThresholdNotConfigured"));
        if threshold == 0 { panic!("ThresholdNotConfigured"); }
        if settlement.approval_weight < threshold { panic!("ThresholdNotMet"); }
        if token_contract == env.current_contract_address() { panic!("InvalidTokenContract"); }
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        token_client.transfer(&treasury, &settlement.merchant_address, &partial_amount);
        settlement.status = SettlementStatus::PartiallyExecuted;
        env.storage().persistent().set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish((Symbol::new(&env, "settlement_partial_executed"), settlement_id), settlement);
    }

    /// Cancels a pending settlement, preventing further approvals or execution.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `SettlementNotCancellable`.
    /// Emits: `settlement_cancelled`.
    pub fn cancel_settlement(env: Env, signer: Address, settlement_id: u64) {
        Self::require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env.storage().persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::Pending { panic!("SettlementNotCancellable"); }
        settlement.status = SettlementStatus::Cancelled;
        env.storage().persistent().set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish((Symbol::new(&env, "settlement_cancelled"), settlement_id), settlement);
    }

    pub fn batch_cancel_settlements(env: Env, admin: Address, ids: Vec<u64>) {
        Self::require_admin(&env, &admin);
        for id in ids.iter() {
            let settlement_opt: Option<Settlement> = env.storage().persistent()
                .get(&DataKey::Settlement(id));
            if let Some(mut settlement) = settlement_opt {
                if settlement.status == SettlementStatus::Pending {
                    settlement.status = SettlementStatus::Cancelled;
                    env.storage().persistent().set(&DataKey::Settlement(id), &settlement);
                    env.events().publish((Symbol::new(&env, "settlement_cancelled"), id), settlement);
                }
                // non-pending settlements are silently skipped
            }
            // missing settlement IDs are silently skipped
        }
    }

    pub fn get_pending_settlements(env: Env) -> Vec<Settlement> {
        let count: u64 = env.storage().instance().get(&DataKey::SettlementCount).unwrap_or(0);
        let mut pending = Vec::new(&env);
        let mut id = 1u64;
        while id <= count {
            if let Some(settlement) = env.storage().persistent().get::<DataKey, Settlement>(&DataKey::Settlement(id)) {
                if settlement.status == SettlementStatus::Pending { pending.push_back(settlement); }
            }
            id += 1;
        }
        pending
    }

    /// Returns a page of pending settlements: skips the first `start` entries and returns up to `limit`.
    pub fn get_pending_settlements_page(env: Env, start: u64, limit: u64) -> Vec<Settlement> {
        let count: u64 = env.storage().instance().get(&DataKey::SettlementCount).unwrap_or(0);
        let mut page = Vec::new(&env);
        let mut skipped: u64 = 0;
        let mut id = 1u64;
        while id <= count {
            if let Some(settlement) = env.storage().persistent().get::<DataKey, Settlement>(&DataKey::Settlement(id)) {
                if settlement.status == SettlementStatus::Pending {
                    if skipped < start { skipped += 1; }
                    else if (page.len() as u64) < limit { page.push_back(settlement); }
                    else { break; }
                }
            }
            id += 1;
        }
        page
    }

    /// Returns the settlement with the given `settlement_id`.
    /// Panics: `SettlementNotFound`.
    pub fn get_settlement(env: Env, settlement_id: u64) -> Settlement {
        env.storage().persistent().get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"))
    }

    /// Expires a pending settlement whose TTL has elapsed (admin-only).
    /// Panics: `SettlementNotFound`, `AlreadyExecuted`, `TtlNotElapsed`.
    /// Emits: `settlement_expired`.
    pub fn expire_settlement(env: Env, admin: Address, settlement_id: u64) {
        Self::require_admin(&env, &admin);
        let mut settlement: Settlement = env.storage().persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::Pending { panic!("AlreadyExecuted"); }
        if env.ledger().timestamp() <= settlement.proposed_at + SETTLEMENT_TTL {
            panic!("TtlNotElapsed");
        }
        settlement.status = SettlementStatus::Expired;
        env.storage().persistent().set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish((Symbol::new(&env, "settlement_expired"), settlement_id), settlement);
    }

    /// Updates the multisig approval threshold required to execute settlements (admin-only).
    /// Errors: `ZeroThreshold`.
    /// Emits: `threshold_updated`.
    pub fn update_threshold(env: Env, admin: Address, new_threshold: u32) -> Result<(), TreasuryError> {
        Self::require_admin(&env, &admin);
        if new_threshold == 0 { return Err(TreasuryError::ZeroThreshold); }
        env.storage().instance().set(&DataKey::Threshold, &new_threshold);
        env.events().publish((Symbol::new(&env, "threshold_updated"),), new_threshold);
        Ok(())
    }

    /// Pauses the contract, blocking all state-mutating operations except admin functions (admin-only).
    /// Emits: `treasury_paused`.
    pub fn pause(env: Env, admin: Address) {
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events().publish((Symbol::new(&env, "treasury_paused"),), admin);
    }

    /// Resumes normal operations after a pause (admin-only).
    /// Emits: `treasury_unpaused`.
    pub fn unpause(env: Env, admin: Address) {
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events().publish((Symbol::new(&env, "treasury_unpaused"),), admin);
    }

    /// Raises a dispute against `settlement_id`, placing it on hold while the dispute is open.
    /// `expires_at` is a ledger UNIX timestamp (seconds) after which `expire_dispute` may be called.
    /// Preconditions: contract not paused; `amount` must be positive.
    /// Panics: `ContractPaused`, `InvalidAmount`.
    /// Emits: `dispute_raised`.
    pub fn raise_dispute(env: Env, claimant: Address, settlement_id: u64, counterparty: Address, amount: i128, expires_at: u64) -> u64 {
        Self::require_not_paused(&env);
        claimant.require_auth();
        if amount <= 0 { panic!("InvalidAmount"); }
        if let Some(mut settlement) = env.storage().persistent().get::<DataKey, Settlement>(&DataKey::Settlement(settlement_id)) {
            if settlement.status == SettlementStatus::Pending {
                settlement.status = SettlementStatus::OnHold;
                env.storage().persistent().set(&DataKey::Settlement(settlement_id), &settlement);
            }
        }
        let count: u64 = env.storage().instance().get(&DataKey::DisputeCount).unwrap_or(0);
        let id = count + 1;
        let dispute = Dispute {
            id, settlement_id, claimant, counterparty, amount,
            status: DisputeStatus::Raised,
            resolution_approvals: Vec::new(&env),
            resolution_weight: 0,
            resolution_for_claimant: false,
            dispute_expires_at: expires_at,
        };
        env.storage().persistent().set(&DataKey::Dispute(id), &dispute);
        env.storage().instance().set(&DataKey::DisputeCount, &id);
        env.events().publish((Symbol::new(&env, "dispute_raised"), id), dispute);
        id
    }

    /// Transitions a `Raised` dispute to `Expired` after its deadline and releases the
    /// associated settlement from `OnHold` back to `Pending`.
    /// Panics: `Unauthorized`, `DisputeNotFound`, `DisputeAlreadyResolved`, `DisputeNotExpired`.
    /// Emits: `dispute_expired`.
    pub fn expire_dispute(env: Env, admin: Address, dispute_id: u64) {
        Self::require_admin(&env, &admin);
        let mut dispute: Dispute = env.storage().persistent().get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| panic!("DisputeNotFound"));
        if dispute.status != DisputeStatus::Raised { panic!("DisputeAlreadyResolved"); }
        if env.ledger().timestamp() < dispute.dispute_expires_at { panic!("DisputeNotExpired"); }
        dispute.status = DisputeStatus::Expired;
        env.storage().persistent().set(&DataKey::Dispute(dispute_id), &dispute);
        if let Some(mut settlement) = env.storage().persistent().get::<DataKey, Settlement>(&DataKey::Settlement(dispute.settlement_id)) {
            if settlement.status == SettlementStatus::OnHold {
                settlement.status = SettlementStatus::Pending;
                settlement.hold_reason = SettlementHoldReason::None;
                env.storage().persistent().set(&DataKey::Settlement(dispute.settlement_id), &settlement);
            }
        }
        env.events().publish((Symbol::new(&env, "dispute_expired"), dispute_id), dispute);
    }

    /// Returns the dispute with the given `dispute_id`.
    /// Panics: `DisputeNotFound`.
    pub fn get_dispute(env: Env, dispute_id: u64) -> Dispute {
        env.storage().persistent().get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| panic!("DisputeNotFound"))
    }

    /// Resolves an open dispute in favour of claimant or counterparty (admin-only).
    /// When the last open dispute for a settlement is resolved, the settlement hold is released.
    /// Panics: `DisputeNotFound`, `DisputeAlreadyResolved`, `ContractPaused`.
    /// Emits: `dispute_resolved`.
    pub fn resolve_dispute(env: Env, admin: Address, dispute_id: u64, in_favor_of_claimant: bool) {
        Self::require_admin(&env, &admin);
        Self::require_not_paused(&env);
        let mut dispute: Dispute = env.storage().persistent().get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| panic!("DisputeNotFound"));
        if dispute.status != DisputeStatus::Raised { panic!("DisputeAlreadyResolved"); }
        dispute.status = if in_favor_of_claimant { DisputeStatus::ResolvedClaimant } else { DisputeStatus::ResolvedCounterparty };
        let settlement_id = dispute.settlement_id;
        env.storage().persistent().set(&DataKey::Dispute(dispute_id), &dispute);
        env.events().publish((Symbol::new(&env, "dispute_resolved"), dispute_id), dispute);
        if let Some(mut settlement) = env.storage().persistent().get::<DataKey, Settlement>(&DataKey::Settlement(settlement_id)) {
            if settlement.status == SettlementStatus::OnHold {
                let dispute_count: u64 = env.storage().instance().get(&DataKey::DisputeCount).unwrap_or(0);
                let mut has_open = false;
                let mut i = 1u64;
                while i <= dispute_count {
                    if let Some(d) = env.storage().persistent().get::<DataKey, Dispute>(&DataKey::Dispute(i)) {
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
                    env.storage().persistent().set(&DataKey::Settlement(settlement_id), &settlement);
                }
            }
        }
    }

    /// Casts a weighted signer vote on a dispute; auto-resolves when cumulative weight meets threshold.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `DisputeNotFound`, `DisputeAlreadyResolved`,
    ///         `ResolutionDirectionMismatch`, `ThresholdNotConfigured`.
    /// Emits: `dispute_resolution_voted`.
    pub fn vote_dispute_resolution(env: Env, signer: Address, dispute_id: u64, in_favor_of_claimant: bool) {
        Self::require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut dispute: Dispute = env.storage().persistent().get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| panic!("DisputeNotFound"));
        if dispute.status != DisputeStatus::Raised { panic!("DisputeAlreadyResolved"); }
        if dispute.resolution_weight == 0 {
            dispute.resolution_for_claimant = in_favor_of_claimant;
        } else if dispute.resolution_for_claimant != in_favor_of_claimant {
            panic!("ResolutionDirectionMismatch");
        }
        if !dispute.resolution_approvals.contains(&signer) {
            dispute.resolution_weight += signer_weight(&env, &signer);
            dispute.resolution_approvals.push_back(signer);
        }
        let threshold: u32 = env.storage().instance().get(&DataKey::Threshold)
            .unwrap_or_else(|| panic!("ThresholdNotConfigured"));
        if dispute.resolution_weight >= threshold {
            dispute.status = if dispute.resolution_for_claimant { DisputeStatus::ResolvedClaimant } else { DisputeStatus::ResolvedCounterparty };
        }
        env.storage().persistent().set(&DataKey::Dispute(dispute_id), &dispute);
        env.events().publish((Symbol::new(&env, "dispute_resolution_voted"), dispute_id), dispute);
    }

    /// Deposits `amount` tokens from `from` into the treasury via `token_contract`.
    /// Panics: `ContractPaused`, `InvalidAmount`.
    /// Emits: `deposit`.
    pub fn deposit(env: Env, from: Address, token_contract: Address, amount: i128) {
        Self::require_not_paused(&env);
        from.require_auth();
        if amount <= 0 { panic!("InvalidAmount"); }
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        token_client.transfer(&from, &treasury, &amount);
        let mut balance: i128 = env.storage().persistent().get(&DataKey::Balance(from.clone())).unwrap_or(0);
        balance += amount;
        env.storage().persistent().set(&DataKey::Balance(from.clone()), &balance);
        env.events().publish((Symbol::new(&env, "deposit"), from), amount);
    }

    /// Withdraws `amount` tokens from the treasury to `to` via `token_contract`.
    /// Panics: `ContractPaused`, `InvalidAmount`, `InsufficientBalance`.
    /// Emits: `withdraw`.
    pub fn withdraw(env: Env, to: Address, token_contract: Address, amount: i128) {
        Self::require_not_paused(&env);
        to.require_auth();
        if amount <= 0 { panic!("InvalidAmount"); }
        let mut balance: i128 = env.storage().persistent().get(&DataKey::Balance(to.clone())).unwrap_or(0);
        if balance < amount { panic!("InsufficientBalance"); }
        balance -= amount;
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &balance);
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        token_client.transfer(&treasury, &to, &amount);
        env.events().publish((Symbol::new(&env, "withdraw"), to), amount);
    }

    /// Drains the full token balance of the treasury to `recipient` (admin-only, paused-only emergency drain).
    /// Panics: `Unauthorized`, `NotPaused`.
    /// Emits: `treasury_drained`.
    pub fn withdraw_all(env: Env, admin: Address, token_contract: Address, recipient: Address) {
        Self::require_admin(&env, &admin);
        let paused: bool = env.storage().instance().get(&DataKey::Paused).unwrap_or(false);
        if !paused { panic!("NotPaused"); }
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        let balance = token_client.balance(&treasury);
        if balance > 0 {
            token_client.transfer(&treasury, &recipient, &balance);
        }
        env.events().publish((Symbol::new(&env, "treasury_drained"),), recipient);
    }

    /// Adds `token` to the settlement token allowlist (admin-only). No-op if already present.
    /// Emits: `token_allowed`.
    pub fn add_allowed_token(env: Env, admin: Address, token: Address) {
        Self::require_admin(&env, &admin);
        let mut allowlist: Vec<Address> = env.storage().instance()
            .get(&DataKey::TokenAllowlist).unwrap_or_else(|| Vec::new(&env));
        if !allowlist.contains(&token) {
            allowlist.push_back(token.clone());
            env.storage().instance().set(&DataKey::TokenAllowlist, &allowlist);
            env.events().publish((Symbol::new(&env, "token_allowed"),), token);
        }
    }

    /// Removes `token` from the settlement token allowlist (admin-only).
    /// Emits: `token_removed`.
    pub fn remove_allowed_token(env: Env, admin: Address, token: Address) {
        Self::require_admin(&env, &admin);
        let allowlist: Vec<Address> = env.storage().instance()
            .get(&DataKey::TokenAllowlist).unwrap_or_else(|| Vec::new(&env));
        let mut updated = Vec::new(&env);
        for t in allowlist.iter() {
            if t != token { updated.push_back(t); }
        }
        env.storage().instance().set(&DataKey::TokenAllowlist, &updated);
        env.events().publish((Symbol::new(&env, "token_removed"),), token);
    }

    /// Returns the current list of allowed token contract addresses.
    pub fn get_allowed_tokens(env: Env) -> Vec<Address> {
        env.storage().instance().get(&DataKey::TokenAllowlist)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns all registered signers and their current weights.
    pub fn get_all_signers(env: Env) -> Vec<(Address, u32)> {
        let list: Vec<Address> = env.storage().instance()
            .get(&DataKey::SignerList).unwrap_or_else(|| Vec::new(&env));
        let mut result = Vec::new(&env);
        for signer in list.iter() {
            let weight: u32 = env.storage().instance()
                .get(&DataKey::Signer(signer.clone())).unwrap_or(0);
            result.push_back((signer, weight));
        }
        result
    }

    /// Proposes replacing `old_signer` with `new_signer` in the authorised signer set.
    /// Emits: `rotation_proposed`.
    pub fn propose_signer_rotation(env: Env, proposer: Address, old_signer: Address, new_signer: Address) -> u64 {
        require_authorized_signer(&env, &proposer);
        let count: u64 = env.storage().instance().get(&DataKey::RotationCount).unwrap_or(0);
        let id = count + 1;
        let weight = signer_weight(&env, &proposer);
        let mut approvals = Vec::new(&env);
        approvals.push_back(proposer);
        let proposal = SignerRotationProposal {
            id, old_signer, new_signer, approvals,
            approval_weight: weight,
            status: RotationStatus::Pending,
        };
        env.storage().persistent().set(&DataKey::SignerRotation(id), &proposal);
        env.storage().instance().set(&DataKey::RotationCount, &id);
        env.events().publish((Symbol::new(&env, "rotation_proposed"), id), proposal);
        id
    }

    /// Approves a pending signer rotation; executes the swap when cumulative weight meets threshold.
    /// Panics: `UnauthorizedSigner`, `RotationNotFound`, `RotationAlreadyExecuted`.
    /// Emits: `rotation_approved`; additionally `rotation_executed` when threshold is met.
    pub fn approve_signer_rotation(env: Env, approver: Address, rotation_id: u64) -> SignerRotationProposal {
        require_authorized_signer(&env, &approver);
        let mut proposal: SignerRotationProposal = env.storage().persistent()
            .get(&DataKey::SignerRotation(rotation_id))
            .unwrap_or_else(|| panic!("RotationNotFound"));
        if proposal.status != RotationStatus::Pending { panic!("RotationAlreadyExecuted"); }
        if !proposal.approvals.contains(&approver) {
            proposal.approval_weight += signer_weight(&env, &approver);
            proposal.approvals.push_back(approver);
        }
        let threshold: u32 = env.storage().instance().get(&DataKey::Threshold).unwrap_or(1);
        if proposal.approval_weight >= threshold {
            let old_weight = signer_weight(&env, &proposal.old_signer);
            env.storage().instance().set(&DataKey::Signer(proposal.new_signer.clone()), &old_weight);
            env.storage().instance().set(&DataKey::Signer(proposal.old_signer.clone()), &0u32);
            proposal.status = RotationStatus::Executed;
            env.events().publish((Symbol::new(&env, "rotation_executed"), rotation_id), proposal.clone());
        }
        env.storage().persistent().set(&DataKey::SignerRotation(rotation_id), &proposal);
        env.events().publish((Symbol::new(&env, "rotation_approved"), rotation_id), proposal.clone());
        proposal
    }

    /// Sets or updates the payout address for `merchant` (merchant-only, not paused).
    /// Emits: `merchant_payout_updated`.
    pub fn update_merchant_payout_address(env: Env, merchant: Address, new_payout_address: Address) {
        Self::require_not_paused(&env);
        merchant.require_auth();
        env.storage().instance().set(&DataKey::MerchantPayoutAddress(merchant.clone()), &new_payout_address);
        env.events().publish((Symbol::new(&env, "merchant_payout_updated"), merchant), new_payout_address);
    }

    /// Returns the registered payout address for `merchant`, or `None` if not set.
    pub fn get_merchant_payout_address(env: Env, merchant: Address) -> Option<Address> {
        env.storage().instance().get(&DataKey::MerchantPayoutAddress(merchant))
    }

    /// Places a pending settlement on hold with a `reason` code (admin-only).
    /// Panics: `Unauthorized`, `SettlementNotFound`, `AlreadyExecuted`.
    /// Emits: `settlement_held`.
    pub fn hold_settlement(env: Env, admin: Address, settlement_id: u64, reason: SettlementHoldReason) {
        Self::require_admin(&env, &admin);
        let mut settlement: Settlement = env.storage().persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::Pending { panic!("AlreadyExecuted"); }
        settlement.status = SettlementStatus::OnHold;
        settlement.hold_reason = reason.clone();
        env.storage().persistent().set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish((Symbol::new(&env, "settlement_held"), settlement_id), reason);
    }

    /// Releases a held settlement back to `Pending` status (admin-only).
    /// Panics: `Unauthorized`, `SettlementNotFound`, `NotOnHold`.
    /// Emits: `settlement_released`.
    pub fn release_hold(env: Env, admin: Address, settlement_id: u64) {
        Self::require_admin(&env, &admin);
        let mut settlement: Settlement = env.storage().persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::OnHold { panic!("NotOnHold"); }
        settlement.status = SettlementStatus::Pending;
        settlement.hold_reason = SettlementHoldReason::None;
        env.storage().persistent().set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish((Symbol::new(&env, "settlement_released"), settlement_id), settlement);
    }

    fn require_admin(env: &Env, admin: &Address) {
        admin.require_auth();
        let stored: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        if stored != *admin { panic!("Unauthorized"); }
    }

    fn require_not_paused(env: &Env) {
        let paused: bool = env.storage().instance().get(&DataKey::Paused).unwrap_or(false);
        if paused { panic!("ContractPaused"); }
    }
}

#[cfg(test)]
extern crate std;
