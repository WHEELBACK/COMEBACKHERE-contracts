#[path = "reentrancy_suite/malicious_compliance.rs"]
mod malicious_compliance;

use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    token, Address, Env, FromVal, Symbol, TryFromVal, Vec,
};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

/// Generous CPU-instruction ceiling for the two-hop cross-contract call chain
/// (Compliance::is_allowed → Treasury::execute_settlement). Native/test-host
/// numbers are far lower; this bound is wide enough to avoid flakiness while
/// still catching a large, unintended regression in the composed call chain (#368).
const MAX_EXECUTE_INSTRUCTIONS: u64 = 5_000_000;

fn setup() -> (
    Env,
    Address,
    Address,
    ComplianceContractClient<'static>,
    Address,
    TreasuryContractClient<'static>,
    Address,
    SettlementWorkflowContractClient<'static>,
    Address,
) {
    setup_with_signer(true)
}

/// `register_workflow_signer` controls whether the workflow contract is registered
/// as a Treasury signer. Pass `false` to exercise the #370 precondition path where
/// the workflow's own address has not been registered via `Treasury::set_signer`.
fn setup_with_signer(
    register_workflow_signer: bool,
) -> (
    Env,
    Address,
    Address,
    ComplianceContractClient<'static>,
    Address,
    TreasuryContractClient<'static>,
    Address,
    SettlementWorkflowContractClient<'static>,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    let workflow_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    // Pin the trusted compliance/treasury instances once at init (#364) and record
    // the admin allowed to pause/unpause the workflow (#616).
    workflow.initialize(&admin, &compliance_id, &treasury_id);
    // The workflow contract executes settlements as itself, so it must be an
    // authorized Treasury signer.
    if register_workflow_signer {
        treasury.set_signer(&admin, &workflow_id, &1);
    }

    let token_id = env.register_stellar_asset_contract(admin.clone());

    (
        env,
        admin,
        merchant,
        compliance,
        compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    )
}

#[test]
fn execution_blocked_when_compliance_returns_false() {
    let (
        env,
        admin,
        merchant,
        _compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    let err = workflow
        .try_execute_with_compliance(&settlement_id, &token_id, &merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);
}

#[test]
fn successful_path_executes_treasury_settlement() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    workflow
        .try_execute_with_compliance(&settlement_id, &token_id, &merchant)
        .unwrap()
        .unwrap();

    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        10_000_000
    );
}

#[test]
fn emits_settlement_workflow_executed_event() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    workflow.execute_with_compliance(&settlement_id, &token_id, &merchant);

    let (_, topics, _) = env.events().all().last().unwrap();
    let emitted_symbol = Symbol::from_val(&env, &topics.get_unchecked(0));
    assert_eq!(
        emitted_symbol,
        Symbol::new(&env, "settlement_workflow_executed"),
        "expected a settlement_workflow_executed event to be emitted"
    );
}

#[test]
fn initialize_is_idempotent_and_pins_trusted_instances() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let compliance_id = Address::generate(&env);
    let treasury_id = Address::generate(&env);
    let workflow_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);

    workflow.initialize(&admin, &compliance_id, &treasury_id);
    // Second initialize must trap with AlreadyInitialized.
    let err = workflow
        .try_initialize(&admin, &compliance_id, &treasury_id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::AlreadyInitialized.into());
}

#[test]
fn batch_executes_multiple_settlements_and_skips_invalid_ids() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);

    let good_1 = treasury.propose_settlement(&admin, &merchant, &5_000_000);
    let good_2 = treasury.propose_settlement(&admin, &merchant, &5_000_000);
    // A settlement that does not exist.
    let bogus: u64 = 999;
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(good_1);
    ids.push_back(bogus);
    ids.push_back(good_2);

    let executed = workflow.execute_with_compliance_batch(&ids, &token_id, &merchant);
    assert_eq!(
        executed,
        soroban_sdk::Vec::from_array(&env, [good_1, good_2])
    );
    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        10_000_000
    );
}

#[test]
fn batch_rejected_when_compliance_fails() {
    let (
        env,
        admin,
        merchant,
        _compliance,
        _compliance_id,
        treasury,
        _treasury_id,
        workflow,
        token_id,
    ) = setup();

    let good = treasury.propose_settlement(&admin, &merchant, &5_000_000);
    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(good);

    // merchant is not on the compliance allowlist — the batch must be rejected
    // with ComplianceCheckFailed before any settlement is attempted.
    let err = workflow
        .try_execute_with_compliance_batch(&ids, &token_id, &merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());
}

#[test]
fn execute_with_compliance_stays_under_instruction_budget() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    // Lift budget limits so the call chain is measured, not artificially capped.
    env.cost_estimate().budget().reset_unlimited();
    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);
    env.cost_estimate().budget().reset_tracker();

    workflow.execute_with_compliance(&settlement_id, &token_id, &merchant);

    let instructions = env.cost_estimate().budget().cpu_instruction_cost();
    assert!(
        instructions <= MAX_EXECUTE_INSTRUCTIONS,
        "execute_with_compliance used {instructions} instructions, \
         expected <= {MAX_EXECUTE_INSTRUCTIONS}"
    );
}

#[test]
fn execute_with_compliance_is_idempotent_against_retried_call() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    // First execute call succeeds.
    workflow.execute_with_compliance(&settlement_id, &token_id, &merchant);
    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        10_000_000
    );

    // Retried execute call with identical parameters fails cleanly via Treasury's
    // AlreadyExecuted guard (settlement status is no longer Pending after the first
    // successful call), not by double-paying the merchant.
    let err = workflow
        .try_execute_with_compliance(&settlement_id, &token_id, &merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::AlreadyExecuted.into());

    // Verify balance did not change: only one payment occurred despite two calls.
    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        10_000_000
    );
}

// ─── #616 Pause control matrix ────────────────────────────────────────────────

/// A freshly initialized workflow is not paused, and reports the admin recorded
/// at initialization.
#[test]
fn workflow_is_not_paused_after_initialize() {
    let (
        _env,
        admin,
        _merchant,
        _compliance,
        _compliance_id,
        _treasury,
        _treasury_id,
        workflow,
        _token_id,
    ) = setup();

    assert!(!workflow.is_paused());
    assert_eq!(workflow.get_admin(), admin);
}

/// `is_paused` flips true after `pause` and back to false after `unpause`.
#[test]
fn is_paused_tracks_pause_and_unpause() {
    let (
        _env,
        admin,
        _merchant,
        _compliance,
        _compliance_id,
        _treasury,
        _treasury_id,
        workflow,
        _token_id,
    ) = setup();

    workflow.pause(&admin);
    assert!(workflow.is_paused());

    workflow.unpause(&admin);
    assert!(!workflow.is_paused());
}

/// While paused, `execute_with_compliance` is rejected with `ContractPaused` and
/// no funds move.
#[test]
fn execute_with_compliance_rejected_while_paused() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    workflow.pause(&admin);

    let err = workflow
        .try_execute_with_compliance(&settlement_id, &token_id, &merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ContractPaused.into());
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);
}

/// While paused, `execute_with_compliance_batch` is rejected with `ContractPaused`
/// and no settlement in the batch is executed.
#[test]
fn execute_with_compliance_batch_rejected_while_paused() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    let good_1 = treasury.propose_settlement(&admin, &merchant, &5_000_000);
    let good_2 = treasury.propose_settlement(&admin, &merchant, &5_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(good_1);
    ids.push_back(good_2);

    workflow.pause(&admin);

    let err = workflow
        .try_execute_with_compliance_batch(&ids, &token_id, &merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ContractPaused.into());
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);
}

/// Unpausing restores the execution path: the pending settlement settles normally.
#[test]
fn execution_resumes_after_unpause() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();

    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &10_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &10_000_000);

    workflow.pause(&admin);
    workflow.unpause(&admin);

    workflow
        .try_execute_with_compliance(&settlement_id, &token_id, &merchant)
        .unwrap()
        .unwrap();
    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        10_000_000
    );
}

/// The pause check precedes the compliance gate, so a blocked merchant is reported
/// as paused while the workflow is halted rather than as a compliance failure.
#[test]
fn paused_workflow_reports_paused_before_compliance_check() {
    let (
        _env,
        admin,
        merchant,
        _compliance,
        _compliance_id,
        treasury,
        _treasury_id,
        workflow,
        token_id,
    ) = setup();

    // merchant is not on the allowlist: unpaused this is ComplianceCheckFailed.
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &1_000_000);

    workflow.pause(&admin);

    let err = workflow
        .try_execute_with_compliance(&settlement_id, &token_id, &merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ContractPaused.into());
}

/// Only the recorded admin can pause; a random address is rejected with
/// `Unauthorized`.
#[test]
fn non_admin_cannot_pause_or_unpause() {
    let (
        env,
        _admin,
        _merchant,
        _compliance,
        _compliance_id,
        _treasury,
        _treasury_id,
        workflow,
        _token_id,
    ) = setup();

    let outsider = Address::generate(&env);
    assert_eq!(
        workflow.try_pause(&outsider).unwrap_err().unwrap(),
        TreasuryError::Unauthorized.into()
    );
    assert!(!workflow.is_paused());
    assert_eq!(
        workflow.try_unpause(&outsider).unwrap_err().unwrap(),
        TreasuryError::Unauthorized.into()
    );
}

/// `pause` / `unpause` emit their own events so monitoring can follow the
/// circuit breaker without polling `is_paused`.
#[test]
fn pause_and_unpause_emit_events() {
    let (
        env,
        admin,
        _merchant,
        _compliance,
        _compliance_id,
        _treasury,
        _treasury_id,
        workflow,
        _token_id,
    ) = setup();

    workflow.pause(&admin);
    let (_, topics, data) = env.events().all().last().unwrap();
    assert_eq!(
        Symbol::from_val(&env, &topics.get_unchecked(0)),
        Symbol::new(&env, "settlement_workflow_paused")
    );
    assert_eq!(Address::from_val(&env, &data), admin);

    workflow.unpause(&admin);
    let (_, topics, data) = env.events().all().last().unwrap();
    assert_eq!(
        Symbol::from_val(&env, &topics.get_unchecked(0)),
        Symbol::new(&env, "settlement_workflow_unpaused")
    );
    assert_eq!(Address::from_val(&env, &data), admin);
}

// ─── #614 Workflow event emission ─────────────────────────────────────────────

/// Collects the workflow contract's own events as `(name, settlement_id, merchant,
/// token, amount)` tuples, in emission order.
///
/// `settlement_workflow_executed` carries the settlement ID in `topics[1]` and
/// `(merchant, token_contract, amount)` in the data payload; the per-item and
/// summary events are recorded with `None` for the fields they don't carry, so a
/// test can assert on the whole sequence in one place.
type WorkflowEvent = (
    String,
    Option<u64>,
    Option<Address>,
    Option<Address>,
    Option<i128>,
);

fn workflow_events(env: &Env, workflow_id: &Address) -> std::vec::Vec<WorkflowEvent> {
    /// Returns the `i`th element of a tuple event payload, if present.
    fn payload_at(env: &Env, data: &soroban_sdk::Val, i: u32) -> Option<soroban_sdk::Val> {
        match Vec::<soroban_sdk::Val>::try_from_val(env, data) {
            Ok(payload) => payload.get(i),
            // Single (non-tuple) payloads sit at index 0.
            Err(_) if i == 0 => Some(data.clone()),
            Err(_) => None,
        }
    }

    env.events()
        .all()
        .iter()
        .filter(|(contract_id, _, _)| contract_id == workflow_id)
        .map(|(_, topics, data)| {
            let name = Symbol::from_val(env, &topics.get_unchecked(0)).to_string();
            let settlement_id = if topics.len() > 1 {
                Some(u64::from_val(env, &topics.get_unchecked(1)))
            } else {
                None
            };
            match name.as_str() {
                "settlement_workflow_executed" => (
                    name,
                    settlement_id,
                    payload_at(env, &data, 0).map(|v| Address::from_val(env, &v)),
                    payload_at(env, &data, 1).map(|v| Address::from_val(env, &v)),
                    payload_at(env, &data, 2).map(|v| i128::from_val(env, &v)),
                ),
                // (requested, executed) is folded into a single i128 as
                // `requested + executed * 1_000_000` so the whole summary fits the
                // one-slot payload this helper records for it.
                "workflow_batch_completed" => {
                    let requested = payload_at(env, &data, 0)
                        .map(|v| u32::from_val(env, &v))
                        .unwrap_or(0) as i128;
                    let executed = payload_at(env, &data, 1)
                        .map(|v| u32::from_val(env, &v))
                        .unwrap_or(0) as i128;
                    (
                        name,
                        None,
                        None,
                        None,
                        Some(requested + executed * 1_000_000),
                    )
                }
                _ => (name, settlement_id, None, None, None),
            }
        })
        .collect()
}

/// `execute_with_compliance` publishes one summary event carrying the outcome
/// (settlement ID), the recipient, and the amount that moved (#614).
#[test]
fn execute_with_compliance_event_carries_outcome_recipient_and_amount() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();
    let workflow_id = workflow.address.clone();

    compliance.allow_address(&admin, &merchant);
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &7_500_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &7_500_000);

    workflow.execute_with_compliance(&settlement_id, &token_id, &merchant);

    let events = workflow_events(&env, &workflow_id);
    assert_eq!(
        events.last().unwrap(),
        &(
            "settlement_workflow_executed".to_string(),
            Some(settlement_id),
            Some(merchant.clone()),
            Some(token_id.clone()),
            Some(7_500_000),
        ),
        "expected a settlement_workflow_executed summary event carrying \
         (settlement_id, merchant, token, amount); got: {events:?}"
    );
}

/// The summary event is emitted on the successful path only: a compliance-blocked
/// merchant aborts the whole invocation, so no event is published and the failure is
/// observable from the reverted transaction instead (Soroban discards events from a
/// failed invocation). Pinning this so nobody later "fixes" it by emitting an event
/// that can never be seen.
#[test]
fn blocked_merchant_publishes_no_workflow_event() {
    let (
        env,
        admin,
        merchant,
        _compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();
    let workflow_id = workflow.address.clone();

    // merchant is never allowed.
    let settlement_id = treasury.propose_settlement(&admin, &merchant, &4_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &4_000_000);

    let err = workflow
        .try_execute_with_compliance(&settlement_id, &token_id, &merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());

    assert_eq!(
        workflow_events(&env, &workflow_id),
        std::vec::Vec::<WorkflowEvent>::new(),
        "a rejected call must not publish a settlement_workflow_executed event"
    );
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);
}

/// The batch publishes one `settlement_workflow_executed` per executed settlement —
/// each with its own settlement ID and amount — and exactly one
/// `workflow_batch_completed` summary as the last event (#614).
#[test]
fn batch_emits_per_settlement_events_plus_one_summary() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();
    let workflow_id = workflow.address.clone();

    compliance.allow_address(&admin, &merchant);
    let small = treasury.propose_settlement(&admin, &merchant, &3_000_000);
    let large = treasury.propose_settlement(&admin, &merchant, &9_000_000);
    let bogus: u64 = 4242;
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &12_000_000);

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(small);
    ids.push_back(bogus);
    ids.push_back(large);

    workflow.execute_with_compliance_batch(&ids, &token_id, &merchant);

    let events = workflow_events(&env, &workflow_id);
    let names: std::vec::Vec<&str> = events.iter().map(|(name, ..)| name.as_str()).collect();
    assert_eq!(
        names,
        std::vec![
            "settlement_workflow_executed",
            "settlement_workflow_executed",
            "workflow_batch_completed",
        ],
        "expected one event per executed settlement followed by a single batch summary"
    );

    assert_eq!(events[0].1, Some(small));
    assert_eq!(events[0].4, Some(3_000_000));
    assert_eq!(events[1].1, Some(large));
    assert_eq!(events[1].4, Some(9_000_000));
    // (requested, executed) == (3, 2) is encoded as 3 + 2 * 1_000_000.
    assert_eq!(events[2].4, Some(2_000_003));
}

/// The batch summary is published even when nothing could be executed, so a
/// fully-skipped batch is still visible to monitoring (#614).
#[test]
fn batch_summary_is_emitted_when_every_item_is_skipped() {
    let (
        env,
        admin,
        merchant,
        compliance,
        _compliance_id,
        treasury,
        _treasury_id,
        workflow,
        token_id,
    ) = setup();
    let workflow_id = workflow.address.clone();

    compliance.allow_address(&admin, &merchant);
    // Settlements below the multisig threshold, plus a non-existent one: none execute.
    let low_weight = treasury.propose_settlement(&admin, &merchant, &1_000_000);
    treasury.cancel_settlement(&admin, &low_weight);
    let bogus: u64 = 77;

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(low_weight);
    ids.push_back(bogus);

    let executed = workflow.execute_with_compliance_batch(&ids, &token_id, &merchant);
    assert_eq!(executed, soroban_sdk::Vec::from_array(&env, []));

    let events = workflow_events(&env, &workflow_id);
    assert_eq!(
        events,
        std::vec![(
            "workflow_batch_completed".to_string(),
            None,
            None,
            None,
            // (requested, executed) == (2, 0).
            Some(2),
        )],
        "a fully-skipped batch must still publish its summary event"
    );
}

/// The workflow's emitted event names must match `abis/settlement-workflow.json`
/// exactly (#614).
///
/// Two classes of drift are caught here: an event added/renamed in source without
/// regenerating the snapshot, and an event name that the host would reject at
/// runtime — Soroban symbols are capped at 32 characters, and an over-long name
/// fails the whole invocation with `InvalidInput` rather than at compile time
/// (`settlement_workflow_batch_completed` did exactly that).
#[test]
fn emitted_events_match_abi_event_snapshot() {
    /// The test host resets the event log on every top-level invocation, so each
    /// entrypoint's events have to be drained before the next call.
    fn drain(env: &Env, workflow_id: &Address) -> std::vec::Vec<String> {
        env.events()
            .all()
            .iter()
            .filter(|(contract_id, _, _)| contract_id == workflow_id)
            .map(|(_, topics, _)| Symbol::from_val(env, &topics.get_unchecked(0)).to_string())
            .collect()
    }

    let (
        env,
        admin,
        merchant,
        compliance,
        compliance_id,
        treasury,
        treasury_id,
        workflow,
        token_id,
    ) = setup();
    let workflow_id = workflow.address.clone();

    let mut emitted: std::vec::Vec<String> = std::vec::Vec::new();

    // Exercise every entrypoint that publishes a workflow event.
    compliance.allow_address(&admin, &merchant);
    let single = treasury.propose_settlement(&admin, &merchant, &2_000_000);
    let batched = treasury.propose_settlement(&admin, &merchant, &2_000_000);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &4_000_000);

    workflow.execute_with_compliance(&single, &token_id, &merchant);
    emitted.extend(drain(&env, &workflow_id));

    let mut ids = soroban_sdk::Vec::new(&env);
    ids.push_back(batched);
    workflow.execute_with_compliance_batch(&ids, &token_id, &merchant);
    emitted.extend(drain(&env, &workflow_id));

    workflow.pause(&admin);
    emitted.extend(drain(&env, &workflow_id));
    workflow.unpause(&admin);
    emitted.extend(drain(&env, &workflow_id));

    // `setup()` already initialized `workflow`, so initialize a second instance to
    // cover `workflow_initialized` too.
    let other_id = env.register_contract(None, SettlementWorkflowContract);
    let other = SettlementWorkflowContractClient::new(&env, &other_id);
    other.initialize(&admin, &compliance_id, &other_id);
    emitted.extend(drain(&env, &other_id));

    for name in &emitted {
        assert!(
            name.len() <= 32,
            "event name `{name}` is {} characters; Soroban rejects symbols over 32",
            name.len()
        );
    }

    emitted.sort();
    emitted.dedup();

    assert_eq!(
        emitted,
        snapshot_event_names(),
        "emitted event names drifted from abis/settlement-workflow.json — \
         run `bash scripts/regen-abis.sh`"
    );
}

/// Extracts the `"events"` array from `abis/settlement-workflow.json`, sorted.
///
/// Hand-rolled rather than pulling in a JSON parser: the snapshot is a flat list of
/// string literals, and a test-only dependency would have to clear the repo's
/// license/advisory policy for no real gain.
fn snapshot_event_names() -> std::vec::Vec<String> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../abis/settlement-workflow.json"
    );
    let raw = std::fs::read_to_string(path).expect("abis/settlement-workflow.json must exist");
    let array = raw
        .split_once("\"events\"")
        .expect("snapshot must contain an \"events\" array")
        .1
        .split_once('[')
        .expect("\"events\" must be an array")
        .1
        .split_once(']')
        .expect("\"events\" array must be terminated")
        .0;
    let mut names: std::vec::Vec<String> = array
        .split(',')
        .filter_map(|entry| {
            let trimmed = entry.trim();
            let inner = trimmed.strip_prefix('"')?.strip_suffix('"')?;
            (!inner.is_empty()).then(|| inner.to_string())
        })
        .collect();
    names.sort();
    names
}
