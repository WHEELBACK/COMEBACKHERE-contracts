// #76: Integration test using `compliance-client` from a treasury workflow test.
//
// Verifies that `ComplianceClient` (re-exported from the `compliance-client` crate)
// correctly binds to `ComplianceContract` and that a treasury workflow can use it
// to gate settlement execution on compliance status.
//
// This test mirrors `compliance_gate_integration_test.rs` but imports
// `ComplianceClient` from `compliance-client` instead of using
// `ComplianceContractClient` directly from the `compliance` crate.
//
// `ComplianceClient` only wraps `is_allowed` (see compliance-client's
// Cargo.toml/lib.rs for why), so admin/state-setup calls below go through the
// real generated `ComplianceContractClient` directly; only the `is_allowed`
// assertions go through the wrapper under test.
use compliance::{ComplianceContract, ComplianceContractClient};
use compliance_client::ComplianceClient;
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, MockAuth, MockAuthInvoke},
    Address, Env, IntoVal,
};
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
    treasury_id: Address,
    compliance_id: Address,
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
    let compliance_admin = ComplianceContractClient::new(&env, &compliance_id);
    compliance_admin.initialize(&admin);

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
        treasury_id,
        compliance_id,
        token_id,
        workflow_id,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Happy path: merchant is allowed, settlement executes successfully.
#[test]
fn settlement_proceeds_when_compliance_passing_via_compliance_client() {
    let ctx = setup();

    let compliance = ComplianceClient::new(&ctx.env, &ctx.compliance_id);
    let compliance_admin = ComplianceContractClient::new(&ctx.env, &ctx.compliance_id);
    let treasury = TreasuryContractClient::new(&ctx.env, &ctx.treasury_id);
    let token = TestTokenClient::new(&ctx.env, &ctx.token_id);

    // Allow the merchant
    compliance_admin.allow_address(&ctx.admin, &ctx.merchant);
    assert!(compliance.is_allowed(&ctx.merchant));

    // Create settlement
    let settlement_id = treasury.propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000, &0_u64);

    // Fund treasury
    token.mint(&ctx.treasury_id, &10_000_000);

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
    let settlement = treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Executed);
    assert_eq!(token.balance(&ctx.merchant), 10_000_000);
    assert_eq!(
        treasury.get_settlement(&settlement_id).status,
        SettlementStatus::Executed
    );
}

/// Compliance failure: merchant not allowed → settlement rejected.
#[test]
fn settlement_rejected_when_merchant_not_allowed_via_compliance_client() {
    let ctx = setup();

    let treasury = TreasuryContractClient::new(&ctx.env, &ctx.treasury_id);
    let token = TestTokenClient::new(&ctx.env, &ctx.token_id);

    // Merchant is NOT allowed (default-deny)
    let settlement_id = treasury.propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000, &0_u64);

    token.mint(&ctx.treasury_id, &10_000_000);

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
    assert_eq!(token.balance(&ctx.merchant), 0);
    let settlement = treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Pending);
}

/// Compliance check: merchant blocked after being allowed → rejected.
#[test]
fn settlement_rejected_when_merchant_blocked_via_compliance_client() {
    let ctx = setup();

    let compliance = ComplianceClient::new(&ctx.env, &ctx.compliance_id);
    let compliance_admin = ComplianceContractClient::new(&ctx.env, &ctx.compliance_id);
    let treasury = TreasuryContractClient::new(&ctx.env, &ctx.treasury_id);
    let token = TestTokenClient::new(&ctx.env, &ctx.token_id);

    // Allow then block the merchant
    compliance_admin.allow_address(&ctx.admin, &ctx.merchant);
    compliance_admin.block_address(&ctx.admin, &ctx.merchant, &None);
    assert!(!compliance.is_allowed(&ctx.merchant));

    let settlement_id = treasury.propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000, &0_u64);

    token.mint(&ctx.treasury_id, &10_000_000);

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
    assert_eq!(token.balance(&ctx.merchant), 0);
}

/// Compliance passes when merchant is allowed with a temp allow.
#[test]
fn settlement_proceeds_with_temp_allow_via_compliance_client() {
    let ctx = setup();

    let compliance = ComplianceClient::new(&ctx.env, &ctx.compliance_id);
    let compliance_admin = ComplianceContractClient::new(&ctx.env, &ctx.compliance_id);
    let treasury = TreasuryContractClient::new(&ctx.env, &ctx.treasury_id);
    let token = TestTokenClient::new(&ctx.env, &ctx.token_id);

    let now = ctx.env.ledger().timestamp();
    compliance_admin.allow_address_until(&ctx.admin, &ctx.merchant, &(now + 1000));
    assert!(compliance.is_allowed(&ctx.merchant));

    let settlement_id = treasury.propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000, &0_u64);

    token.mint(&ctx.treasury_id, &10_000_000);

    let wf = ComplianceGatedSettlementClient::new(&ctx.env, &ctx.workflow_id);

    wf.execute(
        &ctx.compliance_id,
        &ctx.treasury_id,
        &settlement_id,
        &ctx.token_id,
        &ctx.merchant,
    );

    assert_eq!(token.balance(&ctx.merchant), 10_000_000);
    let settlement = treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Executed);
}

/// Compliance fails when temp allow has expired.
#[test]
fn settlement_rejected_when_temp_allow_expired_via_compliance_client() {
    let ctx = setup();

    let compliance = ComplianceClient::new(&ctx.env, &ctx.compliance_id);
    let compliance_admin = ComplianceContractClient::new(&ctx.env, &ctx.compliance_id);
    let treasury = TreasuryContractClient::new(&ctx.env, &ctx.treasury_id);
    let token = TestTokenClient::new(&ctx.env, &ctx.token_id);

    let now = ctx.env.ledger().timestamp();
    // Set temp allow that expired in the past
    compliance_admin.allow_address_until(&ctx.admin, &ctx.merchant, &now);
    assert!(!compliance.is_allowed(&ctx.merchant));

    let settlement_id = treasury.propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000, &0_u64);

    token.mint(&ctx.treasury_id, &10_000_000);

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
    assert_eq!(token.balance(&ctx.merchant), 0);
}

/// check_compliance standalone test via compliance-client.
#[test]
fn check_compliance_returns_ok_for_allowed_merchant_via_compliance_client() {
    let ctx = setup();

    let compliance_admin = ComplianceContractClient::new(&ctx.env, &ctx.compliance_id);
    compliance_admin.allow_address(&ctx.admin, &ctx.merchant);

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

// ─── Cross-contract invocation mock tests (oracle-adapter) ───────────────────
//
// These tests exercise the oracle-adapter's cross-contract surface using the
// Soroban SDK testutils. A mock oracle contract is registered, and a consumer
// contract invokes the adapter cross-contract. Return values, auth
// requirements, and failure paths (unauthorized caller, missing data, zero
// balance) are asserted without panicking the test harness.

/// Mock oracle contract: stores a price per asset and exposes a `price` getter.
#[contract]
struct MockOracle;

#[contractimpl]
impl MockOracle {
    pub fn set_price(env: Env, admin: Address, asset: Address, price: i128) {
        admin.require_auth();
        env.storage().persistent().set(&("price", asset), &price);
    }

    pub fn price(env: Env, asset: Address) -> Option<i128> {
        env.storage().persistent().get(&("price", asset))
    }
}

/// Mock oracle-adapter: reads from the oracle and normalizes the price.
/// Exposes a `quote` method that returns `Option<i128>` so callers can
/// distinguish "missing data" from a real price without panicking.
#[contract]
struct MockOracleAdapter;

#[contractimpl]
impl MockOracleAdapter {
    pub fn quote(env: Env, oracle_id: Address, asset: Address) -> Option<i128> {
        let oracle = MockOracleClient::new(&env, &oracle_id);
        oracle.price(&asset)
    }

    /// Requires the caller to be authorized and returns the quoted value.
    pub fn quote_authorized(
        env: Env,
        caller: Address,
        oracle_id: Address,
        asset: Address,
    ) -> Option<i128> {
        caller.require_auth();
        let oracle = MockOracleClient::new(&env, &oracle_id);
        oracle.price(&asset)
    }
}

/// Consumer contract that invokes the adapter cross-contract.
#[contract]
struct OracleConsumer;

#[contractimpl]
impl OracleConsumer {
    pub fn read_quote(
        env: Env,
        adapter_id: Address,
        oracle_id: Address,
        asset: Address,
    ) -> Option<i128> {
        let adapter = MockOracleAdapterClient::new(&env, &adapter_id);
        adapter.quote(&oracle_id, &asset)
    }

    pub fn read_quote_authorized(
        env: Env,
        caller: Address,
        adapter_id: Address,
        oracle_id: Address,
        asset: Address,
    ) -> Option<i128> {
        let adapter = MockOracleAdapterClient::new(&env, &adapter_id);
        adapter.quote_authorized(&caller, &oracle_id, &asset)
    }
}

struct OracleCtx {
    env: Env,
    admin: Address,
    asset: Address,
    oracle_id: Address,
    adapter_id: Address,
    consumer_id: Address,
}

fn setup_oracle() -> OracleCtx {
    let env = Env::default();
    env.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&env);
    let asset = Address::generate(&env);

    let oracle_id = env.register_contract(None, MockOracle);
    let adapter_id = env.register_contract(None, MockOracleAdapter);
    let consumer_id = env.register_contract(None, OracleConsumer);

    OracleCtx {
        env,
        admin,
        asset,
        oracle_id,
        adapter_id,
        consumer_id,
    }
}

/// Cross-contract invocation returns the oracle price through the adapter.
#[test]
fn cross_contract_quote_returns_price() {
    let ctx = setup_oracle();

    let oracle = MockOracleClient::new(&ctx.env, &ctx.oracle_id);
    oracle.set_price(&ctx.admin, &ctx.asset, &42_000_000);

    let consumer = OracleConsumerClient::new(&ctx.env, &ctx.consumer_id);
    let quoted = consumer.read_quote(&ctx.adapter_id, &ctx.oracle_id, &ctx.asset);
    assert_eq!(quoted, Some(42_000_000));
}

/// Missing data path: adapter returns `None` rather than panicking.
#[test]
fn cross_contract_quote_missing_data_returns_none() {
    let ctx = setup_oracle();

    let consumer = OracleConsumerClient::new(&ctx.env, &ctx.consumer_id);
    let quoted = consumer.read_quote(&ctx.adapter_id, &ctx.oracle_id, &ctx.asset);
    assert_eq!(quoted, None);
}

/// Zero balance path: adapter returns `Some(0)` for a zero price.
#[test]
fn cross_contract_quote_zero_balance_returns_zero() {
    let ctx = setup_oracle();

    let oracle = MockOracleClient::new(&ctx.env, &ctx.oracle_id);
    oracle.set_price(&ctx.admin, &ctx.asset, &0);

    let consumer = OracleConsumerClient::new(&ctx.env, &ctx.consumer_id);
    let quoted = consumer.read_quote(&ctx.adapter_id, &ctx.oracle_id, &ctx.asset);
    assert_eq!(quoted, Some(0));
}

/// Authorized path: caller with auth succeeds.
#[test]
fn cross_contract_quote_authorized_succeeds() {
    let ctx = setup_oracle();

    let oracle = MockOracleClient::new(&ctx.env, &ctx.oracle_id);
    oracle.set_price(&ctx.admin, &ctx.asset, &7_500_000);

    let consumer = OracleConsumerClient::new(&ctx.env, &ctx.consumer_id);
    let quoted =
        consumer.read_quote_authorized(&ctx.admin, &ctx.adapter_id, &ctx.oracle_id, &ctx.asset);
    assert_eq!(quoted, Some(7_500_000));
}

/// Unauthorized caller: without auth, the adapter rejects the call.
#[test]
fn cross_contract_quote_authorized_rejects_unauthorized_caller() {
    let ctx = setup_oracle();

    // Disable blanket auth mocking so the missing auth is observable.
    ctx.env.set_auths(&[]);

    let oracle = MockOracleClient::new(&ctx.env, &ctx.oracle_id);
    // Admin auth is mocked for the setup call only.
    ctx.env.mock_auths(&[MockAuth {
        address: &ctx.admin,
        invoke: &MockAuthInvoke {
            contract: &ctx.oracle_id,
            fn_name: "set_price",
            args: (ctx.admin.clone(), ctx.asset.clone(), 1_000_i128).into_val(&ctx.env),
            sub_invokes: &[],
        },
    }]);
    oracle.set_price(&ctx.admin, &ctx.asset, &1_000);

    // Clear auths again so the consumer call has no authorization.
    ctx.env.set_auths(&[]);

    let consumer = OracleConsumerClient::new(&ctx.env, &ctx.consumer_id);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        consumer.read_quote_authorized(&ctx.admin, &ctx.adapter_id, &ctx.oracle_id, &ctx.asset);
    }));
    assert!(
        result.is_err(),
        "expected unauthorized caller to be rejected"
    );
}
