//! Re-initialisation guard for `SettlementWorkflowContract::initialize` (#620).
//!
//! `initialize` is what pins the compliance and treasury instances the workflow
//! treats as authoritative. If it could be called a second time, anyone able to
//! submit a transaction could repoint the gate at a contract they control — a
//! no-op compliance stub that approves everything, and a treasury they own — and
//! the compliance check would become decorative. The guard in
//! `contracts/settlement-workflow/src/lib.rs` already rejects a repeat call with
//! `TreasuryError::AlreadyInitialized`; this file pins that behaviour down so a
//! later refactor cannot quietly drop it.
//!
//! The existing coverage in `settlement_workflow_test.rs`
//! (`initialize_is_idempotent_and_pins_trusted_instances`) only re-initialises
//! with the *same* addresses, so it would still pass against an implementation
//! that blindly overwrote both keys on every call. The dangerous case is
//! re-initialising with *different* addresses, which is what these tests drive:
//! the call must fail with a typed error, the stored links must be unchanged, and
//! the workflow must keep gating through the instances it was originally pinned
//! to.

use compliance::{ComplianceContract, ComplianceContractClient};
use settlement_workflow::{DataKey, SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{testutils::Address as _, token, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

const SETTLEMENT_AMOUNT: i128 = 10_000_000;

/// A freshly initialized compliance + treasury pair. Tests stand up two of these
/// and then try to re-pin the workflow onto the second.
struct Pair {
    compliance_id: Address,
    treasury_id: Address,
    token_id: Address,
}

fn deploy_pair(env: &Env, admin: &Address) -> Pair {
    let compliance_id = env.register_contract(None, ComplianceContract);
    ComplianceContractClient::new(env, &compliance_id).initialize(admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    TreasuryContractClient::new(env, &treasury_id)
        .initialize(admin, &1, &soroban_sdk::Vec::new(env));

    // Fund each treasury so an execution can be observed by checking the payer's
    // token balance rather than only the returned Ok/Err.
    let token_id = env.register_stellar_asset_contract(admin.clone());
    token::StellarAssetClient::new(env, &token_id).mint(&treasury_id, &SETTLEMENT_AMOUNT);

    Pair {
        compliance_id,
        treasury_id,
        token_id,
    }
}

/// A workflow pinned to [`Fixture::first`], plus a decoy [`Fixture::second`]
/// pair that the re-initialisation attempt tries to pivot onto.
struct Fixture {
    env: Env,
    admin: Address,
    workflow_id: Address,
    workflow: SettlementWorkflowContractClient<'static>,
    first: Pair,
    second: Pair,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);

    let first = deploy_pair(&env, &admin);
    let second = deploy_pair(&env, &admin);

    let workflow_id = env.register_contract(None, SettlementWorkflowContract);
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    workflow.initialize(&first.compliance_id, &first.treasury_id);

    // The workflow executes settlements as its own address, so it must be a
    // registered signer on the treasury it was pinned to. It is deliberately
    // *not* registered on the decoy treasury, so a successful re-init would
    // surface as a failure to pay out rather than a silent pass.
    TreasuryContractClient::new(&env, &first.treasury_id).set_signer(&admin, &workflow_id, &1);

    Fixture {
        env,
        admin,
        workflow_id,
        workflow,
        first,
        second,
    }
}

/// Reads a `DataKey` out of the workflow's own instance storage from the test
/// process. Used instead of a public getter so the contract's ABI does not have
/// to grow an accessor that exists only for tests.
fn read_workflow_key(env: &Env, workflow_id: &Address, key: &DataKey) -> Option<Address> {
    env.as_contract(workflow_id, || {
        env.storage().instance().get::<_, Address>(key)
    })
}

/// Re-attempt the pivot onto the decoy pair and discard the outcome, for tests
/// whose real subject is what the workflow does *afterwards*.
fn attempt_reinit(f: &Fixture) {
    let _ = f
        .workflow
        .try_initialize(&f.second.compliance_id, &f.second.treasury_id);
}

/// A second `initialize` with *different* compliance/treasury addresses must
/// fail with a typed `AlreadyInitialized` error rather than repointing the gate.
#[test]
fn second_initialize_with_different_arguments_fails_with_typed_error() {
    let f = setup();

    let err = f
        .workflow
        .try_initialize(&f.second.compliance_id, &f.second.treasury_id)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::AlreadyInitialized.into());
}

/// The rejected re-initialisation must not have written either link. Compared
/// against the *original* pair so a swapped-in pair is caught directly, rather
/// than only inferring it from later behaviour.
#[test]
fn reinit_does_not_overwrite_pinned_compliance_or_treasury() {
    let f = setup();
    attempt_reinit(&f);

    assert_eq!(
        read_workflow_key(&f.env, &f.workflow_id, &DataKey::ComplianceId),
        Some(f.first.compliance_id.clone()),
        "re-initialisation must not repoint the pinned compliance instance"
    );
    assert_eq!(
        read_workflow_key(&f.env, &f.workflow_id, &DataKey::TreasuryId),
        Some(f.first.treasury_id.clone()),
        "re-initialisation must not repoint the pinned treasury instance"
    );
}

/// Behavioural counterpart to the storage assertion above: a merchant the
/// *decoy* compliance instance approves must still be rejected, proving the
/// workflow is not consulting the replacement instance. This is the property an
/// attacker would be after.
#[test]
fn reinit_does_not_let_decoy_compliance_approve_a_merchant() {
    let f = setup();

    // Allow the merchant on the decoy compliance only.
    let merchant = Address::generate(&f.env);
    ComplianceContractClient::new(&f.env, &f.second.compliance_id)
        .allow_address(&f.admin, &merchant);

    let treasury = TreasuryContractClient::new(&f.env, &f.first.treasury_id);
    let settlement_id = treasury.propose_settlement(&f.admin, &merchant, &SETTLEMENT_AMOUNT);

    attempt_reinit(&f);

    let err = f
        .workflow
        .try_execute_with_compliance(&settlement_id, &f.first.token_id, &merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());
    assert_eq!(
        token::Client::new(&f.env, &f.first.token_id).balance(&merchant),
        0
    );
}

/// The mirror image: the workflow must still *accept* a merchant that the
/// originally pinned compliance instance approves, and pay out from the
/// originally pinned treasury. Guards against a re-initialisation attempt
/// wedging the workflow by leaving it pointing at nothing or somewhere new.
#[test]
fn workflow_still_executes_against_the_original_pairs_after_reinit_attempt() {
    let f = setup();

    let merchant = Address::generate(&f.env);
    ComplianceContractClient::new(&f.env, &f.first.compliance_id).allow_address(&f.admin, &merchant);

    let treasury = TreasuryContractClient::new(&f.env, &f.first.treasury_id);
    let settlement_id = treasury.propose_settlement(&f.admin, &merchant, &SETTLEMENT_AMOUNT);

    attempt_reinit(&f);

    f.workflow
        .try_execute_with_compliance(&settlement_id, &f.first.token_id, &merchant)
        .unwrap()
        .unwrap();

    // Paid out of the pinned treasury's funded balance, and not out of the
    // decoy treasury — proof the original treasury is still the one in use.
    assert_eq!(
        token::Client::new(&f.env, &f.first.token_id).balance(&merchant),
        SETTLEMENT_AMOUNT
    );
    assert_eq!(
        token::Client::new(&f.env, &f.second.token_id).balance(&merchant),
        0
    );
}

/// Repeated re-initialisation attempts are all rejected, and none of them leave
/// the workflow wedged: the original pair still works afterwards.
#[test]
fn repeated_reinit_attempts_are_all_rejected() {
    let f = setup();

    for _ in 0..3 {
        let err = f
            .workflow
            .try_initialize(&f.second.compliance_id, &f.second.treasury_id)
            .unwrap_err()
            .unwrap();
        assert_eq!(err, TreasuryError::AlreadyInitialized.into());
    }

    let merchant = Address::generate(&f.env);
    ComplianceContractClient::new(&f.env, &f.first.compliance_id).allow_address(&f.admin, &merchant);
    let treasury = TreasuryContractClient::new(&f.env, &f.first.treasury_id);
    let settlement_id = treasury.propose_settlement(&f.admin, &merchant, &SETTLEMENT_AMOUNT);

    f.workflow
        .try_execute_with_compliance(&settlement_id, &f.first.token_id, &merchant)
        .unwrap()
        .unwrap();
    assert_eq!(
        token::Client::new(&f.env, &f.first.token_id).balance(&merchant),
        SETTLEMENT_AMOUNT
    );
}
