use crate::{
    require_admin, require_not_paused, write_settlement, DataKey, MaybeAddress, Settlement,
    SettlementHoldReason, SettlementStatus, TreasuryContract, TreasuryContractArgs,
    TreasuryContractClient, TreasuryError, MAX_ALLOWED_TOKENS,
};
use compliance_client::ComplianceClient;
use multisig::{
    meets_threshold, record_approval, require_authorized_signer,
    revoke_approval as revoke_signer_approval, signer_weight,
};
use soroban_sdk::{contractimpl, token, Address, Env, Symbol, Vec};

/// Maximum number of settlement IDs accepted per batch call, consistent with
/// the batch caps used elsewhere in the workspace (see #8/#21).
const MAX_BATCH_SIZE: u32 = 50;

/// Maximum number of payout address history entries retained per merchant (#579).
/// Older entries are dropped (FIFO) once this cap is reached to prevent unbounded
/// storage growth while still keeping a useful audit trail.
const MAX_PAYOUT_HISTORY: u32 = 10;
/// Whether any settlement is still `Pending`. Stops at the first one found.
fn has_pending_settlement(env: &Env) -> bool {
    let count: u64 = env
        .storage()
        .instance()
        .get(&DataKey::SettlementCount)
        .unwrap_or(0);
    let mut id = 1u64;
    while id <= count {
        if let Some(settlement) = env
            .storage()
            .persistent()
            .get::<DataKey, Settlement>(&DataKey::Settlement(id))
        {
            if settlement.status == SettlementStatus::Pending {
                return true;
            }
        }
        id += 1;
    }
    false
}

#[contractimpl]
impl TreasuryContract {
    /// Proposes a new settlement of `amount` tokens payable to `merchant_address`.
    /// An optional `execution_deadline` (Unix timestamp) may be set; if non-zero,
    /// `execute_settlement` will reject calls after that timestamp even when approvals
    /// are complete. Pass `0` for no deadline.
    /// Preconditions: contract not paused; `signer` must be an authorised signer with non-zero weight.
    /// If a compliance contract has been pinned via `set_compliance_id` (#571), `merchant_address`
    /// must currently pass `Compliance::is_allowed` or the proposal is rejected outright — this
    /// surfaces a blocked recipient immediately instead of after signers have already spent time
    /// approving a proposal that could never execute. The execution-time check (enforced
    /// separately, by a compliance-gated workflow contract) still applies on top of this: an
    /// address can be blocked *between* proposal and execution, which this does not catch.
    /// Without a pinned compliance contract, this check is skipped entirely (pre-#571 behavior).
    /// Panics: `ContractPaused`, `UnauthorizedSigner`.
    /// Errors: `InvalidAmount`, `ArithmeticOverflow`, `ComplianceCheckFailed`.
    /// Emits: `settlement_proposed`.
    pub fn propose_settlement(
        env: Env,
        signer: Address,
        merchant_address: Address,
        amount: i128,
        execution_deadline: u64,
    ) -> Result<u64, TreasuryError> {
        propose(
            env,
            signer,
            merchant_address,
            amount,
            MaybeAddress::None,
            execution_deadline,
        )
    }

    /// Proposes a new settlement capturing the intended `token` at proposal time.
    /// Behaves identically to `propose_settlement` except that `token` is stored on the
    /// settlement and surfaced by `get_pending_metrics` to group metrics per token.
    /// Use `MaybeAddress::None` if the token is not yet known (equivalent to
    /// calling `propose_settlement`).
    /// Preconditions: contract not paused; `signer` must be an authorised signer with non-zero weight.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`.
    /// Errors: `InvalidAmount`, `ArithmeticOverflow`.
    /// Emits: `settlement_proposed`.
    pub fn propose_settlement_with_token(
        env: Env,
        signer: Address,
        merchant_address: Address,
        amount: i128,
        token: MaybeAddress,
    ) -> Result<u64, TreasuryError> {
        propose(env, signer, merchant_address, amount, token, 0)
    }
}

/// Shared body of `propose_settlement` and `propose_settlement_with_token`: the
/// former carries an execution deadline (#590), the latter a token (#585), and a
/// settlement records both.
fn propose(
    env: Env,
    signer: Address,
    merchant_address: Address,
    amount: i128,
    token: MaybeAddress,
    execution_deadline: u64,
) -> Result<u64, TreasuryError> {
    require_not_paused(&env);
    require_authorized_signer(&env, &signer);
    if amount <= 0 {
        return Err(TreasuryError::InvalidAmount);
    }
    if let Some(compliance_id) = env
        .storage()
        .instance()
        .get::<DataKey, Address>(&DataKey::ComplianceId)
    {
        ComplianceClient::new(&env, &compliance_id)
            .require_allowed_for_treasury(&merchant_address)?;
    }
    let count: u64 = env
        .storage()
        .instance()
        .get(&DataKey::SettlementCount)
        .unwrap_or(0);
    let id = count
        .checked_add(1)
        .ok_or(TreasuryError::ArithmeticOverflow)?;
    let mut approvals = Vec::new(&env);
    let mut weight = 0u32;
    record_approval(&env, &mut approvals, &mut weight, &signer);
    let settlement = Settlement {
        id,
        merchant_address,
        amount,
        approvals,
        approval_weight: weight,
        status: SettlementStatus::Pending,
        hold_reason: SettlementHoldReason::None,
        proposed_at: env.ledger().timestamp(),
        execution_deadline,
        token,
    };
    write_settlement(&env, id, &settlement);
    env.storage().instance().set(&DataKey::SettlementCount, &id);
    push_merchant_settlement(&env, &settlement.merchant_address, id);
    env.events()
        .publish((Symbol::new(&env, "settlement_proposed"), id), settlement);
    Ok(id)
}

#[contractimpl]
impl TreasuryContract {
    /// Alias of `propose_settlement` for partial-settlement workflows.
    pub fn propose_partial_settlement(
        env: Env,
        signer: Address,
        merchant_address: Address,
        amount: i128,
        execution_deadline: u64,
    ) -> Result<u64, TreasuryError> {
        Self::propose_settlement(env, signer, merchant_address, amount, execution_deadline)
    }

    /// Adds `signer`'s weight to the approval set of a pending settlement.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`.
    /// Errors: `SettlementNotFound`, `AlreadyExecuted`.
    /// Emits: `settlement_approved`.
    pub fn approve_settlement(
        env: Env,
        signer: Address,
        settlement_id: u64,
    ) -> Result<Settlement, TreasuryError> {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;
        if settlement.status != SettlementStatus::Pending {
            return Err(TreasuryError::AlreadyExecuted);
        }
        record_approval(
            &env,
            &mut settlement.approvals,
            &mut settlement.approval_weight,
            &signer,
        );
        write_settlement(&env, settlement_id, &settlement);
        env.events().publish(
            (Symbol::new(&env, "settlement_approved"), settlement_id),
            settlement.clone(),
        );
        Ok(settlement)
    }

    /// Withdraws `signer`'s earlier approval of a pending settlement, subtracting their weight
    /// from the settlement's approval weight. If that drops the total below the threshold,
    /// `execute_settlement` is blocked again until enough approvals are re-collected.
    /// Only possible while the settlement is still `Pending` (i.e. before execution).
    /// Panics: `ContractPaused`, `UnauthorizedSigner`.
    /// Errors: `SettlementNotFound`, `AlreadyExecuted`, `ApprovalNotFound`.
    /// Emits: `settlement_approval_revoked`.
    pub fn revoke_approval(
        env: Env,
        signer: Address,
        settlement_id: u64,
    ) -> Result<Settlement, TreasuryError> {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;
        if settlement.status != SettlementStatus::Pending {
            return Err(TreasuryError::AlreadyExecuted);
        }
        if !revoke_signer_approval(
            &env,
            &mut settlement.approvals,
            &mut settlement.approval_weight,
            &signer,
        ) {
            return Err(TreasuryError::ApprovalNotFound);
        }
        write_settlement(&env, settlement_id, &settlement);
        env.events().publish(
            (
                Symbol::new(&env, "settlement_approval_revoked"),
                settlement_id,
            ),
            (signer, settlement.clone()),
        );
        Ok(settlement)
    }

    /// Approves multiple pending settlements in one call, reducing transaction overhead for
    /// high-volume signers. IDs that don't exist or aren't `Pending` are skipped rather than
    /// aborting the whole batch; a single bad ID never rolls back the others' approvals.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`.
    /// Errors: `BatchTooLarge`, `WeightOverflow`.
    /// Emits: `settlement_approved` for each settlement actually approved.
    pub fn batch_approve_settlements(
        env: Env,
        signer: Address,
        ids: Vec<u64>,
    ) -> Result<Vec<Settlement>, TreasuryError> {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        if ids.len() > MAX_BATCH_SIZE {
            return Err(TreasuryError::BatchTooLarge);
        }
        let weight = signer_weight(&env, &signer);
        let mut approved = Vec::new(&env);
        for id in ids.iter() {
            let settlement_opt: Option<Settlement> =
                env.storage().persistent().get(&DataKey::Settlement(id));
            if let Some(mut settlement) = settlement_opt {
                if settlement.status == SettlementStatus::Pending {
                    if !settlement.approvals.contains(&signer) {
                        settlement.approval_weight = settlement
                            .approval_weight
                            .checked_add(weight)
                            .ok_or(TreasuryError::WeightOverflow)?;
                        settlement.approvals.push_back(signer.clone());
                    }
                    write_settlement(&env, id, &settlement);
                    env.events().publish(
                        (Symbol::new(&env, "settlement_approved"), id),
                        settlement.clone(),
                    );
                    approved.push_back(settlement);
                }
                // non-pending settlements are silently skipped
            }
            // missing settlement IDs are silently skipped
        }
        Ok(approved)
    }

    /// Approves a pending settlement with a `partial_amount` cap; accumulates `signer`'s weight.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`.
    /// Errors: `SettlementNotFound`, `AlreadyExecuted`, `InvalidAmount`.
    /// Emits: `settlement_partial_approved`.
    pub fn approve_partial_settlement(
        env: Env,
        signer: Address,
        settlement_id: u64,
        partial_amount: i128,
    ) -> Result<Settlement, TreasuryError> {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;
        if settlement.status != SettlementStatus::Pending {
            return Err(TreasuryError::AlreadyExecuted);
        }
        if partial_amount <= 0 || partial_amount >= settlement.amount {
            return Err(TreasuryError::InvalidAmount);
        }
        let approved_total: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PartialApprovedTotal(settlement_id))
            .unwrap_or(0);
        if approved_total + partial_amount > settlement.amount {
            return Err(TreasuryError::InvalidAmount);
        }
        env.storage().persistent().set(
            &DataKey::PartialApprovedTotal(settlement_id),
            &(approved_total + partial_amount),
        );
        record_approval(
            &env,
            &mut settlement.approvals,
            &mut settlement.approval_weight,
            &signer,
        );
        write_settlement(&env, settlement_id, &settlement);
        env.events().publish(
            (
                Symbol::new(&env, "settlement_partial_approved"),
                settlement_id,
            ),
            settlement.clone(),
        );
        Ok(settlement)
    }

    /// Transfers the settlement amount to the merchant via `token_contract`.
    /// Preconditions: not paused; approval weight meets threshold; token is on allowlist (if non-empty).
    /// Panics: `ContractPaused`, `UnauthorizedSigner`.
    /// Errors: `SettlementNotFound`, `SettlementOnHold`, `AlreadyExecuted`, `ThresholdNotConfigured`,
    ///         `ThresholdNotMet`, `InvalidTokenContract`, `TokenNotAllowed`.
    /// Emits: `settlement_executed`.
    /// Note: this function performs **no** compliance check — Treasury does not
    /// consult Compliance. Callers wanting a compliance-gated execution path should
    /// use `SettlementWorkflowContract::execute_with_compliance` (see
    /// `contracts/settlement-workflow`) instead, which gates this exact call behind
    /// `Compliance::is_allowed` and is the recommended, compliance-checked entry point
    /// for executing a settlement (per ARCHITECTURE.md's description of
    /// SettlementWorkflow's role).
    pub fn execute_settlement(
        env: Env,
        signer: Address,
        settlement_id: u64,
        token_contract: Address,
    ) -> Result<(), TreasuryError> {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;
        if settlement.status == SettlementStatus::OnHold {
            // Treat an expired hold as released — check whether the hold still
            // applies before rejecting execution.
            let hold_expired = env
                .storage()
                .persistent()
                .get::<_, u64>(&DataKey::HoldExpiry(settlement_id))
                .map(|expires_at| env.ledger().timestamp() >= expires_at)
                .unwrap_or(false);
            if !hold_expired {
                return Err(TreasuryError::SettlementOnHold);
            }
            // Hold has lapsed — treat as Pending for execution purposes.
            // The status remains OnHold in storage until explicitly released
            // (lazy evaluation, consistent with compliance's AllowedUntil pattern).
        }
        if settlement.status != SettlementStatus::Pending
            && settlement.status != SettlementStatus::OnHold
        {
            return Err(TreasuryError::AlreadyExecuted);
        }
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Threshold)
            .ok_or(TreasuryError::ThresholdNotConfigured)?;
        if threshold == 0 {
            return Err(TreasuryError::ThresholdNotConfigured);
        }
        if !meets_threshold(settlement.approval_weight, threshold) {
            return Err(TreasuryError::ThresholdNotMet);
        }
        // Enforce execution deadline: if the proposer set a non-zero deadline,
        // reject execution after that timestamp even when approvals are complete.
        if settlement.execution_deadline > 0
            && env.ledger().timestamp() > settlement.execution_deadline
        {
            return Err(TreasuryError::ExecutionDeadlineExceeded);
        }
        if token_contract == env.current_contract_address() {
            return Err(TreasuryError::InvalidTokenContract);
        }
        let allowlist: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::TokenAllowlist)
            .unwrap_or_else(|| Vec::new(&env));
        if !allowlist.is_empty() && !allowlist.contains(&token_contract) {
            return Err(TreasuryError::TokenNotAllowed);
        }
        // #757: enforce the recipient's compliance tier limit before funds move.
        crate::require_compliance_allowed(&env, &settlement.merchant_address, settlement.amount)?;
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
        write_settlement(&env, settlement_id, &settlement);
        env.events().publish(
            (Symbol::new(&env, "settlement_executed"), settlement_id),
            settlement,
        );
        Ok(())
    }

    /// Executes multiple pending settlements in one call. IDs that don't exist, aren't `Pending`,
    /// or don't yet meet the approval threshold are silently skipped rather than aborting the
    /// whole batch. Shares `MAX_BATCH_SIZE` with the other settlement batch entrypoints (see
    /// #725) since execution is the most instruction-expensive batch operation and must not use
    /// a looser cap than the others.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`, `BatchTooLarge`, `ThresholdNotConfigured`,
    ///         `InvalidTokenContract`, `TokenNotAllowed`.
    /// Emits: `settlement_executed` for each settlement actually executed.
    pub fn batch_execute_settlements(
        env: Env,
        signer: Address,
        ids: Vec<u64>,
        token_contract: Address,
    ) -> Vec<Settlement> {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        if ids.len() > MAX_BATCH_SIZE {
            panic!("BatchTooLarge");
        }
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Threshold)
            .unwrap_or_else(|| panic!("ThresholdNotConfigured"));
        if threshold == 0 {
            panic!("ThresholdNotConfigured");
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
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        let mut executed = Vec::new(&env);
        for id in ids.iter() {
            let settlement_opt: Option<Settlement> =
                env.storage().persistent().get(&DataKey::Settlement(id));
            if let Some(mut settlement) = settlement_opt {
                if settlement.status == SettlementStatus::Pending
                    && meets_threshold(settlement.approval_weight, threshold)
                {
                    let payout_address = env
                        .storage()
                        .instance()
                        .get::<DataKey, Address>(&DataKey::MerchantPayoutAddress(
                            settlement.merchant_address.clone(),
                        ))
                        .unwrap_or_else(|| settlement.merchant_address.clone());
                    token_client.transfer(&treasury, &payout_address, &settlement.amount);
                    settlement.status = SettlementStatus::Executed;
                    env.storage()
                        .persistent()
                        .set(&DataKey::Settlement(id), &settlement);
                    env.events().publish(
                        (Symbol::new(&env, "settlement_executed"), id),
                        settlement.clone(),
                    );
                    executed.push_back(settlement);
                }
                // on-hold, non-pending, or under-threshold settlements are silently skipped
            }
            // missing settlement IDs are silently skipped
        }
        executed
    }

    /// Read-only view of how far a partial settlement has progressed (#738).
    /// Returns `(approved_total, executed_total, remaining)`: the tracked
    /// `PartialApprovedTotal`, the `PartialExecutedTotal` already paid out, and
    /// `approved_total - executed_total` saturating at zero.
    /// Errors: `SettlementNotFound`.
    pub fn get_partial_progress(
        env: Env,
        settlement_id: u64,
    ) -> Result<(i128, i128, i128), TreasuryError> {
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Settlement(settlement_id))
        {
            return Err(TreasuryError::SettlementNotFound);
        }
        let approved: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PartialApprovedTotal(settlement_id))
            .unwrap_or(0);
        let executed: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PartialExecutedTotal(settlement_id))
            .unwrap_or(0);
        let remaining = if approved > executed { approved - executed } else { 0 };
        Ok((approved, executed, remaining))
    }

    /// Transfers `partial_amount` tokens to the merchant and marks the settlement as `PartiallyExecuted`.
    /// Cumulative executed amounts across multiple calls are tracked under
    /// `DataKey::PartialExecutedTotal` and accumulated with `checked_add` to prevent
    /// overflow from wrapping (#578). The cumulative total must never exceed the
    /// settlement's original `amount`; attempting to do so returns `InvalidAmount`.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`.
    /// Errors: `SettlementNotFound`, `AlreadyExecuted`, `InvalidAmount`, `ThresholdNotConfigured`,
    ///         `ThresholdNotMet`, `InvalidTokenContract`, `ArithmeticOverflow`.
    /// Emits: `settlement_partial_executed`.
    pub fn partially_execute_settlement(
        env: Env,
        signer: Address,
        settlement_id: u64,
        partial_amount: i128,
        token_contract: Address,
    ) -> Result<(), TreasuryError> {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;
        if settlement.status != SettlementStatus::Pending {
            return Err(TreasuryError::AlreadyExecuted);
        }
        if partial_amount <= 0 || partial_amount >= settlement.amount {
            return Err(TreasuryError::InvalidAmount);
        }
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Threshold)
            .ok_or(TreasuryError::ThresholdNotConfigured)?;
        if threshold == 0 {
            return Err(TreasuryError::ThresholdNotConfigured);
        }
        if !meets_threshold(settlement.approval_weight, threshold) {
            return Err(TreasuryError::ThresholdNotMet);
        }
        if token_contract == env.current_contract_address() {
            return Err(TreasuryError::InvalidTokenContract);
        }
        // Accumulate the cumulative executed total using checked arithmetic to
        // guard against overflow wrap-around (#578).
        let prev_executed: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PartialExecutedTotal(settlement_id))
            .unwrap_or(0i128);
        let new_executed = prev_executed
            .checked_add(partial_amount)
            .ok_or(TreasuryError::ArithmeticOverflow)?;
        if new_executed > settlement.amount {
            return Err(TreasuryError::InvalidAmount);
        }
        env.storage().persistent().set(
            &DataKey::PartialExecutedTotal(settlement_id),
            &new_executed,
        );
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        token_client.transfer(&treasury, &settlement.merchant_address, &partial_amount);
        settlement.status = SettlementStatus::PartiallyExecuted;
        write_settlement(&env, settlement_id, &settlement);
        env.events().publish(
            (
                Symbol::new(&env, "settlement_partial_executed"),
                settlement_id,
            ),
            settlement,
        );
        Ok(())
    }

    /// Cancels a pending settlement, preventing further approvals or execution.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`.
    /// Errors: `SettlementNotFound`, `SettlementNotCancellable`.
    /// Emits: `settlement_cancelled`.
    pub fn cancel_settlement(
        env: Env,
        signer: Address,
        settlement_id: u64,
    ) -> Result<(), TreasuryError> {
        require_not_paused(&env);
        require_authorized_signer(&env, &signer);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;
        if settlement.status != SettlementStatus::Pending {
            return Err(TreasuryError::SettlementNotCancellable);
        }
        settlement.status = SettlementStatus::Cancelled;
        write_settlement(&env, settlement_id, &settlement);
        env.events().publish(
            (Symbol::new(&env, "settlement_cancelled"), settlement_id),
            settlement,
        );
        Ok(())
    }

    /// Cancels all pending settlements in `ids` (admin-only). IDs that are missing or not
    /// `Pending` are skipped rather than causing the whole batch to fail.
    /// Returns the ids that were actually cancelled.
    /// Errors: `Unauthorized`.
    /// Emits: `settlement_cancelled` per cancelled settlement.
    pub fn batch_cancel_settlements(
        env: Env,
        admin: Address,
        ids: Vec<u64>,
    ) -> Result<Vec<u64>, TreasuryError> {
        require_admin(&env, &admin);
        let mut cancelled = Vec::new(&env);
        for id in ids.iter() {
            let settlement_opt: Option<Settlement> =
                env.storage().persistent().get(&DataKey::Settlement(id));
            if let Some(mut settlement) = settlement_opt {
                if settlement.status == SettlementStatus::Pending {
                    settlement.status = SettlementStatus::Cancelled;
                    write_settlement(&env, id, &settlement);
                    env.events()
                        .publish((Symbol::new(&env, "settlement_cancelled"), id), settlement);
                    cancelled.push_back(id);
                }
                // non-pending settlements are skipped
            }
            // missing settlement IDs are skipped
        }
        Ok(cancelled)
    }

    /// **Emergency admin escape hatch (see #457).** Force-cancels a single, specifically
    /// identified settlement that is stuck in `Pending` or `OnHold` and cannot be resolved
    /// through the normal `cancel_settlement`/dispute-resolution paths — for example because
    /// the signer weight required to reach either the settlement threshold or the dispute
    /// resolution threshold has become permanently unreachable. This is deliberately narrow:
    /// it force-cancels one settlement by ID, it does not touch signer weights, thresholds, or
    /// any other settlement, and it is not a general-purpose bypass of the multisig process.
    ///
    /// This entrypoint bypasses the normal signer-quorum requirement by design, since the
    /// quorum being unavailable is exactly the failure mode it exists to recover from. It
    /// should only ever be invoked as a last resort once normal recovery paths are confirmed
    /// unavailable, and every call is independently auditable via the `settlement_force_cancelled`
    /// event (distinct from `settlement_cancelled`), which records the admin who invoked it.
    /// If/when a timelock mechanism lands for admin actions, this entrypoint should be gated
    /// behind it.
    ///
    /// Panics: `SettlementNotFound`, `ForceCancelNotAllowed` (settlement is already
    /// `Executed`, `PartiallyExecuted`, `PartiallySettled`, `Cancelled`, or `Expired`).
    /// Emits: `settlement_force_cancelled`.
    pub fn force_cancel_settlement(env: Env, admin: Address, settlement_id: u64) {
        require_admin(&env, &admin);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| {
                soroban_sdk::panic_with_error!(env, TreasuryError::SettlementNotFound)
            });
        if settlement.status != SettlementStatus::Pending
            && settlement.status != SettlementStatus::OnHold
        {
            soroban_sdk::panic_with_error!(env, TreasuryError::ForceCancelNotAllowed);
        }
        settlement.status = SettlementStatus::Cancelled;
        write_settlement(&env, settlement_id, &settlement);
        env.events().publish(
            (
                Symbol::new(&env, "settlement_force_cancelled"),
                settlement_id,
            ),
            (admin, settlement),
        );
    }

    /// Returns every currently-`Pending` settlement (#572: reads
    /// `DataKey::PendingSettlementIndex` — kept in sync by `write_settlement` on every
    /// settlement write — so cost is proportional to the pending set, not to total
    /// settlement history).
    pub fn get_pending_settlements(env: Env) -> Vec<Settlement> {
        let index: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::PendingSettlementIndex)
            .unwrap_or_else(|| Vec::new(&env));
        let mut pending = Vec::new(&env);
        for id in index.iter() {
            if let Some(settlement) = env
                .storage()
                .persistent()
                .get::<DataKey, Settlement>(&DataKey::Settlement(id))
            {
                pending.push_back(settlement);
            }
        }
        pending
    }

    /// Returns a page of pending settlements: skips the first `start` entries and returns up to `limit`.
    /// (#572: iterates the pending index rather than the full settlement history.)
    pub fn get_pending_settlements_page(env: Env, start: u64, limit: u64) -> Vec<Settlement> {
        let index: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::PendingSettlementIndex)
            .unwrap_or_else(|| Vec::new(&env));
        let mut page = Vec::new(&env);
        let mut skipped: u64 = 0;
        for id in index.iter() {
            if skipped < start {
                skipped += 1;
                continue;
            }
            if (page.len() as u64) >= limit {
                break;
            }
            if let Some(settlement) = env
                .storage()
                .persistent()
                .get::<DataKey, Settlement>(&DataKey::Settlement(id))
            {
                page.push_back(settlement);
            }
        }
        page
    }

    /// Returns aggregate metrics over all pending settlements broken down per token.
    /// Each entry in the returned `Vec` is `(token, count, total_value)` where `token`
    /// is the `MaybeAddress` captured at proposal time. Settlements proposed without a
    /// specific token are grouped under `MaybeAddress::None`. This replaces the previous
    /// single-bucket `(count, total_value)` return, which was misleading when the treasury
    /// holds multiple assets (see #585).
    pub fn get_pending_metrics(env: Env) -> Vec<(MaybeAddress, u64, i128)> {
        // #572: iterate the pending index rather than every settlement ever proposed.
        let index: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::PendingSettlementIndex)
            .unwrap_or_else(|| Vec::new(&env));
        // We build a flat accumulator list of (token, count, total) tuples.
        // Soroban does not provide a Map type in storage helpers, so we do a linear
        // scan over the accumulator on every new token — acceptable because the
        // number of distinct tokens is bounded by MAX_ALLOWED_TOKENS (20).
        let mut buckets: Vec<(MaybeAddress, u64, i128)> = Vec::new(&env);
        for id in index.iter() {
            let Some(settlement) = env
                .storage()
                .persistent()
                .get::<DataKey, Settlement>(&DataKey::Settlement(id))
            else {
                continue;
            };
            let tok = settlement.token.clone();
            let existing = buckets.iter().position(|(b_tok, _, _)| b_tok == tok);
            match existing {
                Some(i) => {
                    let i = i as u32;
                    let (b_tok, b_cnt, b_val) = buckets.get_unchecked(i);
                    buckets.set(i, (b_tok, b_cnt + 1, b_val + settlement.amount));
                }
                None => buckets.push_back((tok, 1u64, settlement.amount)),
            }
        }
        buckets
    }

    /// Returns the settlement with the given `settlement_id`.
    /// Errors: `SettlementNotFound`.
    pub fn get_settlement(env: Env, settlement_id: u64) -> Result<Settlement, TreasuryError> {
        env.storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .unwrap_or_else(|| {
                soroban_sdk::panic_with_error!(env, TreasuryError::SettlementNotFound)
            })
    }

    /// Expires a pending settlement once `SETTLEMENT_TTL` has elapsed since it was proposed.
    /// Confirmed semantics (see #34): this is genuinely time-based — the TTL check below is
    /// mandatory regardless of caller — and admin-gated for the call itself, mirroring the
    /// invoice contract's `batch_expire` precedent rather than being open to any caller.
    /// Panics: `Unauthorized`.
    /// Errors: `SettlementNotFound`, `AlreadyExecuted`, `TtlNotElapsed`.
    /// Emits: `settlement_expired`.
    pub fn expire_settlement(
        env: Env,
        admin: Address,
        settlement_id: u64,
    ) -> Result<(), TreasuryError> {
        require_admin(&env, &admin);
        let mut settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;
        if settlement.status != SettlementStatus::Pending {
            return Err(TreasuryError::AlreadyExecuted);
        }
        let expiry_secs: u64 = env
            .storage()
            .instance()
            .get(&DataKey::SettlementExpirySecs)
            .unwrap_or(7u64 * 24 * 60 * 60);
        if env.ledger().timestamp() <= settlement.proposed_at + expiry_secs {
            return Err(TreasuryError::TtlNotElapsed);
        }
        settlement.status = SettlementStatus::Expired;
        write_settlement(&env, settlement_id, &settlement);
        env.events().publish(
            (Symbol::new(&env, "settlement_expired"), settlement_id),
            settlement,
        );
        Ok(())
    }

    /// Sets or updates the payout address for `merchant` (merchant-only, not paused).
    /// Appends `(new_payout_address, current_timestamp)` to a bounded history stored
    /// under `DataKey::PayoutAddressHistory(merchant)`, capped at `MAX_PAYOUT_HISTORY`
    /// entries. The oldest entry is dropped when the cap is reached (#579).
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
        // Append the change to the bounded payout history for audit (#579).
        let history_key = DataKey::PayoutAddressHistory(merchant.clone());
        let mut history: Vec<(Address, u64)> = env
            .storage()
            .persistent()
            .get(&history_key)
            .unwrap_or_else(|| Vec::new(&env));
        // Drop the oldest entry once the cap is reached.
        if history.len() >= MAX_PAYOUT_HISTORY {
            let mut trimmed = Vec::new(&env);
            let start = history.len() - MAX_PAYOUT_HISTORY + 1;
            for i in start..history.len() {
                trimmed.push_back(history.get(i).unwrap());
            }
            history = trimmed;
        }
        history.push_back((new_payout_address.clone(), env.ledger().timestamp()));
        env.storage().persistent().set(&history_key, &history);
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

    /// Returns the bounded payout-address change history for `merchant` as a list of
    /// `(address, timestamp)` pairs ordered oldest-first. Returns an empty list if no
    /// history has been recorded yet. Capped at `MAX_PAYOUT_HISTORY` entries (#579).
    pub fn get_payout_address_history(env: Env, merchant: Address) -> Vec<(Address, u64)> {
        env.storage()
            .persistent()
            .get(&DataKey::PayoutAddressHistory(merchant))
            .unwrap_or_else(|| Vec::new(&env))
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
            if allowlist.len() >= MAX_ALLOWED_TOKENS {
                soroban_sdk::panic_with_error!(env, TreasuryError::AllowlistFull);
            }
            allowlist.push_back(token.clone());
            env.storage()
                .instance()
                .set(&DataKey::TokenAllowlist, &allowlist);
            env.events()
                .publish((Symbol::new(&env, "token_allowed"),), token);
        }
    }

    /// Removes `token` from the settlement token allowlist (admin-only).
    /// A settlement does not record its token (it is supplied to `execute_settlement`), so
    /// any `Pending` settlement is treated as potentially depending on every allowlisted
    /// token: removal of an allowlisted token is refused until none remain pending.
    /// Removing a token that is not on the allowlist is not blocked.
    /// Panics: `Unauthorized`, `TokenHasPendingSettlements`.
    /// Emits: `token_removed`.
    pub fn remove_allowed_token(env: Env, admin: Address, token: Address) {
        require_admin(&env, &admin);
        let allowlist: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::TokenAllowlist)
            .unwrap_or_else(|| Vec::new(&env));
        if allowlist.contains(&token) && has_pending_settlement(&env) {
            soroban_sdk::panic_with_error!(env, TreasuryError::TokenHasPendingSettlements);
        }
        // #742: refuse to remove a token while depositors still hold balances in it,
        // which would hide those balances from `get_all_balances`.
        let token_total: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TokenTotal(token.clone()))
            .unwrap_or(0);
        if allowlist.contains(&token) && token_total > 0 {
            soroban_sdk::panic_with_error!(env, TreasuryError::TokenHasBalance);
        }
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

    /// Returns a page of settlement IDs proposed for `merchant`: skips the first `start` entries
    /// of the merchant's settlement index and returns up to `limit`. The index itself is capped
    /// at `MAX_MERCHANT_SETTLEMENTS` entries (see #727), oldest dropped first.
    pub fn get_settlements_by_merchant(
        env: Env,
        merchant: Address,
        start: u32,
        limit: u32,
    ) -> Vec<Settlement> {
        let ids: Vec<u64> = env
            .storage()
            .instance()
            .get(&DataKey::MerchantSettlements(merchant))
            .unwrap_or_else(|| Vec::new(&env));
        let mut page = Vec::new(&env);
        let mut skipped: u32 = 0;
        for id in ids.iter() {
            if skipped < start {
                skipped += 1;
                continue;
            }
            if (page.len() as u32) >= limit {
                break;
            }
            if let Some(settlement) = env
                .storage()
                .persistent()
                .get::<DataKey, Settlement>(&DataKey::Settlement(id))
            {
                page.push_back(settlement);
            }
        }
        page
    }

    /// Returns the signers who have approved `settlement_id` and the summed approval weight,
    /// excluding approvals no longer active (see `approval_is_active`).
    /// Errors: `SettlementNotFound`.
    pub fn get_approvals(
        env: Env,
        settlement_id: u64,
    ) -> Result<(Vec<Address>, u32), TreasuryError> {
        let settlement: Settlement = env
            .storage()
            .persistent()
            .get(&DataKey::Settlement(settlement_id))
            .ok_or(TreasuryError::SettlementNotFound)?;
        if !approval_is_active(&env, &settlement) {
            return Ok((Vec::new(&env), 0));
        }
        let mut signers = Vec::new(&env);
        let mut weight = 0u32;
        for signer in settlement.approvals.iter() {
            weight += multisig::signer_weight(&env, &signer);
            signers.push_back(signer);
        }
        Ok((signers, weight))
    }
}

/// Whether a settlement's approvals should still be considered active, i.e. the settlement is
/// still `Pending` and hasn't passed the settlement expiry window since it was proposed. Mirrors the
/// `expire_settlement` TTL check so approvals on an effectively-expired settlement read as
/// stale even before `expire_settlement` is actually called (see #728).
fn approval_is_active(env: &Env, settlement: &Settlement) -> bool {
    let expiry_secs: u64 = env
        .storage()
        .instance()
        .get(&DataKey::SettlementExpirySecs)
        .unwrap_or(7u64 * 24 * 60 * 60);
    settlement.status == SettlementStatus::Pending
        && env.ledger().timestamp() <= settlement.proposed_at.saturating_add(expiry_secs)
}

/// Maximum number of settlement IDs kept in a merchant's settlement index (#727).
pub(crate) const MAX_MERCHANT_SETTLEMENTS: u32 = 50;

/// Appends `settlement_id` to `merchant`'s bounded settlement index, dropping the oldest entry
/// first if the index is already at `MAX_MERCHANT_SETTLEMENTS` (see #727).
fn push_merchant_settlement(env: &Env, merchant: &Address, settlement_id: u64) {
    let mut ids: Vec<u64> = env
        .storage()
        .instance()
        .get(&DataKey::MerchantSettlements(merchant.clone()))
        .unwrap_or_else(|| Vec::new(env));
    if ids.len() >= MAX_MERCHANT_SETTLEMENTS {
        ids.remove(0);
    }
    ids.push_back(settlement_id);
    env.storage()
        .instance()
        .set(&DataKey::MerchantSettlements(merchant.clone()), &ids);
}
