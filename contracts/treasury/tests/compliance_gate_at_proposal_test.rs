//! Tests for the optional compliance gate on `propose_settlement` (#571).
//!
//! Complements `compliance_block_between_proposal_execution_test.rs`, which covers
//! the separate *execution*-time gate (enforced by a workflow contract, not
//! treasury itself) and explicitly relies on treasury never gating proposals on
//! compliance when no compliance contract has been pinned. These tests cover the
//! opposite configuration: a treasury that *has* called `set_compliance_id`.

use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

struct Fixture {
    admin: Address,
    compliance: ComplianceContractClient<'static>,
    treasury: TreasuryContractClient<'static>,
}

fn setup(env: &Env) -> Fixture {
    env.mock_all_auths();
    let admin = Address::generate(env);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(env, &compliance_id);
    compliance.initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    treasury.set_compliance_id(&admin, &compliance_id);

    Fixture {
        admin,
        compliance,
        treasury,
    }
}

#[test]
fn proposal_succeeds_when_recipient_is_allowed() {
    let env = Env::default();
    let f = setup(&env);
    let merchant = Address::generate(&env);

    f.compliance.allow_address(&f.admin, &merchant);

    let result = f
        .treasury
        .try_propose_settlement(&f.admin, &merchant, &10_000_000);
    assert!(result.is_ok(), "allowed recipient's proposal must succeed");
}

#[test]
fn proposal_fails_when_recipient_is_blocked() {
    let env = Env::default();
    let f = setup(&env);
    let merchant = Address::generate(&env);

    f.compliance.allow_address(&f.admin, &merchant);
    f.compliance.block_address(&f.admin, &merchant, &None);

    let result = f
        .treasury
        .try_propose_settlement(&f.admin, &merchant, &10_000_000);
    assert_eq!(
        result,
        Err(Ok(TreasuryError::ComplianceCheckFailed)),
        "proposing a settlement to a blocked recipient must fail fast, before any \
         signer spends time approving it"
    );
}

#[test]
fn proposal_fails_when_recipient_was_never_allowed() {
    let env = Env::default();
    let f = setup(&env);
    let merchant = Address::generate(&env);

    // Never allowed at all — compliance defaults to blocked.
    let result = f
        .treasury
        .try_propose_settlement(&f.admin, &merchant, &10_000_000);
    assert_eq!(result, Err(Ok(TreasuryError::ComplianceCheckFailed)));
}

/// Without `set_compliance_id`, proposals are never gated on compliance — the
/// pre-#571 default every existing treasury deployment keeps.
#[test]
fn proposal_succeeds_without_any_pinned_compliance_contract() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    // No set_compliance_id call.

    let result = treasury.try_propose_settlement(&admin, &merchant, &10_000_000);
    assert!(
        result.is_ok(),
        "with no compliance contract pinned, proposals must not be gated at all"
    );
    assert!(treasury.get_compliance_id().is_none());
}

#[test]
fn recipient_blocked_after_proposal_can_still_be_re_proposed_rejected() {
    let env = Env::default();
    let f = setup(&env);
    let merchant = Address::generate(&env);

    f.compliance.allow_address(&f.admin, &merchant);
    let sid = f
        .treasury
        .propose_settlement(&f.admin, &merchant, &10_000_000);

    // Blocked sometime after the first proposal.
    f.compliance.block_address(&f.admin, &merchant, &None);

    // The already-pending settlement is untouched (propose-time gate only checks
    // at proposal, not retroactively) ...
    assert_eq!(f.treasury.get_settlement(&sid).amount, 10_000_000);
    // ... but a *new* proposal to the now-blocked merchant is rejected.
    let result = f
        .treasury
        .try_propose_settlement(&f.admin, &merchant, &5_000_000);
    assert_eq!(result, Err(Ok(TreasuryError::ComplianceCheckFailed)));
}
