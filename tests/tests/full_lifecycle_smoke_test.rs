use compliance::{ComplianceContract, ComplianceContractClient};
use invoice::{InvoiceContract, InvoiceContractClient, InvoiceStatus, MaybeAddress, MaybeBytes};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

// Baseline instruction budget recorded on soroban-sdk 22.0.11 (native/test host).
// These are conservative upper-bound guards — a 20 % headroom above the measured
// baseline keeps CI green while still catching large regressions.
// Re-run `cargo test -- --nocapture full_lifecycle_budget_baseline` to refresh.
//
// Measured on 2026-07-27 (soroban-sdk 22.0.11, Rust 1.95.0, native host):
//   CPU instructions : 261_833
//   Memory bytes     :  46_107
const BUDGET_CPU_BASELINE: u64 = 314_200; // 261_833 × 1.20, rounded up
const BUDGET_MEM_BASELINE: u64 = 55_329; // 46_107 × 1.20, rounded up

mod test_token {
    use soroban_sdk::{contract, contractimpl, Address, Env};

    #[contract]
    pub struct TestToken;

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
}

use test_token::{TestToken, TestTokenClient};

#[contract]
struct ComplianceGatedSettlement;

#[contractimpl]
impl ComplianceGatedSettlement {
    pub fn execute(
        env: Env,
        compliance_id: Address,
        treasury_id: Address,
        settlement_id: u64,
        token_id: Address,
        merchant: Address,
    ) {
        let compliance = ComplianceContractClient::new(&env, &compliance_id);
        if !compliance.is_allowed(&merchant) {
            panic!("ComplianceFailed");
        }
        let treasury = TreasuryContractClient::new(&env, &treasury_id);
        treasury.execute_settlement(&env.current_contract_address(), &settlement_id, &token_id);
    }
}

struct TestContext {
    _env: Env,
    admin: Address,
    merchant: Address,
    payer: Address,
    invoice: InvoiceContractClient<'static>,
    _invoice_id: Address,
    treasury: TreasuryContractClient<'static>,
    treasury_id: Address,
    compliance: ComplianceContractClient<'static>,
    compliance_id: Address,
    token: TestTokenClient<'static>,
    token_id: Address,
}

fn setup() -> TestContext {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);

    let invoice_id = env.register_contract(None, InvoiceContract);
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    invoice.initialize(&admin);

    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));
    treasury.set_signer(&admin, &admin, &2);

    let compliance_id = env.register_contract(None, ComplianceContract);
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&admin);

    let token_id = env.register_contract(None, TestToken);
    let token = TestTokenClient::new(&env, &token_id);

    TestContext {
        _env: env,
        admin,
        merchant,
        payer,
        invoice,
        _invoice_id: invoice_id,
        treasury,
        treasury_id,
        compliance,
        compliance_id,
        token,
        token_id,
    }
}

#[test]
fn full_lifecycle_happy_path() {
    let ctx = setup();

    ctx.compliance.allow_address(&ctx.admin, &ctx.merchant);
    assert!(ctx.compliance.is_allowed(&ctx.merchant));

    let inv_id = ctx.invoice.create_invoice(
        &ctx.merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    let inv = ctx.invoice.get_invoice(&inv_id);
    assert_eq!(inv.status, InvoiceStatus::Pending);

    ctx.invoice.mark_paid(
        &ctx.admin,
        &inv_id,
        &ctx.payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
    let inv = ctx.invoice.get_invoice(&inv_id);
    assert_eq!(inv.status, InvoiceStatus::Paid);
    assert_eq!(inv.payer, MaybeAddress::Some(ctx.payer.clone()));

    ctx.invoice.release_escrow(&ctx.admin, &inv_id);
    let inv = ctx.invoice.get_invoice(&inv_id);
    assert_eq!(inv.status, invoice::InvoiceStatus::Released);

    ctx.token.mint(&ctx.treasury_id, &10_000_000);
    assert_eq!(ctx.token.balance(&ctx.treasury_id), 10_000_000);
    assert_eq!(ctx.token.balance(&ctx.merchant), 0);

    let settlement_id = ctx
        .treasury
        .propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000);
    let settlement = ctx.treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Pending);

    // admin is both proposer and approver here, so approval_weight (2) is not
    // double-counted — see #34 duplicate-approval-weight guarantee.
    ctx.treasury.approve_settlement(&ctx.admin, &settlement_id);
    let settlement = ctx.treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.approval_weight, 2);

    let wf_id = ctx._env.register_contract(None, ComplianceGatedSettlement);
    let wf = ComplianceGatedSettlementClient::new(&ctx._env, &wf_id);
    ctx.treasury.set_signer(&ctx.admin, &wf_id, &1);

    wf.execute(
        &ctx.compliance_id,
        &ctx.treasury_id,
        &settlement_id,
        &ctx.token_id,
        &ctx.merchant,
    );

    let settlement = ctx.treasury.get_settlement(&settlement_id);
    assert_eq!(settlement.status, SettlementStatus::Executed);
    assert_eq!(ctx.token.balance(&ctx.treasury_id), 0);
    assert_eq!(ctx.token.balance(&ctx.merchant), 10_000_000);
}

#[test]
fn full_lifecycle_rejected_when_compliance_fails() {
    let ctx = setup();

    let inv_id = ctx.invoice.create_invoice(
        &ctx.merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    ctx.invoice.mark_paid(
        &ctx.admin,
        &inv_id,
        &ctx.payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
    ctx.invoice.release_escrow(&ctx.admin, &inv_id);

    ctx.token.mint(&ctx.treasury_id, &10_000_000);

    let settlement_id = ctx
        .treasury
        .propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000);
    ctx.treasury.approve_settlement(&ctx.admin, &settlement_id);

    let wf_id = ctx._env.register_contract(None, ComplianceGatedSettlement);
    let wf = ComplianceGatedSettlementClient::new(&ctx._env, &wf_id);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        wf.execute(
            &ctx.compliance_id,
            &ctx.treasury_id,
            &settlement_id,
            &ctx.token_id,
            &ctx.merchant,
        );
    }));
    assert!(result.is_err());
    assert_eq!(ctx.token.balance(&ctx.merchant), 0);
}

/// Measures and asserts the instruction / memory budget consumed by the full
/// happy-path lifecycle (Invoice → Treasury → Compliance → Token transfer).
///
/// The constants [`BUDGET_CPU_BASELINE`] and [`BUDGET_MEM_BASELINE`] encode
/// a 20 % headroom above the measured baseline on soroban-sdk 22.0.11
/// running with the native (non-WASM) test host.
///
/// **Important:** native-host numbers are lower than on-chain WASM execution.
/// They are still valuable as regression guards — a large increase here
/// almost always reflects a real cost increase on-chain.
///
/// To refresh the baseline after intentional changes:
/// ```sh
/// cargo test -- --nocapture full_lifecycle_budget_baseline
/// ```
/// and update the constants at the top of this file.
#[test]
fn full_lifecycle_budget_baseline() {
    let ctx = setup();

    // Reset to unlimited limits so the lifecycle is not artificially capped,
    // then zero the counters so we measure only this test's work.
    ctx._env.cost_estimate().budget().reset_unlimited();
    ctx._env.cost_estimate().budget().reset_tracker();

    // ── Full happy-path lifecycle ──────────────────────────────────────────
    ctx.compliance.allow_address(&ctx.admin, &ctx.merchant);

    let inv_id = ctx.invoice.create_invoice(
        &ctx.merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    ctx.invoice.mark_paid(
        &ctx.admin,
        &inv_id,
        &ctx.payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
    ctx.invoice.release_escrow(&ctx.admin, &inv_id);

    ctx.token.mint(&ctx.treasury_id, &10_000_000);

    let settlement_id = ctx
        .treasury
        .propose_settlement(&ctx.admin, &ctx.merchant, &10_000_000);
    ctx.treasury.approve_settlement(&ctx.admin, &settlement_id);

    let wf_id = ctx._env.register_contract(None, ComplianceGatedSettlement);
    let wf = ComplianceGatedSettlementClient::new(&ctx._env, &wf_id);
    ctx.treasury.set_signer(&ctx.admin, &wf_id, &1);

    wf.execute(
        &ctx.compliance_id,
        &ctx.treasury_id,
        &settlement_id,
        &ctx.token_id,
        &ctx.merchant,
    );
    // ── End of lifecycle ───────────────────────────────────────────────────

    let cpu = ctx._env.cost_estimate().budget().cpu_instruction_cost();
    let mem = ctx._env.cost_estimate().budget().memory_bytes_cost();

    // Useful when refreshing the baseline — run with `-- --nocapture`.
    println!("=== full-lifecycle budget (native host) ===");
    println!("  CPU instructions : {cpu}");
    println!("  Memory bytes     : {mem}");

    // Guard against regressions.  The baselines include 20 % headroom above
    // the values measured when this test was first introduced.
    assert!(
        cpu <= BUDGET_CPU_BASELINE,
        "CPU budget regression: {cpu} instructions exceeds baseline {BUDGET_CPU_BASELINE}. \
         If this increase is intentional, update BUDGET_CPU_BASELINE in this file and \
         ARCHITECTURE.md."
    );
    assert!(
        mem <= BUDGET_MEM_BASELINE,
        "Memory budget regression: {mem} bytes exceeds baseline {BUDGET_MEM_BASELINE}. \
         If this increase is intentional, update BUDGET_MEM_BASELINE in this file and \
         ARCHITECTURE.md."
    );
}
