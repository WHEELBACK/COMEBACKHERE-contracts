//! #73: atomic rollback on partial cross-contract failure during an emergency
//! pause.
//!
//! `emergency_pause_all` fans `pause` out across the protocol's contracts in
//! sequence. The failure this suite exists to prevent is the one in the issue: if
//! one contract refuses to pause and the sweep carries on anyway — or swallows
//! the error — the protocol is left **split**, with some contracts stopped and
//! others still live. During an incident that is worse than not pausing at all,
//! because the operator can no longer tell which half of the protocol is moving
//! money.
//!
//! So the property under test is not "every target ends up paused" but the
//! stronger, all-or-nothing one: **after a failed sweep, the protocol is exactly
//! as it was before**. The cooperative targets here are the real compliance and
//! treasury contracts, probed through genuinely pause-gated entrypoints, so a
//! rollback that only appears to work — because the test read a flag instead of
//! exercising the contract's behaviour — cannot pass.

use compliance::{ComplianceContract, ComplianceContractClient, ContractError};
use settlement_workflow::{
    SettlementWorkflowContract, SettlementWorkflowContractClient, WorkflowError, MAX_PAUSE_TARGETS,
};
use soroban_sdk::{testutils::Address as _, vec, Address, Env, Vec};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

/// A target that refuses to pause, standing in for the real-world causes: a
/// contract deployed without a `pause` export, one that traps inside `pause`, or
/// one whose admin key is not the orchestrator. All three reach the sweep as a
/// failed invocation, which is what `try_pause` reports.
mod uncooperative {
    use soroban_sdk::{contract, contracterror, contractimpl, panic_with_error, Address, Env};

    #[contracterror]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u32)]
    pub enum Refusal {
        CannotPause = 1,
    }

    #[contract]
    pub struct Uncooperative;

    #[contractimpl]
    impl Uncooperative {
        /// Always traps, and records nothing. A test that only inspected this
        /// contract's own state would see "nothing happened here" and wrongly
        /// conclude the sweep was clean.
        pub fn pause(env: Env, _admin: Address) {
            panic_with_error!(&env, Refusal::CannotPause);
        }

        /// Unpause succeeds, so a failure in the pause direction cannot be
        /// confused with a contract that is simply inert.
        pub fn unpause(_env: Env, _admin: Address) {}
    }
}

use uncooperative::Uncooperative;

/// A target that honours `pause` but refuses to `unpause`, for the
/// mirror-image rollback test on `resume_all`.
mod stuck {
    use soroban_sdk::{contract, contracterror, contractimpl, panic_with_error, Address, Env};

    #[contracterror]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u32)]
    pub enum Stuck {
        CannotUnpause = 1,
    }

    #[contract]
    pub struct StuckPaused;

    #[contractimpl]
    impl StuckPaused {
        pub fn pause(_env: Env, _admin: Address) {}

        pub fn unpause(env: Env, _admin: Address) {
            panic_with_error!(&env, Stuck::CannotUnpause);
        }
    }
}

use stuck::StuckPaused;

struct Fixture {
    env: Env,
    workflow_id: Address,
    workflow: SettlementWorkflowContractClient<'static>,
    operator: Address,
    compliance: ComplianceContractClient<'static>,
    treasury: TreasuryContractClient<'static>,
    token_id: Address,
    bystander: Address,
    third_party: Address,
}

/// Registers the orchestrator and its two cooperative targets, with the
/// orchestrator installed as each target's admin — the wiring an emergency pause
/// depends on, since the sweep passes its own address to `pause(admin)`. Treasury
/// exposes no `transfer_admin`, so it is initialized with the orchestrator as its
/// admin directly.
fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();

    let operator = Address::generate(&env);
    let workflow_id = env.register(SettlementWorkflowContract, ());
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);

    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&workflow_id);

    let treasury_id = env.register(TreasuryContract, ());
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&workflow_id, &1, &Vec::new(&env));

    let token_id = env.register_stellar_asset_contract(operator.clone());
    let bystander = Address::generate(&env);
    let third_party = Address::generate(&env);

    workflow.initialize(&compliance_id, &treasury_id);

    Fixture {
        env,
        workflow_id,
        workflow,
        operator,
        compliance,
        treasury,
        token_id,
        bystander,
        third_party,
    }
}

impl Fixture {
    fn compliance_id(&self) -> Address {
        self.compliance.address.clone()
    }

    fn treasury_id(&self) -> Address {
        self.treasury.address.clone()
    }

    /// The two cooperative targets, in the order a sweep visits them.
    fn both_targets(&self) -> Vec<Address> {
        vec![&self.env, self.compliance_id(), self.treasury_id()]
    }

    /// The same two, with `extra` appended — i.e. a contract that will fail
    /// *after* both real contracts have already been paused.
    fn targets_then(&self, extra: &Address) -> Vec<Address> {
        let mut all = self.both_targets();
        all.push_back(extra.clone());
        all
    }

    fn configure(&self, targets: &Vec<Address>) {
        self.workflow
            .initialize_emergency_pause(&self.operator, targets);
    }

    /// Behavioural probe for "is compliance running?": `allow_address` is gated
    /// by `require_not_paused`, so a rejection with `ContractPaused` proves the
    /// contract is stopped. Asserting on the error rather than on success keeps
    /// the probe independent of the allowlist's contents.
    fn compliance_is_running(&self) -> bool {
        !matches!(
            self.compliance
                .try_allow_address(&self.workflow_id, &self.third_party),
            Err(Ok(ContractError::ContractPaused))
        )
    }

    /// Behavioural probe for "is treasury running?": `deposit` is gated by
    /// `require_not_paused`, so a `ContractPaused` rejection proves it is
    /// stopped. The caller has no balance, so the unpaused call still fails —
    /// for a different reason, which is exactly why the probe keys off the
    /// specific error rather than off success.
    fn treasury_is_running(&self) -> bool {
        !matches!(
            self.treasury
                .try_deposit(&self.third_party, &self.token_id, &1),
            Err(Ok(TreasuryError::ContractPaused))
        )
    }

    fn protocol_is_live(&self) -> bool {
        self.compliance_is_running() && self.treasury_is_running()
    }
}

// ── the whole point: a failed sweep leaves nothing paused ────────────────────

/// The issue's scenario, end to end. Compliance and treasury are paused
/// successfully, then a third contract refuses. The sweep must fail — and,
/// critically, the two contracts it already paused must be rolled back, because
/// a protocol with compliance stopped and the treasury still live is the exact
/// split state the issue is about.
#[test]
fn a_target_that_refuses_to_pause_leaves_earlier_targets_rolled_back() {
    let f = setup();
    let refuser = f.env.register(Uncooperative, ());
    f.configure(&f.targets_then(&refuser));
    assert!(f.protocol_is_live(), "precondition: protocol starts live");

    assert_eq!(
        f.workflow.try_emergency_pause_all(&f.operator),
        Err(Ok(WorkflowError::PauseTargetFailed))
    );

    assert!(
        f.protocol_is_live(),
        "a contract paused before the failure was not rolled back: split state"
    );
    assert_eq!(
        f.workflow.get_emergency_paused_at(),
        None,
        "a failed sweep must not leave the incident flag set"
    );
}

/// A refusal by the *first* target is reported the same way as a refusal by the
/// last: the caller cannot tell from the error which contract failed, and cannot
/// be left believing a partial sweep happened.
#[test]
fn a_first_target_that_refuses_aborts_before_anything_is_paused() {
    let f = setup();
    let refuser = f.env.register(Uncooperative, ());
    let mut ordered = vec![&f.env, refuser.clone()];
    ordered.push_back(f.compliance_id());
    ordered.push_back(f.treasury_id());
    f.configure(&ordered);

    assert_eq!(
        f.workflow.try_emergency_pause_all(&f.operator),
        Err(Ok(WorkflowError::PauseTargetFailed))
    );
    assert!(f.protocol_is_live());
    assert_eq!(f.workflow.get_emergency_paused_at(), None);
}

/// A sweep that was never configured must do nothing at all — in particular it
/// must not fall back to "pause everything I can find".
#[test]
fn an_unconfigured_sweep_is_refused() {
    let f = setup();
    assert_eq!(
        f.workflow.try_emergency_pause_all(&f.operator),
        Err(Ok(WorkflowError::NotConfigured))
    );
    assert!(f.protocol_is_live());
    assert_eq!(
        f.workflow.try_get_emergency_pause_targets(),
        Err(Ok(WorkflowError::NotConfigured))
    );
    assert_eq!(
        f.workflow.try_get_emergency_pause_admin(),
        Err(Ok(WorkflowError::NotConfigured))
    );
}

// ── the happy path still works, and is observable ───────────────────────────

/// A sweep where every target agrees pauses all of them, reports them in
/// configuration order, and records that the incident is open.
#[test]
fn a_sweep_that_fully_succeeds_pauses_every_target() {
    let f = setup();
    let expected = f.both_targets();
    f.configure(&expected);

    assert_eq!(f.workflow.emergency_pause_all(&f.operator), expected);
    assert!(!f.compliance_is_running());
    assert!(!f.treasury_is_running());
    assert_eq!(
        f.workflow.get_emergency_paused_at(),
        Some(f.env.ledger().timestamp())
    );
}

/// `resume_all` is the mirror image: same all-or-nothing rule, undoing exactly
/// what the sweep did.
#[test]
fn resume_restores_every_target_and_clears_the_incident() {
    let f = setup();
    f.configure(&f.both_targets());
    f.workflow.emergency_pause_all(&f.operator);
    assert!(!f.compliance_is_running());

    let resumed = f.workflow.resume_all(&f.operator);
    assert_eq!(resumed.len(), 2);
    assert!(f.protocol_is_live());
    assert_eq!(f.workflow.get_emergency_paused_at(), None);
}

/// A target that refuses to unpause must roll back the ones already resumed, so
/// the protocol is never left partially resumed — the mirror of the pause rule.
#[test]
fn a_target_that_refuses_to_unpause_leaves_earlier_targets_resumed_rolled_back() {
    let f = setup();
    let stuck = f.env.register(StuckPaused, ());
    f.configure(&f.targets_then(&stuck));
    f.workflow.emergency_pause_all(&f.operator);
    assert!(!f.compliance_is_running());

    assert_eq!(
        f.workflow.try_resume_all(&f.operator),
        Err(Ok(WorkflowError::UnpauseTargetFailed))
    );

    assert!(
        !f.protocol_is_live(),
        "a contract resumed before the failure was not rolled back"
    );
    assert_eq!(
        f.workflow.get_emergency_paused_at(),
        Some(f.env.ledger().timestamp()),
        "a failed resume must not clear the incident flag"
    );
}

/// `resume_all` refuses when no sweep is open, so the orchestrator cannot be
/// used to unpause a protocol somebody else paused through their own admin key.
#[test]
fn resume_is_refused_when_no_emergency_pause_is_active() {
    let f = setup();
    f.configure(&f.both_targets());
    assert_eq!(
        f.workflow.try_resume_all(&f.operator),
        Err(Ok(WorkflowError::NotEmergencyPaused))
    );
    assert!(f.protocol_is_live());
}

// ── authorisation ───────────────────────────────────────────────────────────

/// Only the configured admin can stop the protocol. Anyone else is refused
/// before a single target is touched.
#[test]
fn only_the_configured_admin_can_trigger_a_sweep() {
    let f = setup();
    f.configure(&f.both_targets());

    assert_eq!(
        f.workflow.try_emergency_pause_all(&f.bystander),
        Err(Ok(WorkflowError::Unauthorized))
    );
    assert!(f.protocol_is_live());
    assert_eq!(f.workflow.get_emergency_paused_at(), None);
}

/// The same holds for resuming: a stranger cannot use the orchestrator as a
/// universal unpause.
#[test]
fn only_the_configured_admin_can_trigger_a_resume() {
    let f = setup();
    f.configure(&f.both_targets());
    f.workflow.emergency_pause_all(&f.operator);

    assert_eq!(
        f.workflow.try_resume_all(&f.bystander),
        Err(Ok(WorkflowError::Unauthorized))
    );
    assert!(
        !f.compliance_is_running(),
        "an unauthorised resume must not unpause anything"
    );
}

/// The admin is recorded, so an operator can confirm who holds the key.
#[test]
fn the_configured_admin_is_readable() {
    let f = setup();
    f.configure(&f.both_targets());
    assert_eq!(f.workflow.get_emergency_pause_admin(), f.operator);
}

// ── configuration is validated before anything is written ───────────────────

/// Every structural rejection happens at configuration time, before a single
/// byte is stored, so a bad target set can never be discovered halfway through
/// an incident — when the sweep would already have paused something.
#[test]
fn an_invalid_target_set_is_rejected_at_configuration_time() {
    let f = setup();

    // Empty: nothing to coordinate.
    assert_eq!(
        f.workflow
            .try_initialize_emergency_pause(&f.operator, &Vec::new(&f.env)),
        Err(Ok(WorkflowError::NoPauseTargets))
    );

    // A target listed twice would be paused twice, making the second call the
    // one that decides whether the sweep succeeds.
    let mut duplicated = f.both_targets();
    duplicated.push_back(f.compliance_id());
    assert_eq!(
        f.workflow
            .try_initialize_emergency_pause(&f.operator, &duplicated),
        Err(Ok(WorkflowError::DuplicatePauseTarget))
    );

    // The orchestrator cannot pause itself: doing so part-way through the sweep
    // would make every remaining call fail and strand the protocol.
    let mut suicidal = f.both_targets();
    suicidal.push_back(f.workflow_id.clone());
    assert_eq!(
        f.workflow
            .try_initialize_emergency_pause(&f.operator, &suicidal),
        Err(Ok(WorkflowError::SelfPauseTarget))
    );

    // An unbounded sweep could exhaust the instruction budget part-way through.
    let mut too_many = Vec::new(&f.env);
    for _ in 0..=MAX_PAUSE_TARGETS {
        too_many.push_back(Address::generate(&f.env));
    }
    assert_eq!(
        f.workflow
            .try_initialize_emergency_pause(&f.operator, &too_many),
        Err(Ok(WorkflowError::TooManyPauseTargets))
    );

    // None of the rejections stored anything, so the workflow is still
    // unconfigured rather than half-configured.
    assert_eq!(
        f.workflow.try_get_emergency_pause_targets(),
        Err(Ok(WorkflowError::NotConfigured))
    );
}

/// A target that is a plain address rather than a contract is caught as a failed
/// invocation at sweep time, and aborts the sweep — it is never silently
/// skipped, which is the other way split state gets introduced.
#[test]
fn a_target_that_is_not_a_contract_aborts_the_sweep() {
    let f = setup();
    f.configure(&f.targets_then(&f.bystander));

    assert_eq!(
        f.workflow.try_emergency_pause_all(&f.operator),
        Err(Ok(WorkflowError::PauseTargetFailed))
    );
    assert!(
        f.protocol_is_live(),
        "an address that is not a contract must abort the sweep, not be skipped"
    );
    assert_eq!(f.workflow.get_emergency_paused_at(), None);
}

/// Reconfiguration is refused while an incident is open, because a new target
/// set would not describe the contracts that are actually paused — `resume_all`
/// would then leave the old ones stuck forever.
#[test]
fn the_target_set_cannot_be_repointed_mid_incident() {
    let f = setup();
    f.configure(&f.both_targets());
    f.workflow.emergency_pause_all(&f.operator);

    let replacement = vec![&f.env, f.compliance_id()];
    assert_eq!(
        f.workflow
            .try_initialize_emergency_pause(&f.bystander, &replacement),
        Err(Ok(WorkflowError::EmergencyPauseActive))
    );

    // The original set is untouched, so the pause can still be undone.
    let mut full = replacement;
    full.push_back(f.treasury_id());
    f.workflow.resume_all(&f.operator);
    assert!(f.protocol_is_live());
}

/// Once the incident is closed the target set can be replaced, so a compromised
/// or upgraded-away contract can be swapped without redeploying the orchestrator.
#[test]
fn the_target_set_can_be_replaced_after_an_incident_is_closed() {
    let f = setup();
    f.configure(&f.both_targets());
    f.workflow.emergency_pause_all(&f.operator);
    f.workflow.resume_all(&f.operator);

    let replacement = vec![&f.env, f.compliance_id()];
    f.workflow
        .initialize_emergency_pause(&f.operator, &replacement);
    assert_eq!(f.workflow.get_emergency_pause_targets(), replacement);

    // The new set is what a sweep now covers, and no longer includes treasury.
    f.workflow.emergency_pause_all(&f.operator);
    assert!(!f.compliance_is_running());
    assert!(
        f.treasury_is_running(),
        "treasury is no longer a target, so it must still be live"
    );
}

/// The configured order is the sweep order, and `resume_all` returns the same set
/// reversed — the ordering guarantee the recovery path depends on.
#[test]
fn the_configured_order_is_the_sweep_order() {
    let f = setup();
    let ordered = f.both_targets();
    let expected = ordered.clone();
    f.configure(&ordered);

    assert_eq!(f.workflow.get_emergency_pause_targets(), expected);
    assert_eq!(f.workflow.emergency_pause_all(&f.operator), expected);

    // Recovery runs the same set in the opposite order: the contract paused
    // last is the first one reachable again.
    let mut reversed = Vec::new(&f.env);
    let mut index = expected.len();
    while index > 0 {
        index -= 1;
        reversed.push_back(expected.get(index).unwrap());
    }
    assert_eq!(f.workflow.resume_all(&f.operator), reversed);
    assert!(f.protocol_is_live());
}
