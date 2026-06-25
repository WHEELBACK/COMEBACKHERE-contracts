#![no_std]

mod multisig;
mod settlement;

pub use multisig::{
    DataKey, Dispute, DisputeStatus, RotationStatus, Settlement, SettlementHoldReason,
    SettlementStatus, SignerRotationProposal, TreasuryError,
};

use settlement::{require_authorized_signer, signer_weight};
use soroban_sdk::{contract, contractimpl, token, Address, Env, Symbol, Vec};

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
        }
    }
}

#[contract]
pub struct TreasuryContract;

#[contractimpl]
impl TreasuryContract {
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
        env.events().publish((Symbol::new(&env, "treasury_initialized"),), admin);
        Ok(())
    }

    pub fn set_signer(env: Env, admin: Address, signer: Address, weight: u32) {
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Signer(signer.clone()), &weight);
        env.events().publish((Symbol::new(&env, "signer_weight_set"), signer), weight);
    }

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
        };
        env.storage().persistent().set(&DataKey::Settlement(id), &settlement);
        env.storage().instance().set(&DataKey::SettlementCount, &id);
        env.events().publish((Symbol::new(&env, "settlement_proposed"), id), settlement);
        id
    }

    pub fn propose_partial_settlement(env: Env, signer: Address, merchant_address: Address, amount: i128) -> u64 {
        Self::propose_settlement(env, signer, merchant_address, amount)
    }

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

    pub fn get_settlement(env: Env, settlement_id: u64) -> Settlement {
        env.storage().persistent().get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"))
    }

    pub fn update_threshold(env: Env, admin: Address, new_threshold: u32) -> Result<(), TreasuryError> {
        Self::require_admin(&env, &admin);
        if new_threshold == 0 { return Err(TreasuryError::ZeroThreshold); }
        env.storage().instance().set(&DataKey::Threshold, &new_threshold);
        env.events().publish((Symbol::new(&env, "threshold_updated"),), new_threshold);
        Ok(())
    }

    pub fn pause(env: Env, admin: Address) {
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events().publish((Symbol::new(&env, "treasury_paused"),), admin);
    }

    pub fn unpause(env: Env, admin: Address) {
        Self::require_admin(&env, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events().publish((Symbol::new(&env, "treasury_unpaused"),), admin);
    }

    pub fn raise_dispute(env: Env, claimant: Address, settlement_id: u64, counterparty: Address, amount: i128) -> u64 {
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
        };
        env.storage().persistent().set(&DataKey::Dispute(id), &dispute);
        env.storage().instance().set(&DataKey::DisputeCount, &id);
        env.events().publish((Symbol::new(&env, "dispute_raised"), id), dispute);
        id
    }

    pub fn resolve_dispute(env: Env, admin: Address, dispute_id: u64, in_favor_of_claimant: bool) {
        Self::require_admin(&env, &admin);
        Self::require_not_paused(&env);
        let mut dispute: Dispute = env.storage().persistent().get(&DataKey::Dispute(dispute_id))
            .unwrap_or_else(|| panic!("DisputeNotFound"));
        if dispute.status != DisputeStatus::Raised { panic!("DisputeAlreadyResolved"); }
        dispute.status = if in_favor_of_claimant { DisputeStatus::ResolvedClaimant } else { DisputeStatus::ResolvedCounterparty };
        env.storage().persistent().set(&DataKey::Dispute(dispute_id), &dispute);
        env.events().publish((Symbol::new(&env, "dispute_resolved"), dispute_id), dispute);
    }

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

    pub fn get_allowed_tokens(env: Env) -> Vec<Address> {
        env.storage().instance().get(&DataKey::TokenAllowlist)
            .unwrap_or_else(|| Vec::new(&env))
    }

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

    pub fn update_merchant_payout_address(env: Env, merchant: Address, new_payout_address: Address) {
        Self::require_not_paused(&env);
        merchant.require_auth();
        env.storage().instance().set(&DataKey::MerchantPayoutAddress(merchant.clone()), &new_payout_address);
        env.events().publish((Symbol::new(&env, "merchant_payout_updated"), merchant), new_payout_address);
    }

    pub fn get_merchant_payout_address(env: Env, merchant: Address) -> Option<Address> {
        env.storage().instance().get(&DataKey::MerchantPayoutAddress(merchant))
    }

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
