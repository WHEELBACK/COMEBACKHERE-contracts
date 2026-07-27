use crate::{
    require_admin, require_not_paused, DataKey, Settlement, SettlementHoldReason,
    SettlementStatus, TreasuryContract,
};
use multisig::{require_authorized_signer, signer_weight};
use soroban_sdk::{contractimpl, token, Address, Env, Symbol, Vec};

const SETTLEMENT_TTL: u64 = 7 * 24 * 60 * 60;

#[contractimpl]
impl TreasuryContract {
    /// Proposes a new settlement of `amount` tokens payable to `merchant_address`.
    /// Preconditions: contract not paused; `signer` must be an authorised signer with non-zero weight.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `InvalidAmount`.
    /// Emits: `settlement_proposed`.
    pub fn propose_settlement(
        env: Env,
        signer: Address,
        merchant_address: Address,
        amount: i128,
    ) -> u64 {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        if amount <= 0 {
            panic!("InvalidAmount");
        }
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::SettlementCount)
            .unwrap_or(0);
        let id = count + 1;
        let mut approvals = Vec::new(&env);
        let weight = signer_weight(&env, &signer);
        approvals.push_back(signer);
        let settlement = Settlement {
            id,
            merchant_address,
            amount,
            approvals,
            approval_weight: weight,
            status: SettlementStatus::Pending,
            hold_reason: SettlementHoldReason::None,
            proposed_at: env.ledger().timestamp(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Settlement(id), &settlement);
        env.storage().instance().set(&DataKey::SettlementCount, &id);
        env.events()
            .publish((Symbol::new(&env, "settlement_proposed"), id), settlement);
        id
    }

    /// Alias of `propose_settlement` for partial-settlement workflows.
    pub fn propose_partial_settlement(
        env: Env,
        signer: Address,
        merchant_address: Address,
        amount: i128,
    ) -> u64 {
        Self::propose_settlement(env, signer, merchant_address, amount)
    }

    /// Adds `signer`'s weight to the approval set of a pending settlement.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`.
    /// Emits: `settlement_approved`.
    pub fn approve_settlement(env: Env, signer: Address, settlement_id: u64) -> Settlement {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::Pending {
            panic!("AlreadyExecuted");
        }
        if !settlement.approvals.contains(&signer) {
            settlement.approval_weight += signer_weight(&env, &signer);
            settlement.approvals.push_back(signer);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish(
            (Symbol::new(&env, "settlement_approved"), settlement_id),
            settlement.clone(),
        );
        settlement
    }

    /// Approves a pending settlement with a `partial_amount` cap; accumulates `signer`'s weight.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`, `InvalidAmount`.
    /// Emits: `settlement_partial_approved`.
    pub fn approve_partial_settlement(
        env: Env,
        signer: Address,
        settlement_id: u64,
        partial_amount: i128,
    ) -> Settlement {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::Pending {
            panic!("AlreadyExecuted");
        }
        if partial_amount <= 0 || partial_amount >= settlement.amount {
            panic!("InvalidAmount");
        }
        if !settlement.approvals.contains(&signer) {
            settlement.approval_weight += signer_weight(&env, &signer);
            settlement.approvals.push_back(signer);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish(
            (
                Symbol::new(&env, "settlement_partial_approved"),
                settlement_id,
            ),
            settlement.clone(),
        );
        settlement
    }

    /// Transfers the settlement amount to the merchant via `token_contract`.
    /// Preconditions: not paused; approval weight meets threshold; token is on allowlist (if non-empty).
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `SettlementOnHold`,
    ///         `AlreadyExecuted`, `ThresholdNotConfigured`, `ThresholdNotMet`,
    ///         `InvalidTokenContract`, `TokenNotAllowed`.
    /// Emits: `settlement_executed`.
    pub fn execute_settlement(
        env: Env,
        signer: Address,
        settlement_id: u64,
        token_contract: Address,
    ) {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status == SettlementStatus::OnHold {
            panic!("SettlementOnHold");
        }
        if settlement.status != SettlementStatus::Pending {
            panic!("AlreadyExecuted");
        }
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Threshold)
            .unwrap_or_else(|| panic!("ThresholdNotConfigured"));
        if threshold == 0 {
            panic!("ThresholdNotConfigured");
        }
        if settlement.approval_weight < threshold {
            panic!("ThresholdNotMet");
        }
        if token_contract == env.current_contract_address() {
            panic!("InvalidTokenContract");
        }
        let allowlist: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::TokenAllowlist)
            .unwrap_or_else(|| Vec::new(&env));
        if !allowlist.is_empty() && !allowlist.contains(&token_contract) {
            panic!("TokenNotAllowed");
        }
        let payout_address = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::MerchantPayoutAddress(
                settlement.merchant_address.clone(),
            ))
            .unwrap_or_else(|| settlement.merchant_address.clone());
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        token_client.transfer(&treasury, &payout_address, &settlement.amount);
        settlement.status = SettlementStatus::Executed;
        env.storage()
            .persistent()
            .set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish(
            (Symbol::new(&env, "settlement_executed"), settlement_id),
            settlement,
        );
    }

    /// Transfers `partial_amount` tokens to the merchant and marks the settlement as `PartiallyExecuted`.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`,
    ///         `InvalidAmount`, `ThresholdNotConfigured`, `ThresholdNotMet`.
    /// Emits: `settlement_partial_executed`.
    pub fn partially_execute_settlement(
        env: Env,
        signer: Address,
        settlement_id: u64,
        partial_amount: i128,
        token_contract: Address,
    ) {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::Pending {
            panic!("AlreadyExecuted");
        }
        if partial_amount <= 0 || partial_amount >= settlement.amount {
            panic!("InvalidAmount");
        }
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Threshold)
            .unwrap_or_else(|| panic!("ThresholdNotConfigured"));
        if threshold == 0 {
            panic!("ThresholdNotConfigured");
        }
        if settlement.approval_weight < threshold {
            panic!("ThresholdNotMet");
        }
        if token_contract == env.current_contract_address() {
            panic!("InvalidTokenContract");
        }
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        token_client.transfer(&treasury, &settlement.merchant_address, &partial_amount);
        settlement.status = SettlementStatus::PartiallyExecuted;
        env.storage()
            .persistent()
            .set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish(
            (
                Symbol::new(&env, "settlement_partial_executed"),
                settlement_id,
            ),
            settlement,
        );
    }

    /// Cancels a pending settlement, preventing further approvals or execution.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `SettlementNotCancellable`.
    /// Emits: `settlement_cancelled`.
    pub fn cancel_settlement(env: Env, signer: Address, settlement_id: u64) {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::Pending {
            panic!("SettlementNotCancellable");
        }
        settlement.status = SettlementStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish(
            (Symbol::new(&env, "settlement_cancelled"), settlement_id),
            settlement,
        );
    }

    pub fn batch_cancel_settlements(env: Env, admin: Address, ids: Vec<u64>) {
        require_admin(&env, &admin);
        for id in ids.iter() {
            let settlement_opt: Option<Settlement> =
                env.storage().persistent().get(&DataKey::Settlement(id));
            if let Some(mut settlement) = settlement_opt {
                if settlement.status == SettlementStatus::Pending {
                    settlement.status = SettlementStatus::Cancelled;
                    env.storage()
                        .persistent()
                        .set(&DataKey::Settlement(id), &settlement);
                    env.events()
                        .publish((Symbol::new(&env, "settlement_cancelled"), id), settlement);
                }
                // non-pending settlements are silently skipped
            }
            // missing settlement IDs are silently skipped
        }
    }

    pub fn get_pending_settlements(env: Env) -> Vec<Settlement> {
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::SettlementCount)
            .unwrap_or(0);
        let mut pending = Vec::new(&env);
        let mut id = 1u64;
        while id <= count {
            if let Some(settlement) = env
                .storage()
                .persistent()
                .get::<DataKey, Settlement>(&DataKey::Settlement(id))
            {
                if settlement.status == SettlementStatus::Pending {
                    pending.push_back(settlement);
                }
            }
            id += 1;
        }
        pending
    }

    /// Returns a page of pending settlements: skips the first `start` entries and returns up to `limit`.
    pub fn get_pending_settlements_page(env: Env, start: u64, limit: u64) -> Vec<Settlement> {
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::SettlementCount)
            .unwrap_or(0);
        let mut page = Vec::new(&env);
        let mut skipped: u64 = 0;
        let mut id = 1u64;
        while id <= count {
            if let Some(settlement) = env
                .storage()
                .persistent()
                .get::<DataKey, Settlement>(&DataKey::Settlement(id))
            {
                if settlement.status == SettlementStatus::Pending {
                    if skipped < start {
                        skipped += 1;
                    } else if (page.len() as u64) < limit {
                        page.push_back(settlement);
                    } else {
                        break;
                    }
                }
            }
            id += 1;
        }
        page
    }

    /// Returns the settlement with the given `settlement_id`.
    /// Panics: `SettlementNotFound`.
    pub fn get_settlement(env: Env, settlement_id: u64) -> Settlement {
        env.storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"))
    }

    /// Expires a pending settlement whose TTL has elapsed (admin-only).
    /// Panics: `SettlementNotFound`, `AlreadyExecuted`, `TtlNotElapsed`.
    /// Emits: `settlement_expired`.
    pub fn expire_settlement(env: Env, admin: Address, settlement_id: u64) {
        require_admin(&env, &admin);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| panic!("SettlementNotFound"));
        if settlement.status != SettlementStatus::Pending {
            panic!("AlreadyExecuted");
        }
        if env.ledger().timestamp() <= settlement.proposed_at + SETTLEMENT_TTL {
            panic!("TtlNotElapsed");
        }
        settlement.status = SettlementStatus::Expired;
        env.storage()
            .persistent()
            .set(&DataKey::Settlement(settlement_id), &settlement);
        env.events().publish(
            (Symbol::new(&env, "settlement_expired"), settlement_id),
            settlement,
        );
    }

    /// Sets or updates the payout address for `merchant` (merchant-only, not paused).
    /// Emits: `merchant_payout_updated`.
    pub fn update_merchant_payout_address(
        env: Env,
        merchant: Address,
        new_payout_address: Address,
    ) {
        require_not_paused(&env);
        merchant.require_auth();
        env.storage().instance().set(
            &DataKey::MerchantPayoutAddress(merchant.clone()),
            &new_payout_address,
        );
        env.events().publish(
            (Symbol::new(&env, "merchant_payout_updated"), merchant),
            new_payout_address,
        );
    }

    /// Returns the registered payout address for `merchant`, or `None` if not set.
    pub fn get_merchant_payout_address(env: Env, merchant: Address) -> Option<Address> {
        env.storage()
            .instance()
            .get(&DataKey::MerchantPayoutAddress(merchant))
    }

    /// Adds `token` to the settlement token allowlist (admin-only). No-op if already present.
    /// Emits: `token_allowed`.
    pub fn add_allowed_token(env: Env, admin: Address, token: Address) {
        require_admin(&env, &admin);
        let mut allowlist: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::TokenAllowlist)
            .unwrap_or_else(|| Vec::new(&env));
        if !allowlist.contains(&token) {
            allowlist.push_back(token.clone());
            env.storage()
                .instance()
                .set(&DataKey::TokenAllowlist, &allowlist);
            env.events()
                .publish((Symbol::new(&env, "token_allowed"),), token);
        }
    }

    /// Removes `token` from the settlement token allowlist (admin-only).
    /// Emits: `token_removed`.
    pub fn remove_allowed_token(env: Env, admin: Address, token: Address) {
        require_admin(&env, &admin);
        let allowlist: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::TokenAllowlist)
            .unwrap_or_else(|| Vec::new(&env));
        let mut updated = Vec::new(&env);
        for t in allowlist.iter() {
            if t != token {
                updated.push_back(t);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::TokenAllowlist, &updated);
        env.events()
            .publish((Symbol::new(&env, "token_removed"),), token);
    }

    /// Returns the current list of allowed token contract addresses.
    pub fn get_allowed_tokens(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::TokenAllowlist)
            .unwrap_or_else(|| Vec::new(&env))
    }
}
