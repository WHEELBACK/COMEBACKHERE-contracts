//! Two-step admin rotation for `SettlementWorkflowContract` (#621).
//!
//! The workflow's admin is fixed at initialization, so a lost or leaked key means
//! redeploying the contract. `transfer_admin` / `accept_admin` mirror the pattern
//! invoice and compliance already use: the current admin nominates, the nominee
//! accepts, and until the nominee accepts nothing has changed. That second step is
//! what makes a typo harmless — nominating an address nobody controls is a
//! recoverable mistake, not a permanent loss of the contract.
//!
//! The failure modes these tests pin down are the ones a naive single-step
//! implementation would have:
//!
//! * the role moving on `transfer_admin` alone, before the nominee has accepted;
//! * anyone being able to accept, not just the nominee;
//! * `accept_admin` succeeding with nothing outstanding;
//! * the outgoing admin keeping power after a completed handover;
//! * a completed handover being replayable.

use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{DataKey, SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    token, Address, Env, FromVal, Symbol,
};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

const SETTLEMENT_AMOUNT: i128 = 10_000_000;

struct Fixture {
    env: Env,
    admin: Address,
    workflow_id: Address,
    workflow: SettlementWorkflowContractClient<'static>,
    compliance_id: Address,
    treasury_id: Address,
    token_id: Address,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    ComplianceContractClient::new(&env, &compliance_id).initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));

    let workflow_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    workflow.initialize(&admin, &compliance_id, &treasury_id);
    treasury.set_signer(&admin, &workflow_id, &1);

    let token_id = env.register_stellar_asset_contract(admin.clone());
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &SETTLEMENT_AMOUNT);

    Fixture {
        env,
        admin,
        workflow_id,
        workflow,
        compliance_id,
        treasury_id,
        token_id,
    }
}

/// Reads a `DataKey` out of the workflow's instance storage from the test process,
/// so the stored role can be asserted directly instead of inferred from
/// behaviour. Avoids adding an ABI getter that exists only for tests.
fn read_workflow_key(env: &Env, workflow_id: &Address, key: &DataKey) -> Option<Address> {
    env.as_contract(workflow_id, || {
        env.storage().instance().get::<_, Address>(key)
    })
}

fn stored_admin(f: &Fixture) -> Option<Address> {
    read_workflow_key(&f.env, &f.workflow_id, &DataKey::Admin)
}

fn stored_pending_admin(f: &Fixture) -> Option<Address> {
    read_workflow_key(&f.env, &f.workflow_id, &DataKey::PendingAdmin)
}

/// Event names emitted by the *workflow* contract, in order.
///
/// The Soroban host clears its event buffer at the start of every top-level
/// invocation, so this only reflects the most recent call. Tests that assert on
/// events must therefore read them immediately after the call under test, with
/// no intervening contract call — hence the per-test assertions below rather
/// than a single shared "collect everything" sweep.
fn workflow_event_names(env: &Env, workflow_id: &Address) -> Vec<String> {
    env.events()
        .all()
        .into_iter()
        .filter(|(emitter, _, _)| *emitter == *workflow_id)
        .map(|(_, topics, _)| {
            let name: Symbol = FromVal::from_val(env, &topics.get_unchecked(0));
            name.to_string()
        })
        .collect()
}

/// Minimal setup that leaves `workflow.initialize` as the last contract call, so
/// its event buffer is still intact when read.
fn setup_bare() -> (Env, Address, SettlementWorkflowContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let compliance_id = env.register_contract(None, ComplianceContract);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let workflow_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    workflow.initialize(&admin, &compliance_id, &treasury_id);
    (env, workflow_id, workflow)
}

// -------------------------------------------------------------------------
// Happy path
// -------------------------------------------------------------------------

/// The full rotation: nominate, accept, and the role moves. Also confirms the
/// admin slot is cleared, so the transfer is not replayable.
#[test]
fn admin_rotation_moves_the_role_on_acceptance() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    f.workflow.transfer_admin(&f.admin, &new_admin);
    // The role has not moved yet — only a nomination exists.
    assert_eq!(stored_admin(&f), Some(f.admin.clone()));
    assert_eq!(stored_pending_admin(&f), Some(new_admin.clone()));

    f.workflow.accept_admin(&new_admin);
    assert_eq!(stored_admin(&f), Some(new_admin.clone()));
    assert_eq!(
        stored_pending_admin(&f),
        None,
        "a completed handover must clear the nomination"
    );
}

/// After a completed handover the new admin holds the role's powers and the old
/// admin holds none.
#[test]
fn rotation_transfers_authority_to_the_new_admin() {
    let f = setup();
    let new_admin = Address::generate(&f.env);
    let third = Address::generate(&f.env);

    f.workflow.transfer_admin(&f.admin, &new_admin);
    f.workflow.accept_admin(&new_admin);

    // The new admin can act...
    f.workflow.transfer_admin(&new_admin, &third);
    // ...and the old one cannot.
    let err = f
        .workflow
        .try_transfer_admin(&f.admin, &f.admin)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::Unauthorized.into());
}

/// A completed rotation leaves the workflow fully functional — the compliance
/// gate still gates and settlements still pay out.
#[test]
fn workflow_still_executes_after_a_rotation() {
    let f = setup();
    let new_admin = Address::generate(&f.env);
    f.workflow.transfer_admin(&f.admin, &new_admin);
    f.workflow.accept_admin(&new_admin);

    // Compliance's own admin is unchanged by the workflow rotation — it is a
    // separate contract — so `f.admin` still authorizes the allow.
    let merchant = Address::generate(&f.env);
    ComplianceContractClient::new(&f.env, &f.compliance_id).allow_address(&f.admin, &merchant);
    let treasury = TreasuryContractClient::new(&f.env, &f.treasury_id);
    let settlement_id = treasury.propose_settlement(&f.admin, &merchant, &SETTLEMENT_AMOUNT);

    f.workflow
        .try_execute_with_compliance(&settlement_id, &f.token_id, &merchant)
        .unwrap()
        .unwrap();
    assert_eq!(
        token::Client::new(&f.env, &f.token_id).balance(&merchant),
        SETTLEMENT_AMOUNT
    );
}

// -------------------------------------------------------------------------
// The two-step guarantee
// -------------------------------------------------------------------------

/// The outgoing admin keeps the role until the nominee accepts. This is the whole
/// point of the two-step flow: `transfer_admin` alone must not hand over
/// authority.
#[test]
fn transfer_admin_alone_does_not_move_the_role() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    f.workflow.transfer_admin(&f.admin, &new_admin);

    assert_eq!(
        stored_admin(&f),
        Some(f.admin.clone()),
        "the role must not move until the nominee accepts"
    );
    // And the original admin can still act, so an unaccepted nomination is inert.
    f.workflow.transfer_admin(&f.admin, &new_admin);
}

/// A transfer that is never accepted leaves the original admin in full control —
/// the contract is not stranded, and a lost nomination is recoverable.
#[test]
fn unaccepted_transfer_leaves_the_original_admin_in_control() {
    let f = setup();
    let lost_nominee = Address::generate(&f.env);

    f.workflow.transfer_admin(&f.admin, &lost_nominee);
    // The nominee never calls accept_admin.

    assert_eq!(stored_admin(&f), Some(f.admin.clone()));
    // The original admin can still rotate again, superseding the dead nomination.
    let replacement = Address::generate(&f.env);
    f.workflow.transfer_admin(&f.admin, &replacement);
    assert_eq!(stored_pending_admin(&f), Some(replacement.clone()));
    f.workflow.accept_admin(&replacement);
    assert_eq!(stored_admin(&f), Some(replacement));
}

/// Only the nominee may accept. The issue's "acceptance by the wrong address"
/// case: the admin itself is authorized but is not the nominee.
#[test]
fn accept_admin_rejects_an_address_that_is_not_the_nominee() {
    let f = setup();
    let new_admin = Address::generate(&f.env);
    let impostor = Address::generate(&f.env);

    f.workflow.transfer_admin(&f.admin, &new_admin);

    let err = f.workflow.try_accept_admin(&impostor).unwrap_err().unwrap();
    assert_eq!(err, TreasuryError::Unauthorized.into());
    assert_eq!(
        stored_admin(&f),
        Some(f.admin.clone()),
        "a rejected acceptance must not change the admin"
    );

    // The real nominee can still complete the handover afterwards.
    f.workflow.accept_admin(&new_admin);
    assert_eq!(stored_admin(&f), Some(new_admin));
}

/// A third party cannot accept a nomination they were not part of, even after a
/// legitimate nomination is outstanding.
#[test]
fn unrelated_address_cannot_accept_an_outstanding_nomination() {
    let f = setup();
    let new_admin = Address::generate(&f.env);
    let stranger = Address::generate(&f.env);

    f.workflow.transfer_admin(&f.admin, &new_admin);
    for candidate in [stranger.clone(), f.admin.clone(), f.workflow_id.clone()] {
        let err = f
            .workflow
            .try_accept_admin(&candidate)
            .unwrap_err()
            .unwrap();
        assert_eq!(
            err,
            TreasuryError::Unauthorized.into(),
            "{candidate:?} must not be able to accept"
        );
    }
    assert_eq!(stored_admin(&f), Some(f.admin.clone()));
}

// -------------------------------------------------------------------------
// Error paths
// -------------------------------------------------------------------------

/// `accept_admin` with nothing outstanding is a typed error, not a silent success
/// that would promote an arbitrary caller.
#[test]
fn accept_admin_without_a_nomination_errors() {
    let f = setup();
    let candidate = Address::generate(&f.env);

    let err = f
        .workflow
        .try_accept_admin(&candidate)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::NoPendingAdmin.into());
    assert_eq!(stored_admin(&f), Some(f.admin.clone()));
}

/// A completed handover cannot be replayed: the nomination is cleared, so a
/// second `accept_admin` by the *former* nominee fails rather than re-promoting
/// them.
#[test]
fn completed_handover_cannot_be_replayed() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    f.workflow.transfer_admin(&f.admin, &new_admin);
    f.workflow.accept_admin(&new_admin);

    let err = f
        .workflow
        .try_accept_admin(&new_admin)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::NoPendingAdmin.into());
    assert_eq!(stored_admin(&f), Some(new_admin));
}

/// A non-admin cannot nominate a successor.
#[test]
fn non_admin_cannot_start_a_transfer() {
    let f = setup();
    let impostor = Address::generate(&f.env);

    let err = f
        .workflow
        .try_transfer_admin(&impostor, &impostor)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::Unauthorized.into());
    assert_eq!(
        stored_pending_admin(&f),
        None,
        "a rejected nomination must not be staged"
    );
}

/// A re-nomination supersedes the previous one: the first nominee can no longer
/// accept, and the second one can. This is what stops a lost nominee key from
/// wedging the contract.
#[test]
fn re_nomination_supersedes_the_previous_one() {
    let f = setup();
    let first = Address::generate(&f.env);
    let second = Address::generate(&f.env);

    f.workflow.transfer_admin(&f.admin, &first);
    f.workflow.transfer_admin(&f.admin, &second);

    let err = f.workflow.try_accept_admin(&first).unwrap_err().unwrap();
    assert_eq!(err, TreasuryError::Unauthorized.into());

    f.workflow.accept_admin(&second);
    assert_eq!(stored_admin(&f), Some(second));
}

// -------------------------------------------------------------------------
// Events
// -------------------------------------------------------------------------

/// `transfer_admin` announces the nomination and nothing else — in particular it
/// must not emit `admin_transferred`, because the role has not moved yet. An
/// indexer that treats `admin_transferred` as "admin is now X" would otherwise
/// act on a handover that the nominee never accepted.
#[test]
fn transfer_admin_emits_only_the_initiation_event() {
    let f = setup();
    let new_admin = Address::generate(&f.env);

    f.workflow.transfer_admin(&f.admin, &new_admin);

    let names = workflow_event_names(&f.env, &f.workflow_id);
    assert_eq!(
        names,
        vec!["admin_transfer_initiated".to_string()],
        "transfer_admin must emit exactly one event, and it must be the initiation"
    );
}

/// `accept_admin` emits the completion event, completing the pair the issue asks
/// for: both steps are observable, so indexers can track pending vs. completed
/// handovers without polling storage.
#[test]
fn accept_admin_emits_the_completion_event() {
    let f = setup();
    let new_admin = Address::generate(&f.env);
    f.workflow.transfer_admin(&f.admin, &new_admin);

    f.workflow.accept_admin(&new_admin);

    let names = workflow_event_names(&f.env, &f.workflow_id);
    assert_eq!(
        names,
        vec!["admin_transferred".to_string()],
        "accept_admin must emit exactly one event, and it must be the completion"
    );
}

/// Initialization still emits its own event, and only that — the extra `admin`
/// argument added in #621 must not have displaced it or introduced noise.
#[test]
fn initialize_still_emits_only_its_own_event() {
    let (env, workflow_id, _workflow) = setup_bare();
    let names = workflow_event_names(&env, &workflow_id);
    assert_eq!(names, vec!["workflow_initialized".to_string()]);
}
