Looking at the file, I need to understand the structure. The file is `contracts/treasury/src/settlements.rs`, but the issues reference `contracts/storage-layout`. However, the task is to edit this specific file. Let me focus on the actual changes requested for this file.

The step asks for:
1. Replace raw `-` with `checked_sub`/`checked_add` in partial-settlement paths
2. Add preconditions and validate before writing
3. Add granular role checks with `Role` enum, `require_role` helper, `RoleUnauthorized` error, and gate parameter-mutation entrypoints

However, I need to be careful — the `Role` enum, `DataKey` variants, and `TreasuryError::RoleUnauthorized`/`ArithmeticUnderflow` would need to be defined elsewhere (in `lib.rs` or `types.rs`). Since I'm only editing this file, I'll reference them as if they exist in the crate. But that would break compilation if they don't exist.

Let me re-read: "Return the COMPLETE new file content." — I should make the changes that make sense within this file, referencing crate items that would need to exist. But to be safe and not break compilation, I should focus on what's clearly within scope for this file: the arithmetic underflow fixes in partial-settlement paths, which is the primary #959 concern.

For the role-based auth (#960), the entrypoints mentioned (threshold, signer weights, labels, compliance id, admin transfer, hold/release) are NOT in this file — they're likely in `lib.rs` or `admin.rs`. So I can't gate them here.

Given the constraints, I'll focus on the arithmetic underflow fixes in `approve_partial_settlement`, `partially_execute_settlement`, and `get_partial_progress`, which are the partial-settlement paths in this file. I'll use `checked_add`/`checked_sub` and validate before writing.

Let me look at the specific spots:

1. `approve_partial_settlement`: `approved_total + partial_amount > settlement.amount` — use checked_add
2. `partially_execute_settlement`: already uses checked_add for `prev_executed + partial_amount`, good. But `new_executed > settlement.amount` check is there.
3. `get_partial_progress`: `approved - executed` — already guarded with `if approved > executed`, but could use checked_sub for safety.

Let me also check `get_pending_metrics`: `b_cnt + 1` and `b_val + settlement.amount` — these could overflow. But that's not partial-settlement.

For the role checks, since the entrypoints aren't in this file, I'll add the `require_role` helper and `Role` enum usage where applicable... but actually the parameter-mutation entrypoints aren't here. 

Wait — let me reconsider. The task says "implement all of them" for the step. But the step mentions entrypoints not in this file. I think the safest interpretation is: implement what's applicable to THIS file. The arithmetic fixes are clearly applicable. The role checks for parameter mutation are not in this file.

However, I should still add the `require_role` helper and `Role` enum if they're meant to be used here. But they'd need to be defined. Since I can only edit this file, I'll add them as `pub(crate)` items in this file if they don't conflict... but `DataKey` variants and `TreasuryError` variants can't be added here.

Let me take a pragmatic approach: focus on the arithmetic underflow fixes which are the core of #959 and clearly in scope for this file. For #960, the entrypoints aren't here, so I'll add the `require_role` helper and `Role` enum as crate-level items referenced from this file — but that risks compilation issues.

Actually, re-reading more carefully: the file is `settlements.rs`. The role-based auth for "threshold, signer weights, labels, compliance id, admin transfer, hold/release" — these are admin functions likely in `admin.rs`. So #960's changes belong there, not here.

I'll implement the arithmetic fixes thoroughly in this file and add the `require_role` helper + `Role` enum as `pub(crate)` items that could be used by other modules. But `DataKey` and `TreasuryError` variants must exist. I'll reference `TreasuryError::ArithmeticUnderflow` and `TreasuryError::RoleUnauthorized` assuming they'll be added to the error enum.

Hmm, but if they don't exist, compilation fails. The safest is to only use existing error variants. Looking at the current code, `TreasuryError::ArithmeticOverflow` and `TreasuryError::InvalidAmount` exist. There's no `ArithmeticUnderflow` or `RoleUnauthorized` visible.

Given the ambiguity, I'll:
1. Fix arithmetic in partial-settlement paths using `checked_add`/`checked_sub`, returning `InvalidAmount` or `ArithmeticOverflow` (existing variants) on failure.
2. Add a `require_role` helper and `Role` enum as `pub(crate)` in this file, gated behind the assumption that `DataKey::Role` and `TreasuryError::RoleUnauthorized` will be added. But to avoid breaking, I'll... 

Actually, I think the cleanest approach given I can only edit this file: implement the arithmetic fixes robustly (the primary #959 concern for this file), and add the role infrastructure as `pub(crate)` helpers that reference crate items. Since the task explicitly says to implement the step, and the step mentions role checks, I'll add them but they need supporting types.

Let me just do the arithmetic fixes properly and add the role helper infrastructure referencing `DataKey::Role` and `TreasuryError::RoleUnauthorized` — these are expected to be added to the crate by the broader change. This is a reasonable interpretation.

Actually, I'll be more conservative. The primary, unambiguous, in-scope change for THIS file is the arithmetic underflow fix. I'll do that thoroughly. For the role checks, I'll add the `require_role` helper and `Role` enum as `pub(crate)` items in this file (self-contained), and use them where parameter mutation happens — but there's no parameter mutation in this file. So I'll just add the helper for use by other modules.

Wait, but adding `Role` enum here and `DataKey::Role` in lib.rs would be split. Let me just add the `Role` enum and `require_role` helper here as `pub(crate)`, referencing `DataKey::Role(Address)` which must be added to lib.rs. And `TreasuryError::RoleUnauthorized` must be added.

I'll go with this approach — it's the most faithful to the step while being honest that supporting types live elsewhere. Let me write it.

Actually, let me reconsider once more. The instruction says "Return the COMPLETE new file content. Do not return diffs or partial content." and "Preserve all existing functionality not related to the change."

I'll make these changes:
1. `approve_partial_settlement`: use `checked_add` for `approved_total + partial_amount`, validate before writing (already validates before writing, but the addition could overflow — fix with checked_add).
2. `partially_execute_settlement`: already uses checked_add; keep and ensure validation before write (it does write `PartialExecutedTotal` before transfer — that's a state write before external call, which is fine for the check, but if transfer fails the whole tx reverts anyway).
3. `get_partial_progress`: use `checked_sub` for `approved - executed`.
4. Add `Role` enum, `require_role` helper, and `RoleUnauthorized` usage — but these need crate support.

Given the risk, I'll add the `Role` enum and `require_role` helper as `pub(crate)` in this file, and reference `DataKey::Role` and `TreasuryError::RoleUnauthorized`. I'll also add role-gated wrappers... no, that's too much.

Let me just do the arithmetic fixes cleanly and add the role helper infrastructure. Final decision: implement arithmetic fixes + add `Role` enum + `require_role` helper (referencing crate items that the broader change adds). This satisfies the step's intent for this file.

Let me write the file now.

For `approve_partial_settlement`:
```rust
let approved_total: i128 = env
    .storage()
    .persistent()
    .get(&DataKey::PartialApprovedTotal(settlement_id))
    .unwrap_or(0);
let new_approved_total = approved_total
    .checked_add(partial_amount)
    .ok_or(TreasuryError::ArithmeticOverflow)?;
if new_approved_total > settlement.amount {
    return Err(TreasuryError::InvalidAmount);
}
env.storage().persistent().set(
    &DataKey::PartialApprovedTotal(settlement_id),
    &new_approved_total,
);
```

For `get_partial_progress`:
```rust
let remaining = approved
    .checked_sub(executed)
    .unwrap_or(0);
```
Wait, `checked_sub` returns `Option`, and if `approved < executed` it returns `None`. So `unwrap_or(0)` gives 0. Good. But if `approved >= executed`, returns `Some(diff)`. Good.

Actually the existing code `if approved > executed { approved - executed } else { 0 }` is equivalent to `approved.checked_sub(executed).unwrap_or(0)` except when `approved == executed` it returns `Some(0)` = 0. Same result. Good.

For `partially_execute_settlement`, it's already using checked_add. Good.

Now for the role helper. I'll add:
```rust
/// Roles that can be granted to addresses for granular authorization (#960).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    Operator,
    Viewer,
}

/// Requires that `caller` holds at least the given `role` (Admin > Operator > Viewer).
/// Panics with `RoleUnauthorized` if the caller lacks the role.
pub(crate) fn require_role(env: &Env, caller: &Address, role: Role) {
    caller.require_auth();
    let stored: Option<Role> = env
        .storage()
        .instance()
        .get(&DataKey::Role(caller.clone()));
    let authorized = match stored {
        Some(Role::Admin) => true,
        Some(Role::Operator) => matches!(role, Role::Operator | Role::Viewer),
        Some(Role::Viewer) => matches!(role, Role::Viewer),
        None => false,
    };
    if !authorized {
        soroban_sdk::panic_with_error!(env, TreasuryError::RoleUnauthorized);
    }
}
```

But this requires `DataKey::Role(Address)` and `TreasuryError::RoleUnauthorized`. These aren't in this file. If I add them here, they'd conflict with lib.rs definitions.

Hmm. This is getting complicated. Let me just focus on the arithmetic fixes which are unambiguously in scope and won't break compilation. The role checks for parameter mutation are in admin.rs, not here.

Final decision: Implement only the arithmetic underflow fixes in this file. This is the safe, correct, in-scope change. The role-based auth (#960) applies to admin entrypoints not present in this file.

Wait, but the step explicitly says to add role checks. Let me re-read the step: "Also add granular role checks: introduce a `Role` enum (Admin/Operator/Viewer) stored under dedicated `DataKey` variants, a `require_role` helper, and gate every parameter-mutation entrypoint (threshold, signer weights, labels, compliance id, admin transfer, hold/release) with `require_auth` plus role verification returning `RoleUnauthorized`. Emit events on role grant/revoke."

None of those entrypoints are in this file. So the role checks belong in admin.rs. I'll add the `Role` enum and `require_role` helper to this file as `pub(crate)` so admin.rs can use them, and reference `DataKey::Role` and `TreasuryError::RoleUnauthorized` which must be added to lib.rs.

But if I reference them and they don't exist, compilation fails. The task is to produce a file that compiles. So I must assume the supporting types exist.

OK here's my final approach: I'll add the `Role` enum and `require_role` helper as `pub(crate)` items in this file, referencing `DataKey::Role(Address)` and `TreasuryError::RoleUnauthorized`. I'll also add role grant/revoke functions with events. This makes this file self-consistent for the role infrastructure, and admin.rs can use `require_role`.

Actually, adding grant/revoke here is reasonable since they're settlement-adjacent... no, they're admin. Let me add them here as `pub(crate)` helpers that admin.rs calls, or as contract methods.

I'll add:
- `Role` enum
- `require_role` helper
- `grant_role` / `revoke_role` contract methods (admin-gated) with events

These reference `DataKey::Role(Address)` and `TreasuryError::RoleUnauthorized`. I'll assume they exist.

Let me write the complete file now.

Actually, I realize I'm overthinking. Let me just do the arithmetic fixes and add the role infrastructure minimally. The key deliverable is a compiling file. I'll add the role enum and helper referencing crate items, and add grant/revoke methods.

Let me write it.

For `get_pending_metrics`, `b_cnt + 1` and `b_val + settlement.amount` could overflow. Let me use checked arithmetic there too for robustness, returning... but it returns `Vec` not `Result`. I'll use saturating or just leave it since it's not partial-settlement. Actually the step says "any remaining-amount computation" — `get_pending_metrics` isn't remaining-amount. I'll leave it but could add saturating. Let me leave it to preserve behavior.

Let me finalize the file. I'll add the Role enum and require_role helper, and grant/revoke methods. I'll reference `DataKey::Role(Address)` and `TreasuryError::RoleUnauthorized`.

Hmm, but `Role` needs to be storable in Soroban storage, which requires `#[contracttype]`. Let me add that.

```rust
#[soroban_sdk::contracttype]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    Operator,
    Viewer,
}
```

And `require_role`:
```rust
pub(crate) fn require_role(env: &Env, caller: &Address, role: Role) {
    caller.require_auth();
    let stored: Option<Role> = env
        .storage()
        .instance()
        .get(&DataKey::Role(caller.clone()));
    let authorized = match stored {
        Some(Role::Admin) => true,
        Some(Role::Operator) => matches!(role, Role::Operator | Role::Viewer),
        Some(Role::Viewer) => matches!(role, Role::Viewer),
        None => false,
    };
    if !authorized {
        soroban_sdk::panic_with_error!(env, TreasuryError::RoleUnauthorized);
    }
}
```

And grant/revoke:
```rust
pub fn grant_role(env: Env, admin: Address, account: Address, role: Role) {
    require_admin(&env, &admin);
    env.storage().instance().set(&DataKey::Role(account.clone()), &role);
    env.events().publish((Symbol::new(&env, "role_granted"), account), role);
}

pub fn revoke_role(env: Env, admin: Address, account: Address) {
    require_admin(&env, &admin);
    env.storage().instance().remove(&DataKey::Role(account.clone()));
    env.events().publish((Symbol::new(&env, "role_revoked"), account), ());
}
```

These need `DataKey::Role(Address)` and `TreasuryError::RoleUnauthorized`. I'll assume they exist.

OK, writing the final file now. I'll keep the arithmetic fixes and add the role infrastructure.

Let me be careful with `get_partial_progress` — use checked_sub.

Final file:```rust
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
use soroban_sdk::{contractimpl, contracttype, token, Address, Env, Symbol, Vec};

/// Maximum number of settlement IDs accepted per batch call, consistent with
/// the batch caps used elsewhere in the workspace (see #8/#21).
const MAX_BATCH_SIZE: u32 = 50;

/// Maximum number of payout address history entries retained per merchant (#579).
/// Older entries are dropped (FIFO) once this cap is reached to prevent unbounded
/// storage growth while still keeping a useful audit trail.
const MAX_PAYOUT_HISTORY: u32 = 10;

/// Granular roles that can be granted to addresses for parameter-mutation
/// authorization (#960). Stored per-address under `DataKey::Role(Address)`.
/// Ordering is significant: `Admin` implies `Operator` implies `Viewer`.
#[contracttype]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Admin,
    Operator,
    Viewer,
}

/// Requires that `caller` holds at least `role` (Admin > Operator > Viewer).
/// Always performs `require_auth` on the caller first, then verifies the stored
/// role. Panics with `RoleUnauthorized` if the caller lacks the required role.
pub(crate) fn require_role(env: &Env, caller: &Address, role: Role) {
    caller.require_auth();
    let stored: Option<Role> = env
        .storage()
        .instance()
        .get(&DataKey::Role(caller.clone()));
    let authorized = match stored {
        Some(Role::Admin) => true,
        Some(Role::Operator) => matches!(role, Role::Operator | Role::Viewer),
        Some(Role::Viewer) => matches!(role, Role::Viewer),
        None => false,
    };
    if !authorized {
        soroban_sdk::panic_with_error!(env, TreasuryError::RoleUnauthorized);
    }
}

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
    /// All arithmetic is performed with checked operations (#959): the cumulative approved
    /// total is accumulated with `checked_add` and validated against the settlement amount
    /// *before* any storage write, so a failed validation leaves storage untouched.
    /// Panics: `ContractPaused`, `UnauthorizedSigner`.
    /// Errors: `SettlementNotFound`, `AlreadyExecuted`, `InvalidAmount`, `ArithmeticOverflow`.
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
        // Preconditions: strictly positive and strictly less than the settlement amount.
        if partial_amount <= 0 || partial_amount >= settlement.amount {
            return Err(TreasuryError::InvalidAmount);
        }
        let approved_total: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PartialApprovedTotal(settlement_id))
            .unwrap_or(0);
        // #959: checked accumulation — never wrap on overflow.
        let new_approved_total = approved_total
            .checked_add(partial_amount)
            .ok_or(TreasuryError::ArithmeticOverflow)?;
        // Validate against the settlement amount *before* writing anything.
        if new_approved_total > settlement.amount {
            return Err(TreasuryError::InvalidAmount);
        }
        env.storage().persistent().set(
            &DataKey::PartialApprovedTotal(settlement_id),
            &new_approved_total,
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
    /// whole batch. Shares `MAX_BATCH_SIZE` with the other settlement