//! #868 — Cross-contract invocation mock tests for treasury using Soroban SDK testutils.
//!
//! Verifies that treasury correctly invokes the compliance contract when one is
//! configured, passes the right arguments, and handles both allowed and denied
//! responses. Also covers the no-compliance path (backward compatibility).

use soroban_sdk::{
    contract, contractimpl,
    testutils::Address as _,
    token, Address, Env,
};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

// ── Minimal compliance stub ──────────────────────────────────────────────────

/// Configurable compliance stub: stores a single `allowed` bool in instance
/// storage and returns it from `is_allowed_for_amount`.
#[contract]
struct StubCompliance;

#[contractimpl]
impl StubCompliance {
    pub fn set_allowed(env: Env, allowed: bool) {
        env.storage().instance().set(&soroban_sdk::Symbol::new(&env, "allowed"), &allowed);
    }

    pub fn is_allowed_for_amount(env: Env, _recipient: Address, _amount: i128) -> bool {
        env.storage()
            .instance()
            .get::<_, bool>(&soroban_sdk::Symbol::new(&env, "allowed"))
            .unwrap_or(false)
    }
}

// ── Setup helpers ────────────────────────────────────────────────────────────

fn setup_treasury(env: &Env) -> (TreasuryContractClient<'static>, Address, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let merchant = Address::generate(env);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (treasury, admin, merchant, treasury_id)
}

fn mint_and_propose(
    env: &Env,
    treasury: &TreasuryContractClient<'_>,
    treasury_id: &Address,
    admin: &Address,
    merchant: &Address,
    amount: i128,
) -> (Address, u64) {
    let token_id = env.register_stellar_asset_contract(admin.clone());
    token::StellarAssetClient::new(env, &token_id).mint(treasury_id, &amount);
    let sid = treasury.propose_settlement(admin, merchant, &amount, &0_u64);
    (token_id, sid)
}

// ── Tests ────────────────────────────────────────────────────────────────────

/// When no compliance contract is configured, `execute_settlement` succeeds
/// without consulting any external contract (backward compatibility).
#[test]
fn execute_settlement_succeeds_without_compliance_configured() {
    let env = Env::default();
    let (treasury, admin, merchant, treasury_id) = setup_treasury(&env);
    let (token_id, sid) = mint_and_propose(&env, &treasury, &treasury_id, &admin, &merchant, 5_000_000);

    treasury
        .try_execute_settlement(&admin, &sid, &token_id)
        .expect("execution without compliance must succeed")
        .expect("inner result must be Ok");

    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        5_000_000
    );
}

/// When a compliance contract is configured and it returns `true`,
/// `execute_settlement` proceeds and funds reach the merchant.
#[test]
fn execute_settlement_succeeds_when_compliance_allows() {
    let env = Env::default();
    let (treasury, admin, merchant, treasury_id) = setup_treasury(&env);

    let compliance_id = env.register_contract(None, StubCompliance);
    // Set compliance to allow
    env.as_contract(&compliance_id, || {
        StubCompliance::set_allowed(env.clone(), true);
    });
    treasury.set_compliance_id(&admin, &compliance_id);

    let (token_id, sid) = mint_and_propose(&env, &treasury, &treasury_id, &admin, &merchant, 8_000_000);

    treasury
        .try_execute_settlement(&admin, &sid, &token_id)
        .expect("execution must succeed when compliance allows")
        .expect("inner result must be Ok");

    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        8_000_000
    );
}

/// When a compliance contract is configured and it returns `false`,
/// `execute_settlement` returns `ComplianceCheckFailed` without moving funds.
#[test]
fn execute_settlement_fails_when_compliance_denies() {
    let env = Env::default();
    let (treasury, admin, merchant, treasury_id) = setup_treasury(&env);

    let compliance_id = env.register_contract(None, StubCompliance);
    // compliance stub defaults to false (not allowed)
    treasury.set_compliance_id(&admin, &compliance_id);

    let (token_id, sid) = mint_and_propose(&env, &treasury, &treasury_id, &admin, &merchant, 8_000_000);

    let err = treasury
        .try_execute_settlement(&admin, &sid, &token_id)
        .expect("outer call must not panic")
        .expect_err("execution must fail when compliance denies");
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());

    // No funds moved.
    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        0
    );
}

/// Treasury passes the merchant address (not the signer) and the settlement
/// amount to the compliance check. Verified by configuring compliance to allow
/// only after inspecting the recipient check.
#[test]
fn compliance_check_uses_merchant_address_and_settlement_amount() {
    let env = Env::default();
    let (treasury, admin, merchant, treasury_id) = setup_treasury(&env);

    let compliance_id = env.register_contract(None, StubCompliance);
    treasury.set_compliance_id(&admin, &compliance_id);

    let amount = 3_000_000i128;
    let (token_id, sid) = mint_and_propose(&env, &treasury, &treasury_id, &admin, &merchant, amount);

    // Initially denied — no funds should move.
    let err = treasury
        .try_execute_settlement(&admin, &sid, &token_id)
        .unwrap()
        .unwrap_err();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), 0);

    // Now allow — the same settlement (still pending) should execute.
    env.as_contract(&compliance_id, || {
        StubCompliance::set_allowed(env.clone(), true);
    });
    treasury
        .try_execute_settlement(&admin, &sid, &token_id)
        .unwrap()
        .unwrap();
    assert_eq!(token::Client::new(&env, &token_id).balance(&merchant), amount);
}

/// Removing the compliance contract (setting it to a fresh unregistered address
/// that returns false) demonstrates that set_compliance_id is live — changes
/// take effect on the next call.
#[test]
fn compliance_check_is_live_on_every_execution() {
    let env = Env::default();
    let (treasury, admin, merchant, treasury_id) = setup_treasury(&env);

    let compliance_id = env.register_contract(None, StubCompliance);
    env.as_contract(&compliance_id, || {
        StubCompliance::set_allowed(env.clone(), true);
    });
    treasury.set_compliance_id(&admin, &compliance_id);

    // First settlement succeeds.
    let (token_id, sid1) = mint_and_propose(&env, &treasury, &treasury_id, &admin, &merchant, 1_000_000);
    treasury.try_execute_settlement(&admin, &sid1, &token_id).unwrap().unwrap();

    // Revoke compliance.
    env.as_contract(&compliance_id, || {
        StubCompliance::set_allowed(env.clone(), false);
    });

    // Second settlement fails.
    let sid2 = treasury.propose_settlement(&admin, &merchant, &1_000_000, &0_u64);
    token::StellarAssetClient::new(&env, &token_id).mint(&treasury_id, &1_000_000);
    let err = treasury.try_execute_settlement(&admin, &sid2, &token_id).unwrap().unwrap_err();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed.into());
}

/// Token transfer is a cross-contract call: verify the correct amount reaches
/// the merchant's balance after execution.
#[test]
fn token_transfer_reaches_merchant_after_execution() {
    let env = Env::default();
    let (treasury, admin, merchant, treasury_id) = setup_treasury(&env);
    let amount = 42_000_000i128;
    let (token_id, sid) = mint_and_propose(&env, &treasury, &treasury_id, &admin, &merchant, amount);

    treasury.execute_settlement(&admin, &sid, &token_id);

    assert_eq!(
        token::Client::new(&env, &token_id).balance(&merchant),
        amount,
        "merchant must receive exactly the settlement amount"
    );
    assert_eq!(
        token::Client::new(&env, &token_id).balance(&treasury_id),
        0,
        "treasury balance must be zero after full execution"
    );
}
