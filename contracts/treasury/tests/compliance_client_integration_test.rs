// #76: Integration test using `compliance-client` from a treasury workflow test.
//
// Verifies that `ComplianceClient` (re-exported from the `compliance-client` crate)
// correctly binds to `ComplianceContract` and that a treasury workflow can use it
// to gate settlement execution on compliance status.
//
// This test mirrors `compliance_gate_integration_test.rs` but imports
// `ComplianceClient` from `compliance-client` instead of using
// `ComplianceContractClient` directly from the `compliance` crate.
use compliance::ComplianceContract;
use compliance_client::ComplianceClient;
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

// ─── Test Token ───────────────────────────────────────────────────────────────

#[contract]
struct TestToken;

#[contractimpl]
impl TestToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        let key = ("bal", to.clone());
        let bal: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(bal + amount));
    }

    pub fn balance(env: Env, of: Address) -> i128 {
        let key = ("bal", of);
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        let from_key = ("bal", from.clone());
        let to_key = ("bal", to.clone());
        let from_bal: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
        let to_bal: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&from_key, &(from_bal - amount));
        env.storage().persistent().set(&to_key, &(to_bal + amount));
    }
}

use test_token::{TestToken, TestTokenClient};

// ─── Settlement Workflow using compliance-client ─────────────────────────────

#[contract]
struct ComplianceGatedSettlement;

#[contractimpl]
impl ComplianceGatedSettlement {
    /// Executes a settlement only if the merchant passes the compliance check.
    /// Uses `ComplianceClient` from the `compliance-client` crate.
    pub fn execute(
        env: Env,
        compliance_id: Address,
        treasury_id: Address,
        settlement_id: u64,
        token_id: Address,
        merchant: Address,
    ) {
        let compliance = ComplianceClient::new(&env, &compliance_id);
        if !compliance.is_allowed(&merchant) {
            panic!("ComplianceFailed");
        }
        let treasury = TreasuryContractClient::new(&env, &treasury_id);
        treasury.execute_settlement(&env.current_contract_address(), &settlement_id, &token_id);
    }

    /// Check-only variant: returns Ok if compliance passes, panics otherwise.
    pub fn check_compliance(env: Env, compliance_id: Address, merchant: Address) {
        let compliance = ComplianceClient::new(&env, &compliance_id);
        if !compliance.is_allowed(&merchant) {
            panic!("ComplianceFailed");
        }
    }
}

// ─── Test helpers ─────────────────────────────────────────────────────────────

struct TestContext {
    env: Env,
    admin: Address,
    merchant: Address,
    treasury: TreasuryContractClient<'static>,
    treasury_id: Address,
    compliance: ComplianceClient<'static>,
    compliance_id: Address,
    token: TestTokenClient<'static>,
    token_id: Address,
    workflow_id: Address,
}

fn setup() -> TestContext {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);

    // Deploy compliance contract
    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    // Deploy treasury contract
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));
    treasury.set_signer(&admin, &admin, &2);

    // Deploy test token
    let token_id = env.register_contract(None, TestToken);
    let token = TestTokenClient::new(&env, &token_id);

    // Deploy workflow contract
    let workflow_id = env.register_contract(None, ComplianceGatedSettlement);
    let _workflow = ComplianceGatedSettlementClient::new(&env, &workflow_id);
    treasury.set_signer(&admin, &workflow_id, &1);

    TestContext {
        env,
        admin,
        merchant,
        treasury,
        treasury_id,
        compliance,
        compliance_id,
        token,
        token_id,
        workflow_id,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Happy path: merchant is allowed, settlement executes successfully.
#[test]
fn settlement_proceeds_when_compliance_passing_via_compliance_client() {
    let ctx = setup();

    // Allow the merchant
    ctx.compliance.allow_address(&ctx.admin, &ctx.merchant);
    assert!(ctx.compliance.is_allowed(&ctx.merchant));

    // Create settlement
    let settlement_id = ctx
        .treasury
        .propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000);

    // Fund treasury
    ctx.token.mint(&ctx.treasury_id, &10_000_000);

    let wf = ComplianceGatedSettlementClient::new(&ctx.env, &ctx.workflow_id);

    // Execute via compliance-gated workflow
    wf.execute(
        &ctx.compliance_id,
        &ctx.treasury_id,
        &settlement_id,
        &ctx.token_id,
        &ctx.merchant,
    );

    // Settlement executed and merchant paid
    let settlement = ctx.treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Executed);
    assert_eq!(ctx.token.balance(&ctx.merchant), 10_000_000);
    assert_eq!(
        ctx.treasury.get_settlement(&settlement_id).status,
        SettlementStatus::Executed
    );
}

/// Compliance failure: merchant not allowed → settlement rejected.
#[test]
fn settlement_rejected_when_merchant_not_allowed_via_compliance_client() {
    let ctx = setup();

    // Merchant is NOT allowed (default-deny)
    let settlement_id = ctx
        .treasury
        .propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000);

    ctx.token.mint(&ctx.treasury_id, &10_000_000);

    let wf = ComplianceGatedSettlementClient::new(&ctx.env, &ctx.workflow_id);

    // Execution must fail the compliance check
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wf.execute(
            &ctx.compliance_id,
            &ctx.treasury_id,
            &settlement_id,
            &ctx.token_id,
            &ctx.merchant,
        );
    }));
    assert!(result.is_err(), "expected compliance failure panic");

    // Merchant received nothing
    assert_eq!(ctx.token.balance(&ctx.merchant), 0);
    let settlement = ctx.treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Pending);
}

/// Compliance check: merchant blocked after being allowed → rejected.
#[test]
fn settlement_rejected_when_merchant_blocked_via_compliance_client() {
    let ctx = setup();

    // Allow then block the merchant
    ctx.compliance.allow_address(&ctx.admin, &ctx.merchant);
    ctx.compliance
        .block_address(&ctx.admin, &ctx.merchant, &None);
    assert!(!ctx.compliance.is_allowed(&ctx.merchant));

    let settlement_id = ctx
        .treasury
        .propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000);

    ctx.token.mint(&ctx.treasury_id, &10_000_000);

    let wf = ComplianceGatedSettlementClient::new(&ctx.env, &ctx.workflow_id);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wf.execute(
            &ctx.compliance_id,
            &ctx.treasury_id,
            &settlement_id,
            &ctx.token_id,
            &ctx.merchant,
        );
    }));
    assert!(
        result.is_err(),
        "expected compliance failure for blocked merchant"
    );
    assert_eq!(ctx.token.balance(&ctx.merchant), 0);
}

/// Compliance passes when merchant is allowed with a temp allow.
#[test]
fn settlement_proceeds_with_temp_allow_via_compliance_client() {
    let ctx = setup();

    let now = ctx.env.ledger().timestamp();
    ctx.compliance
        .allow_address_until(&ctx.admin, &ctx.merchant, &(now + 1000));
    assert!(ctx.compliance.is_allowed(&ctx.merchant));

    let settlement_id = ctx
        .treasury
        .propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000);

    ctx.token.mint(&ctx.treasury_id, &10_000_000);

    let wf = ComplianceGatedSettlementClient::new(&ctx.env, &ctx.workflow_id);

    wf.execute(
        &ctx.compliance_id,
        &ctx.treasury_id,
        &settlement_id,
        &ctx.token_id,
        &ctx.merchant,
    );

    assert_eq!(ctx.token.balance(&ctx.merchant), 10_000_000);
    let settlement = ctx.treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Executed);
}

/// Compliance fails when temp allow has expired.
#[test]
fn settlement_rejected_when_temp_allow_expired_via_compliance_client() {
    let ctx = setup();

    let now = ctx.env.ledger().timestamp();
    // Set temp allow that expired in the past
    ctx.compliance
        .allow_address_until(&ctx.admin, &ctx.merchant, &now);
    assert!(!ctx.compliance.is_allowed(&ctx.merchant));

    let settlement_id = ctx
        .treasury
        .propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000);

    ctx.token.mint(&ctx.treasury_id, &10_000_000);

    let wf = ComplianceGatedSettlementClient::new(&ctx.env, &ctx.workflow_id);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wf.execute(
            &ctx.compliance_id,
            &ctx.treasury_id,
            &settlement_id,
            &ctx.token_id,
            &ctx.merchant,
        );
    }));
    assert!(
        result.is_err(),
        "expected compliance failure for expired temp allow"
    );
    assert_eq!(ctx.token.balance(&ctx.merchant), 0);
}

/// check_compliance standalone test via compliance-client.
#[test]
fn check_compliance_returns_ok_for_allowed_merchant_via_compliance_client() {
    let ctx = setup();

    ctx.compliance.allow_address(&ctx.admin, &ctx.merchant);

    let wf = ComplianceGatedSettlementClient::new(&ctx.env, &ctx.workflow_id);

    // Should not panic
    wf.check_compliance(&ctx.compliance_id, &ctx.merchant);
}

/// check_compliance panics for non-allowed merchant.
#[test]
fn check_compliance_panics_for_non_allowed_merchant_via_compliance_client() {
    let ctx = setup();

    let wf = ComplianceGatedSettlementClient::new(&ctx.env, &ctx.workflow_id);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wf.check_compliance(&ctx.compliance_id, &ctx.merchant);
    }));
    assert!(result.is_err(), "expected panic for non-allowed merchant");
}
