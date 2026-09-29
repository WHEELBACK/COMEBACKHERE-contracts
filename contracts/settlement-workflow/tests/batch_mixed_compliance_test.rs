//! Tests for #615: `execute_with_compliance_batch` when a batch contains a mix of
//! outcomes.
//!
//! Two distinct mixes matter for a batch payout, and both are pinned here:
//!
//! 1. **A blocked recipient.** The batch takes one `merchant` and runs the shared
//!    compliance gate once, so a blocked recipient is *all-or-nothing*: the whole
//!    batch is rejected before any settlement is attempted. The risk this guards
//!    against is a future change that "helpfully" skips the blocked item and pays
//!    the rest — the behaviour must stay a hard rejection, and the unsettled
//!    settlements must remain retryable.
//! 2. **A mix of settleable and unsettleable settlements** for an allowed
//!    recipient. Those are skipped individually (treasury's batch precedent, #38),
//!    so the batch is partial — funds move only for the items that actually
//!    executed, and the per-item plus summary events describe exactly that.
use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    token, Address, Env, FromVal, Symbol, TryFromVal, Vec,
};
use treasury::{
    SettlementHoldReason, SettlementStatus, TreasuryContract, TreasuryContractClient, TreasuryError,
};

/// Settlement ID that was never proposed.
const NON_EXISTENT_ID: u64 = 9_999;

/// Events published by the workflow contract itself, as `(name, settlement_id)`
/// pairs in emission order, drained after each top-level call.
fn drain_events(env: &Env, workflow_id: &Address) -> std::vec::Vec<(String, Option<u64>)> {
    env.events()
        .all()
        .iter()
        .filter(|(contract_id, _, _)| contract_id == workflow_id)
        .map(|(_, topics, _)| {
            let name = Symbol::from_val(env, &topics.get_unchecked(0)).to_string();
            let id = if topics.len() > 1 {
                Some(u64::from_val(env, &topics.get_unchecked(1)))
            } else {
                None
            };
            (name, id)
        })
        .collect()
}

/// Reads the `u32` at `index` of a tuple event payload.
fn payload_u32(env: &Env, data: &soroban_sdk::Val, index: u32) -> u32 {
    let payload = Vec::<soroban_sdk::Val>::try_from_val(env, data).expect("tuple payload");
    u32::from_val(env, &payload.get(index).expect("payload element"))
}

struct Fixture {
    env: Env,
    admin: Address,
    merchant: Address,
    blocked: Address,
    compliance: ComplianceContractClient<'static>,
    compliance_id: Address,
    treasury: TreasuryContractClient<'static>,
    treasury_id: Address,
    workflow: SettlementWorkflowContractClient<'static>,
    workflow_id: Address,
    token_id: Address,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let blocked = Address::generate(&env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    let workflow_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    workflow.initialize(&admin, &compliance_id, &treasury_id);
    treasury.set_signer(&admin, &workflow_id, &1);

    let token_id = env.register_stellar_asset_contract(admin.clone());

    Fixture {
        env,
        admin,
        merchant,
        blocked,
        compliance,
        compliance_id,
        treasury,
        treasury_id,
        workflow,
        workflow_id,
        token_id,
    }
}

impl Fixture {
    /// Proposes a settlement for `merchant`, funds the treasury, and returns its ID.
    fn fund(&self, amount: i128) -> u64 {
        let id = self
            .treasury
            .propose_settlement(&self.admin, &self.merchant, &amount);
        token::StellarAssetClient::new(&self.env, &self.token_id).mint(&self.treasury_id, &amount);
        id
    }

    fn ids(&self, ids: &[u64]) -> Vec<u64> {
        let mut v = Vec::new(&self.env);
        for id in ids {
            v.push_back(*id);
        }
        v
    }

    fn balance(&self, of: &Address) -> i128 {
        token::Client::new(&self.env, &self.token_id).balance(of)
    }

    fn status(&self, settlement_id: u64) -> SettlementStatus {
        self.treasury.get_settlement(&settlement_id).status
    }
}

// ─── A blocked recipient rejects the whole batch ──────────────────────────────

/// A merchant that was never allowed is rejected for the entire batch: not one
/// settlement executes and no funds move.
#[test]
fn unallowed_merchant_rejects_whole_batch_without_moving_funds() {
    let f = setup();
    let s1 = f.fund(2_000_000);
    let s2 = f.fund(3_000_000);
    let s3 = f.fund(4_000_000);

    let err = f
        .workflow
        .try_execute_with_compliance_batch(&f.ids(&[s1, s2, s3]), &f.token_id, &f.merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());

    assert_eq!(
        f.balance(&f.merchant),
        0,
        "no funds may move for a blocked recipient"
    );
    // Every settlement is untouched, so the batch stays retryable.
    for id in [s1, s2, s3] {
        assert_eq!(f.status(id), SettlementStatus::Pending);
    }
    assert_eq!(
        drain_events(&f.env, &f.workflow_id),
        std::vec::Vec::new(),
        "a rejected batch must not publish workflow events"
    );
}

/// Explicitly blocking a previously-allowed merchant is enough to reject the batch:
/// the block is evaluated at call time, not at proposal time.
#[test]
fn explicitly_blocked_merchant_rejects_whole_batch() {
    let f = setup();
    f.compliance.allow_address(&f.admin, &f.merchant);
    let s1 = f.fund(2_000_000);
    let s2 = f.fund(3_000_000);

    f.compliance.block_address(&f.admin, &f.merchant, &None);

    let err = f
        .workflow
        .try_execute_with_compliance_batch(&f.ids(&[s1, s2]), &f.token_id, &f.merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());

    assert_eq!(f.balance(&f.merchant), 0);
    assert_eq!(f.status(s1), SettlementStatus::Pending);
    assert_eq!(f.status(s2), SettlementStatus::Pending);
}

/// The compliance gate runs before the loop, so a blocked recipient rejects even
/// an empty batch rather than returning an empty success.
#[test]
fn blocked_merchant_rejects_even_an_empty_batch() {
    let f = setup();

    let err = f
        .workflow
        .try_execute_with_compliance_batch(&f.ids(&[]), &f.token_id, &f.merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());
}

/// Rejecting a batch must not disturb a settlement that was already paid through
/// the same workflow — a blocked merchant cannot re-trigger or claw back a payment.
#[test]
fn blocked_merchant_cannot_disturb_an_already_executed_settlement() {
    let f = setup();
    f.compliance.allow_address(&f.admin, &f.merchant);
    let already_paid = f.fund(2_000_000);
    let pending = f.fund(5_000_000);

    f.workflow
        .execute_with_compliance(&already_paid, &f.token_id, &f.merchant);
    assert_eq!(f.balance(&f.merchant), 2_000_000);

    f.compliance.block_address(&f.admin, &f.merchant, &None);

    // Batch mixes the already-executed settlement with a fresh one.
    let err = f
        .workflow
        .try_execute_with_compliance_batch(
            &f.ids(&[already_paid, pending]),
            &f.token_id,
            &f.merchant,
        )
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());

    // The earlier payment stands and the fresh settlement was not touched.
    assert_eq!(f.balance(&f.merchant), 2_000_000);
    assert_eq!(f.status(already_paid), SettlementStatus::Executed);
    assert_eq!(f.status(pending), SettlementStatus::Pending);
}

/// A rejected batch leaves nothing behind: once the merchant passes the gate
/// again, the very same batch executes.
#[test]
fn batch_executes_after_the_merchant_passes_the_gate_again() {
    let f = setup();
    let s1 = f.fund(2_000_000);
    let s2 = f.fund(3_000_000);
    let batch = f.ids(&[s1, s2]);

    // First attempt: merchant not on the allowlist.
    assert!(f
        .workflow
        .try_execute_with_compliance_batch(&batch, &f.token_id, &f.merchant)
        .is_err());
    assert_eq!(f.balance(&f.merchant), 0);

    f.compliance.allow_address(&f.admin, &f.merchant);

    let executed = f
        .workflow
        .execute_with_compliance_batch(&batch, &f.token_id, &f.merchant);
    assert_eq!(executed, Vec::from_array(&f.env, [s1, s2]));
    assert_eq!(f.balance(&f.merchant), 5_000_000);
}

// ─── Mixed settleable / unsettleable settlements for an allowed merchant ──────

/// Items that cannot be executed (non-existent, on hold) are skipped individually;
/// the rest settle and the blocked-equivalent items receive nothing.
#[test]
fn batch_skips_unsettleable_items_and_settles_the_rest() {
    let f = setup();
    f.compliance.allow_address(&f.admin, &f.merchant);

    let first = f.fund(1_000_000);
    let held = f.fund(2_000_000);
    let second = f.fund(3_000_000);
    f.treasury
        .hold_settlement(&f.admin, &held, &SettlementHoldReason::ComplianceReview);

    let executed = f.workflow.execute_with_compliance_batch(
        &f.ids(&[first, NON_EXISTENT_ID, held, second]),
        &f.token_id,
        &f.merchant,
    );

    assert_eq!(executed, Vec::from_array(&f.env, [first, second]));
    // Only the two executable settlements moved funds.
    assert_eq!(f.balance(&f.merchant), 4_000_000);
    assert_eq!(f.status(first), SettlementStatus::Executed);
    assert_eq!(f.status(second), SettlementStatus::Executed);
    // The held settlement is untouched and still releasable.
    assert_eq!(f.status(held), SettlementStatus::OnHold);
}

/// The events describe the per-item outcomes: one event per executed settlement
/// (carrying its own ID) plus a single summary whose `(requested, executed)`
/// mismatch is what reveals the skipped items.
#[test]
fn batch_events_match_the_per_item_outcomes() {
    let f = setup();
    f.compliance.allow_address(&f.admin, &f.merchant);

    let first = f.fund(1_000_000);
    let held = f.fund(2_000_000);
    let second = f.fund(3_000_000);
    f.treasury
        .hold_settlement(&f.admin, &held, &SettlementHoldReason::ComplianceReview);

    f.workflow.execute_with_compliance_batch(
        &f.ids(&[first, NON_EXISTENT_ID, held, second]),
        &f.token_id,
        &f.merchant,
    );

    let events = drain_events(&f.env, &f.workflow_id);
    let names: std::vec::Vec<&str> = events.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        std::vec![
            "settlement_workflow_executed",
            "settlement_workflow_executed",
            "workflow_batch_completed",
        ],
        "no event may be published for a skipped settlement"
    );
    assert_eq!(events[0].1, Some(first));
    assert_eq!(events[1].1, Some(second));

    let (_, _, summary) = f
        .env
        .events()
        .all()
        .iter()
        .find(|(_, topics, _)| {
            Symbol::from_val(&f.env, &topics.get_unchecked(0)).to_string()
                == "workflow_batch_completed"
        })
        .expect("batch summary event");
    assert_eq!(
        payload_u32(&f.env, &summary, 0),
        4,
        "requested settlement count"
    );
    assert_eq!(
        payload_u32(&f.env, &summary, 1),
        2,
        "executed settlement count"
    );
}

/// A held settlement skipped by one batch is picked up by a later batch once the
/// hold is released — the skip is not a one-way door.
#[test]
fn previously_skipped_settlement_executes_in_a_later_batch() {
    let f = setup();
    f.compliance.allow_address(&f.admin, &f.merchant);

    let first = f.fund(1_000_000);
    let held = f.fund(2_000_000);
    f.treasury
        .hold_settlement(&f.admin, &held, &SettlementHoldReason::ComplianceReview);

    let executed =
        f.workflow
            .execute_with_compliance_batch(&f.ids(&[first, held]), &f.token_id, &f.merchant);
    assert_eq!(executed, Vec::from_array(&f.env, [first]));
    assert_eq!(f.balance(&f.merchant), 1_000_000);

    f.treasury.release_hold(&f.admin, &held);

    let executed =
        f.workflow
            .execute_with_compliance_batch(&f.ids(&[held]), &f.token_id, &f.merchant);
    assert_eq!(executed, Vec::from_array(&f.env, [held]));
    assert_eq!(f.balance(&f.merchant), 3_000_000);
}

/// Re-running a completed batch executes nothing and pays nothing twice; the
/// summary still reports the requested count so the mismatch is visible.
#[test]
fn replayed_batch_executes_nothing_and_pays_nothing_twice() {
    let f = setup();
    f.compliance.allow_address(&f.admin, &f.merchant);

    let s1 = f.fund(1_000_000);
    let s2 = f.fund(2_000_000);
    let batch = f.ids(&[s1, s2]);

    f.workflow
        .execute_with_compliance_batch(&batch, &f.token_id, &f.merchant);
    assert_eq!(f.balance(&f.merchant), 3_000_000);
    drain_events(&f.env, &f.workflow_id);

    let executed = f
        .workflow
        .execute_with_compliance_batch(&batch, &f.token_id, &f.merchant);
    assert_eq!(executed, Vec::from_array(&f.env, []));

    let (_, _, summary) = f
        .env
        .events()
        .all()
        .iter()
        .find(|(_, topics, _)| {
            Symbol::from_val(&f.env, &topics.get_unchecked(0)).to_string()
                == "workflow_batch_completed"
        })
        .expect("batch summary event");
    assert_eq!(payload_u32(&f.env, &summary, 0), 2);
    assert_eq!(payload_u32(&f.env, &summary, 1), 0);

    // No per-item events, and no double payment.
    let replayed = drain_events(&f.env, &f.workflow_id);
    let names: std::vec::Vec<&str> = replayed.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, std::vec!["workflow_batch_completed"]);
    assert_eq!(f.balance(&f.merchant), 3_000_000);
}

/// Two merchants, one allowed and one blocked: the allowed merchant's batch is
/// unaffected by the other merchant's compliance state — the gate is per recipient,
/// not global.
#[test]
fn allowed_merchant_is_unaffected_by_another_blocked_merchant() {
    let f = setup();
    f.compliance.allow_address(&f.admin, &f.merchant);
    f.compliance.allow_address(&f.admin, &f.blocked);
    f.compliance.block_address(&f.admin, &f.blocked, &None);

    let allowed_id = f.fund(1_500_000);
    // A settlement for the blocked merchant, funded from the same treasury.
    let blocked_id = f
        .treasury
        .propose_settlement(&f.admin, &f.blocked, &2_500_000);

    // Allowed recipient: pays out.
    let executed =
        f.workflow
            .execute_with_compliance_batch(&f.ids(&[allowed_id]), &f.token_id, &f.merchant);
    assert_eq!(executed, Vec::from_array(&f.env, [allowed_id]));
    assert_eq!(f.balance(&f.merchant), 1_500_000);

    // Blocked recipient: rejected, receives nothing.
    let err = f
        .workflow
        .try_execute_with_compliance_batch(&f.ids(&[blocked_id]), &f.token_id, &f.blocked)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());
    assert_eq!(f.balance(&f.blocked), 0);
    assert_eq!(f.status(blocked_id), SettlementStatus::Pending);
}
