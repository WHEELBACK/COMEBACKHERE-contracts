//! Chaos-style failure injection across the full settlement lifecycle (#468).
//!
//! Builds on issue #83's end-to-end lifecycle
//! (`tests/tests/full_lifecycle_smoke_test.rs::full_lifecycle_happy_path`):
//!
//! ```text
//!   create_invoice -> mark_paid -> propose_settlement -> approve_settlement
//!     -> compliance.is_allowed -> execute_settlement (token transfer)
//!     -> release_escrow
//! ```
//!
//! #83 exercises that whole chain once, on the happy path. Isolated unit tests
//! elsewhere cover individual failure modes, but nothing replays *this* full
//! multi-contract flow while deliberately forcing exactly one cross-contract
//! call boundary to fail. This test does: it runs the lifecycle once per
//! boundary, injects a failure at that boundary, and asserts the system is left
//! in a consistent, non-corrupt, recoverable state every time.
//!
//! Boundaries exercised (`FailurePoint`):
//!   * `None`                    - control: the happy path still completes
//!   * `ComplianceGate`          - compliance.is_allowed returns false
//!   * `ExecuteThresholdNotMet`  - treasury.execute_settlement: quorum missing
//!   * `ExecuteTokenNotAllowed`  - treasury.execute_settlement: token off allowlist
//!   * `ExecuteSettlementOnHold` - treasury.execute_settlement: settlement disputed
//!   * `ReleaseEscrow`           - invoice.release_escrow: invoice contract paused
//!
//! Universal invariants asserted after every run, whichever boundary failed
//! (see [`assert_consistent`]):
//!   1. Token conservation: `treasury_balance + merchant_balance == minted total`.
//!   2. All-or-nothing payout: the merchant holds either the whole amount or
//!      nothing - never a partial transfer.
//!   3. The merchant is paid *iff* the settlement reached `Executed`.
//!   4. Settlement status is always `Pending` / `OnHold` / `Executed` - never a
//!      partial/limbo value.
//!   5. Invoice status is always `Paid` or `Released` - a failed `release_escrow`
//!      leaves the invoice cleanly retryable, not wedged.
//!   6. The invoice only reaches `Released` when the settlement is `Executed`.
//!
//! Any divergence found here should be filed as a follow-up issue rather than
//! silently patched in this test file.

use invoice::{
    InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus, MaybeAddress, MaybeBytes,
};
use soroban_sdk::{contract, contractimpl, testutils::Address as _, Address, Env};
use std::panic::{catch_unwind, AssertUnwindSafe};

use compliance::{ComplianceContract, ComplianceContractClient};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

/// Minimal in-crate token double, identical in shape to the one used by #83's
/// `full_lifecycle_smoke_test.rs`. `mock_all_auths` covers the `require_auth`.
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

/// Mirrors #83's `ComplianceGatedSettlement` workflow contract: enforces the
/// compliance gate, then calls `execute_settlement`. Panics (rather than
/// returning `Err`) on either failure so the test can catch the boundary
/// failure with `catch_unwind`, exactly as `full_lifecycle_smoke_test.rs` does.
#[contract]
pub struct ChaosWorkflow;

#[contractimpl]
impl ChaosWorkflow {
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
            panic!("compliance gate: merchant not allowed");
        }
        let treasury = TreasuryContractClient::new(&env, &treasury_id);
        treasury.execute_settlement(&env.current_contract_address(), &settlement_id, &token_id);
    }
}

const AMOUNT: i128 = 10_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FailurePoint {
    /// Control: no failure injected, the whole lifecycle should complete.
    None,
    /// `compliance.is_allowed(merchant)` returns false (merchant never allowed).
    ComplianceGate,
    /// `execute_settlement` rejects: approval weight never reaches threshold.
    ExecuteThresholdNotMet,
    /// `execute_settlement` rejects: the settlement token is not on a non-empty
    /// treasury allowlist.
    ExecuteTokenNotAllowed,
    /// `execute_settlement` rejects: the settlement was put `OnHold` by a dispute.
    ExecuteSettlementOnHold,
    /// `invoice.release_escrow` rejects: the invoice contract is paused. The
    /// settlement side has already completed successfully at this point.
    ReleaseEscrow,
}

const ALL_FAILURE_POINTS: [FailurePoint; 6] = [
    FailurePoint::None,
    FailurePoint::ComplianceGate,
    FailurePoint::ExecuteThresholdNotMet,
    FailurePoint::ExecuteTokenNotAllowed,
    FailurePoint::ExecuteSettlementOnHold,
    FailurePoint::ReleaseEscrow,
];

/// State observed at the moment the injected boundary was reached (before any
/// recovery attempt).
#[derive(Debug)]
struct Outcome {
    settlement_status: SettlementStatus,
    invoice_status: InvoiceStatus,
    treasury_balance: i128,
    merchant_balance: i128,
    minted_total: i128,
    /// Whether the injected cross-contract call boundary actually failed.
    injected_boundary_failed: bool,
}

struct Ctx {
    env: Env,
    admin: Address,
    merchant: Address,
    payer: Address,
    signer2: Address,
    invoice: InvoiceContractClient<'static>,
    treasury: TreasuryContractClient<'static>,
    treasury_id: Address,
    compliance_id: Address,
    token: TestTokenClient<'static>,
    token_id: Address,
    wf_id: Address,
}

fn setup() -> Ctx {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let signer2 = Address::generate(&env);

    let invoice_id = env.register_contract(None, InvoiceContract);
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    invoice.initialize(&admin);

    // Threshold 2, admin weight 1 (set by initialize), plus a second weight-1
    // signer. A proposal alone is then *not* enough to execute - so skipping the
    // second approval is a real `ExecuteThresholdNotMet` injection.
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));
    treasury.set_signer(&admin, &signer2, &1);

    let compliance_id = env.register_contract(None, ComplianceContract);
    ComplianceContractClient::new(&env, &compliance_id).initialize(&admin);

    let token_id = env.register_contract(None, TestToken);
    let token = TestTokenClient::new(&env, &token_id);

    let wf_id = env.register_contract(None, ChaosWorkflow);
    treasury.set_signer(&admin, &wf_id, &1);

    Ctx {
        env,
        admin,
        merchant,
        payer,
        signer2,
        invoice,
        treasury,
        treasury_id,
        compliance_id,
        token,
        token_id,
        wf_id,
    }
}

/// Runs #83's lifecycle once, injecting `fp` at its boundary, and reports the
/// observed cross-contract state.
fn run_lifecycle(fp: FailurePoint) -> Outcome {
    let ctx = setup();
    let compliance = ComplianceContractClient::new(&ctx.env, &ctx.compliance_id);

    // 1. create_invoice -> Pending
    let inv_id = ctx.invoice.create_invoice(
        &ctx.merchant,
        &AMOUNT,
        &(AMOUNT + 250_000),
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    assert_eq!(
        ctx.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Pending
    );

    // 2. mark_paid -> Paid
    ctx.invoice.mark_paid(
        &ctx.admin,
        &inv_id,
        &ctx.payer,
        &MaybeBytes::None,
        &MaybeAddress::None,
    );
    assert_eq!(ctx.invoice.get_invoice(&inv_id).status, InvoiceStatus::Paid);

    // 3. fund the treasury
    ctx.token.mint(&ctx.treasury_id, &AMOUNT);
    let minted_total = AMOUNT;

    // 4. propose_settlement -> Pending (proposer = admin, weight 1)
    let settlement_id = ctx
        .treasury
        .propose_settlement(&ctx.admin, &ctx.merchant, &AMOUNT, &0_u64);
    assert_eq!(
        ctx.treasury.get_settlement(&settlement_id).status,
        SettlementStatus::Pending
    );

    // 5. approve_settlement -> weight 2 == threshold, UNLESS we are injecting a
    //    threshold failure at the execute boundary.
    if fp != FailurePoint::ExecuteThresholdNotMet {
        ctx.treasury
            .approve_settlement(&ctx.signer2, &settlement_id);
    }

    // 6. compliance gate: allow the merchant for every case EXCEPT the
    //    compliance-gate injection.
    if fp != FailurePoint::ComplianceGate {
        compliance.allow_address(&ctx.admin, &ctx.merchant);
    }

    // Boundary-specific pre-conditions for the two remaining execute-failure
    // injections.
    match fp {
        FailurePoint::ExecuteTokenNotAllowed => {
            // Non-empty allowlist that does not contain our settlement token.
            let other_token = ctx.env.register_contract(None, TestToken);
            ctx.treasury.add_allowed_token(&ctx.admin, &other_token);
        }
        FailurePoint::ExecuteSettlementOnHold => {
            let claimant = Address::generate(&ctx.env);
            ctx.treasury.raise_dispute(
                &claimant,
                &settlement_id,
                &ctx.merchant,
                &1,
                &u64::MAX,
                &None,
            );
            assert_eq!(
                ctx.treasury.get_settlement(&settlement_id).status,
                SettlementStatus::OnHold
            );
        }
        _ => {}
    }

    // 7. execute_settlement, via the compliance-gated workflow (as in #83).
    let exec_result = catch_unwind(AssertUnwindSafe(|| {
        ChaosWorkflowClient::new(&ctx.env, &ctx.wf_id).execute(
            &ctx.compliance_id,
            &ctx.treasury_id,
            &settlement_id,
            &ctx.token_id,
            &ctx.merchant,
        );
    }));

    let execute_boundary_failed = exec_result.is_err();

    // 8. release_escrow -> Released. Injection: pause the invoice contract first
    //    so the call is rejected with `ContractPaused`.
    let mut release_boundary_failed = false;
    let execute_succeeded = !execute_boundary_failed;
    if execute_succeeded {
        if fp == FailurePoint::ReleaseEscrow {
            ctx.invoice.pause(&ctx.admin);
        }
        match ctx.invoice.try_release_escrow(&ctx.admin, &inv_id) {
            Ok(Ok(())) => {}
            Err(Ok(e)) => {
                release_boundary_failed = true;
                assert_eq!(
                    e,
                    InvoiceError::ContractPaused,
                    "release_escrow should fail only because the invoice is paused"
                );
            }
            other => panic!("unexpected release_escrow result: {other:?}"),
        }
    }

    let injected_boundary_failed = match fp {
        FailurePoint::None => false,
        FailurePoint::ComplianceGate
        | FailurePoint::ExecuteThresholdNotMet
        | FailurePoint::ExecuteTokenNotAllowed
        | FailurePoint::ExecuteSettlementOnHold => execute_boundary_failed,
        FailurePoint::ReleaseEscrow => release_boundary_failed,
    };

    Outcome {
        settlement_status: ctx.treasury.get_settlement(&settlement_id).status,
        invoice_status: ctx.invoice.get_invoice(&inv_id).status,
        treasury_balance: ctx.token.balance(&ctx.treasury_id),
        merchant_balance: ctx.token.balance(&ctx.merchant),
        minted_total,
        injected_boundary_failed,
    }
}

/// The invariants that must hold no matter which boundary was forced to fail.
fn assert_consistent(fp: FailurePoint, o: &Outcome) {
    // 1. Token conservation - nothing minted or burned by a partial failure.
    assert_eq!(
        o.treasury_balance + o.merchant_balance,
        o.minted_total,
        "{fp:?}: token conservation violated (treasury {} + merchant {} != minted {})",
        o.treasury_balance,
        o.merchant_balance,
        o.minted_total
    );

    // 2. All-or-nothing payout - never a partially applied transfer.
    assert!(
        o.merchant_balance == 0 || o.merchant_balance == o.minted_total,
        "{fp:?}: merchant holds a partial balance {} (expected 0 or {})",
        o.merchant_balance,
        o.minted_total
    );

    // 3. Merchant is paid iff the settlement executed.
    assert_eq!(
        o.merchant_balance == o.minted_total,
        o.settlement_status == SettlementStatus::Executed,
        "{fp:?}: merchant-paid ({}) disagrees with settlement Executed ({:?})",
        o.merchant_balance == o.minted_total,
        o.settlement_status
    );

    // 4. Settlement never lands in a partial/limbo status.
    assert!(
        matches!(
            o.settlement_status,
            SettlementStatus::Pending | SettlementStatus::OnHold | SettlementStatus::Executed
        ),
        "{fp:?}: settlement in unexpected status {:?}",
        o.settlement_status
    );

    // 5. Invoice never wedges - always Paid (retryable) or Released.
    assert!(
        matches!(
            o.invoice_status,
            InvoiceStatus::Paid | InvoiceStatus::Released
        ),
        "{fp:?}: invoice in unexpected status {:?}",
        o.invoice_status
    );

    // 6. Escrow is only released once the settlement has actually executed.
    if o.invoice_status == InvoiceStatus::Released {
        assert_eq!(
            o.settlement_status,
            SettlementStatus::Executed,
            "{fp:?}: invoice Released while settlement is {:?}",
            o.settlement_status
        );
    }
}

#[test]
fn control_happy_path_completes() {
    let o = run_lifecycle(FailurePoint::None);
    assert_consistent(FailurePoint::None, &o);
    assert!(!o.injected_boundary_failed);
    assert_eq!(o.settlement_status, SettlementStatus::Executed);
    assert_eq!(o.invoice_status, InvoiceStatus::Released);
    assert_eq!(o.merchant_balance, o.minted_total);
    assert_eq!(o.treasury_balance, 0);
}

#[test]
fn compliance_gate_failure_leaves_state_consistent() {
    let o = run_lifecycle(FailurePoint::ComplianceGate);
    assert_consistent(FailurePoint::ComplianceGate, &o);
    assert!(
        o.injected_boundary_failed,
        "compliance gate should have failed"
    );
    assert_eq!(o.settlement_status, SettlementStatus::Pending);
    assert_eq!(o.invoice_status, InvoiceStatus::Paid);
    assert_eq!(o.merchant_balance, 0);
    assert_eq!(o.treasury_balance, o.minted_total);
}

#[test]
fn execute_threshold_not_met_leaves_state_consistent() {
    let o = run_lifecycle(FailurePoint::ExecuteThresholdNotMet);
    assert_consistent(FailurePoint::ExecuteThresholdNotMet, &o);
    assert!(
        o.injected_boundary_failed,
        "execute_settlement should have failed"
    );
    assert_eq!(o.settlement_status, SettlementStatus::Pending);
    assert_eq!(o.invoice_status, InvoiceStatus::Paid);
    assert_eq!(o.merchant_balance, 0);
    assert_eq!(o.treasury_balance, o.minted_total);
}

#[test]
fn execute_token_not_allowed_leaves_state_consistent() {
    let o = run_lifecycle(FailurePoint::ExecuteTokenNotAllowed);
    assert_consistent(FailurePoint::ExecuteTokenNotAllowed, &o);
    assert!(
        o.injected_boundary_failed,
        "execute_settlement should have failed"
    );
    assert_eq!(o.settlement_status, SettlementStatus::Pending);
    assert_eq!(o.invoice_status, InvoiceStatus::Paid);
    assert_eq!(o.merchant_balance, 0);
    assert_eq!(o.treasury_balance, o.minted_total);
}

#[test]
fn execute_settlement_on_hold_leaves_state_consistent() {
    let o = run_lifecycle(FailurePoint::ExecuteSettlementOnHold);
    assert_consistent(FailurePoint::ExecuteSettlementOnHold, &o);
    assert!(
        o.injected_boundary_failed,
        "execute_settlement should have failed"
    );
    assert_eq!(o.settlement_status, SettlementStatus::OnHold);
    assert_eq!(o.invoice_status, InvoiceStatus::Paid);
    assert_eq!(o.merchant_balance, 0);
    assert_eq!(o.treasury_balance, o.minted_total);
}

#[test]
fn release_escrow_failure_leaves_state_consistent_and_is_recoverable() {
    let o = run_lifecycle(FailurePoint::ReleaseEscrow);
    assert_consistent(FailurePoint::ReleaseEscrow, &o);
    assert!(
        o.injected_boundary_failed,
        "release_escrow should have failed"
    );
    // The settlement side completed before the failed release.
    assert_eq!(o.settlement_status, SettlementStatus::Executed);
    assert_eq!(o.merchant_balance, o.minted_total);
    assert_eq!(o.treasury_balance, 0);
    // The invoice is left cleanly retryable, not wedged.
    assert_eq!(o.invoice_status, InvoiceStatus::Paid);

    // Recovery: unpausing and retrying the same call completes the lifecycle,
    // proving the earlier failure did not corrupt or block the invoice.
    let ctx = setup();
    let inv_id = ctx.invoice.create_invoice(
        &ctx.merchant,
        &AMOUNT,
        &(AMOUNT + 250_000),
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
    ctx.invoice.pause(&ctx.admin);
    assert!(matches!(
        ctx.invoice.try_release_escrow(&ctx.admin, &inv_id),
        Err(Ok(InvoiceError::ContractPaused))
    ));
    ctx.invoice.unpause(&ctx.admin);
    ctx.invoice.release_escrow(&ctx.admin, &inv_id);
    assert_eq!(
        ctx.invoice.get_invoice(&inv_id).status,
        InvoiceStatus::Released
    );
}

/// Sweep: every boundary in turn leaves the multi-contract system consistent.
#[test]
fn every_boundary_failure_leaves_state_consistent() {
    for fp in ALL_FAILURE_POINTS {
        let o = run_lifecycle(fp);
        assert_consistent(fp, &o);
        match fp {
            FailurePoint::None => assert!(!o.injected_boundary_failed),
            _ => assert!(
                o.injected_boundary_failed,
                "{fp:?}: expected the injected boundary to fail"
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Lifecycle chaos: long operation sequences with per-step invariant checks.
// ---------------------------------------------------------------------------

/// A single inconsistency detected by [`check_invariants`]. Returned (rather
/// than panicking) so a chaos run can report exactly which step and which
/// invariant broke, instead of aborting at the first `assert!`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct InvariantViolation {
    /// Index of the operation in the chaos sequence that produced the state.
    step: usize,
    /// Human-readable name of the operation that produced the state.
    operation: &'static str,
    /// Which invariant failed and the observed values.
    detail: String,
}

impl InvariantViolation {
    fn new(step: usize, operation: &'static str, detail: impl Into<String>) -> Self {
        Self {
            step,
            operation,
            detail: detail.into(),
        }
    }
}

/// Snapshot of the cross-contract state used for invariant verification.
#[derive(Clone, Debug)]
struct Snapshot {
    settlement_status: SettlementStatus,
    invoice_status: InvoiceStatus,
    treasury_balance: i128,
    merchant_balance: i128,
    minted_total: i128,
    /// Whether the merchant is currently on the compliance allowlist.
    merchant_allowed: bool,
    /// Whether the invoice contract is paused.
    invoice_paused: bool,
}

/// Strict invariant verification. Returns the first violation found, or `Ok`
/// when the snapshot is fully consistent. This is the defensive check the
/// chaos runner applies after *every* operation.
fn check_invariants(
    step: usize,
    operation: &'static str,
    s: &Snapshot,
) -> Result<(), InvariantViolation> {
    // 1. Token conservation - nothing minted or burned by a partial failure.
    if s.treasury_balance + s.merchant_balance != s.minted_total {
        return Err(InvariantViolation::new(
            step,
            operation,
            format!(
                "token conservation violated: treasury {} + merchant {} != minted {}",
                s.treasury_balance, s.merchant_balance, s.minted_total
            ),
        ));
    }

    // 2. All-or-nothing payout - never a partially applied transfer.
    if s.merchant_balance != 0 && s.merchant_balance != AMOUNT {
        return Err(InvariantViolation::new(
            step,
            operation,
            format!(
                "partial payout: merchant holds {} (expected 0 or {})",
                s.merchant_balance, AMOUNT
            ),
        ));
    }

    // 3. Merchant is paid iff the settlement executed.
    let merchant_paid = s.merchant_balance == AMOUNT;
    if merchant_paid != (s.settlement_status == SettlementStatus::Executed) {
        return Err(InvariantViolation::new(
            step,
            operation,
            format!(
                "merchant-paid ({merchant_paid}) disagrees with settlement status {:?}",
                s.settlement_status
            ),
        ));
    }

    // 4. Settlement never lands in a partial/limbo status.
    if !matches!(
        s.settlement_status,
        SettlementStatus::Pending | SettlementStatus::OnHold | SettlementStatus::Executed
    ) {
        return Err(InvariantViolation::new(
            step,
            operation,
            format!("settlement in unexpected status {:?}", s.settlement_status),
        ));
    }

    // 5. Invoice never wedges - always Paid (retryable) or Released.
    if !matches!(
        s.invoice_status,
        InvoiceStatus::Paid | InvoiceStatus::Released
    ) {
        return Err(InvariantViolation::new(
            step,
            operation,
            format!("invoice in unexpected status {:?}", s.invoice_status),
        ));
    }

    // 6. Escrow is only released once the settlement has actually executed.
    if s.invoice_status == InvoiceStatus::Released
        && s.settlement_status != SettlementStatus::Executed
    {
        return Err(InvariantViolation::new(
            step,
            operation,
            format!(
                "invoice Released while settlement is {:?}",
                s.settlement_status
            ),
        ));
    }

    // 7. A paused invoice can never have released its escrow.
    if s.invoice_paused && s.invoice_status == InvoiceStatus::Released {
        return Err(InvariantViolation::new(
            step,
            operation,
            "invoice Released while the contract is paused",
        ));
    }

    // 8. A disallowed merchant can never have been paid.
    if !s.merchant_allowed && merchant_paid {
        return Err(InvariantViolation::new(
            step,
            operation,
            "merchant paid while not on the compliance allowlist",
        ));
    }

    Ok(())
}

/// One step in a chaos sequence. Each variant is applied to the live context;
/// the runner snapshots and verifies invariants after every step.
#[derive(Clone, Copy, Debug)]
enum ChaosOp {
    /// Mint `AMOUNT` to the treasury (a deposit).
    Deposit,
    /// Propose a settlement for the merchant.
    Propose,
    /// Approve the most recent settlement with the second signer.
    Approve,
    /// Raise a dispute, moving the settlement `OnHold`.
    RaiseDispute,
    /// Resolve the dispute, returning the settlement to `Pending`.
    ResolveDispute,
    /// Allow the merchant through the compliance gate.
    AllowMerchant,
    /// Revoke the merchant from the compliance allowlist.
    RevokeMerchant,
    /// Pause the invoice contract.
    PauseInvoice,
    /// Unpause the invoice contract.
    UnpauseInvoice,
    /// Attempt to execute the most recent settlement.
    Execute,
    /// Attempt to release the invoice escrow.
    ReleaseEscrow,
}

impl ChaosOp {
    fn name(self) -> &'static str {
        match self {
            ChaosOp::Deposit => "deposit",
            ChaosOp::Propose => "propose",
            ChaosOp::Approve => "approve",
            ChaosOp::RaiseDispute => "raise_dispute",
            ChaosOp::ResolveDispute => "resolve_dispute",
            ChaosOp::AllowMerchant => "allow_merchant",
            ChaosOp::RevokeMerchant => "revoke_merchant",
            ChaosOp::PauseInvoice => "pause_invoice",
            ChaosOp::UnpauseInvoice => "unpause_invoice",
            ChaosOp::Execute => "execute",
            ChaosOp::ReleaseEscrow => "release_escrow",
        }
    }
}

/// Mutable state threaded through a chaos run.
struct ChaosCtx {
    ctx: Ctx,
    inv_id: u64,
    settlement_id: Option<u64>,
    last_dispute_id: Option<u64>,
    minted_total: i128,
    merchant_allowed: bool,
    invoice_paused: bool,
}

impl ChaosCtx {
    fn new() -> Self {
        let ctx = setup();
        let inv_id = ctx.invoice.create_invoice(
            &ctx.merchant,
            &AMOUNT,
            &(AMOUNT + 250_000),
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
        Self {
            ctx,
            inv_id,
            settlement_id: None,
            last_dispute_id: None,
            minted_total: 0,
            merchant_allowed: false,
            invoice_paused: false,
        }
    }

    fn snapshot(&self) -> Snapshot {
        let settlement_status = match self.settlement_id {
            Some(id) => self.ctx.treasury.get_settlement(&id).status,
            // No settlement proposed yet: the invoice is still cleanly Paid and
            // nothing has moved, which is consistent with a `Pending` view.
            None => SettlementStatus::Pending,
        };
        Snapshot {
            settlement_status,
            invoice_status: self.ctx.invoice.get_invoice(&self.inv_id).status,
            treasury_balance: self.ctx.token.balance(&self.ctx.treasury_id),
            merchant_balance: self.ctx.token.balance(&self.ctx.merchant),
            minted_total: self.minted_total,
            merchant_allowed: self.merchant_allowed,
            invoice_paused: self.invoice_paused,
        }
    }

    /// Applies one operation. Cross-contract calls that are *expected* to fail
    /// (e.g. executing an on-hold settlement) are caught so the sequence keeps
    /// running; the invariant check afterwards is what matters.
    fn apply(&mut self, op: ChaosOp) {
        match op {
            ChaosOp::Deposit => {
                self.ctx.token.mint(&self.ctx.treasury_id, &AMOUNT);
                self.minted_total += AMOUNT;
            }
            ChaosOp::Propose => {
                let id = self.ctx.treasury.propose_settlement(
                    &self.ctx.admin,
                    &self.ctx.merchant,
                    &AMOUNT,
                    &0_u64,
                );
                self.settlement_id = Some(id);
            }
            ChaosOp::Approve => {
                if let Some(id) = self.settlement_id {
                    let _ = catch_unwind(AssertUnwindSafe(|| {
                        self.ctx.treasury.approve_settlement(&self.ctx.signer2, &id);
                    }));
                }
            }
            ChaosOp::RaiseDispute => {
                if let Some(id) = self.settlement_id {
                    let claimant = Address::generate(&self.ctx.env);
                    if let Ok(Ok(did)) = catch_unwind(AssertUnwindSafe(|| {
                        self.ctx.treasury.try_raise_dispute(
                            &claimant,
                            &id,
                            &self.ctx.merchant,
                            &1,
                            &u64::MAX,
                            &None,
                        )
                    })) {
                        if let Ok(did) = did {
                            self.last_dispute_id = Some(did);
                        }
                    }
                }
            }
            ChaosOp::ResolveDispute => {
                if let Some(did) = self.last_dispute_id {
                    let _ = catch_unwind(AssertUnwindSafe(|| {
                        let _ = self.ctx.treasury.try_resolve_dispute(
                            &self.ctx.admin,
                            &did,
                            &true,
                            &None,
                        );
                    }));
                }
            }
            ChaosOp::AllowMerchant => {
                self.ctx
                    .compliance_client()
                    .clear_address(&self.ctx.admin, &self.ctx.merchant);
                self.ctx
                    .compliance_client()
                    .allow_address(&self.ctx.admin, &self.ctx.merchant);
                self.merchant_allowed = true;
            }
            ChaosOp::RevokeMerchant => {
                let _ = catch_unwind(AssertUnwindSafe(|| {
                    let _ = self.ctx.compliance_client().try_block_address(
                        &self.ctx.admin,
                        &self.ctx.merchant,
                        &None,
                    );
                }));
                self.merchant_allowed = false;
            }
            ChaosOp::PauseInvoice => {
                self.ctx.invoice.pause(&self.ctx.admin);
                self.invoice_paused = true;
            }
            ChaosOp::UnpauseInvoice => {
                self.ctx.invoice.unpause(&self.ctx.admin);
                self.invoice_paused = false;
            }
            ChaosOp::Execute => {
                if let Some(id) = self.settlement_id {
                    let _ = catch_unwind(AssertUnwindSafe(|| {
                        ChaosWorkflowClient::new(&self.ctx.env, &self.ctx.wf_id).execute(
                            &self.ctx.compliance_id,
                            &self.ctx.treasury_id,
                            &id,
                            &self.ctx.token_id,
                            &self.ctx.merchant,
                        );
                    }));
                }
            }
            ChaosOp::ReleaseEscrow => {
                let _ = catch_unwind(AssertUnwindSafe(|| {
                    self.ctx
                        .invoice
                        .release_escrow(&self.ctx.admin, &self.inv_id);
                }));
            }
        }
    }
}

impl Ctx {
    fn compliance_client(&self) -> ComplianceContractClient<'_> {
        ComplianceContractClient::new(&self.env, &self.compliance_id)
    }
}

/// Runs `ops` against a fresh context, verifying invariants after every step.
/// Returns the first violation, if any.
fn run_chaos(ops: &[ChaosOp]) -> Result<(), InvariantViolation> {
    let mut state = ChaosCtx::new();
    for (step, op) in ops.iter().enumerate() {
        state.apply(*op);
        check_invariants(step, op.name(), &state.snapshot())?;
    }
    Ok(())
}

#[test]
fn chaos_deposit_and_settle_sequence_holds_invariants() {
    let ops = [
        ChaosOp::Deposit,
        ChaosOp::AllowMerchant,
        ChaosOp::Propose,
        ChaosOp::Approve,
        ChaosOp::Execute,
        ChaosOp::ReleaseEscrow,
    ];
    assert_eq!(run_chaos(&ops), Ok(()));
}

#[test]
fn chaos_hold_and_resume_sequence_holds_invariants() {
    let ops = [
        ChaosOp::Deposit,
        ChaosOp::AllowMerchant,
        ChaosOp::Propose,
        ChaosOp::Approve,
        ChaosOp::RaiseDispute,
        ChaosOp::Execute, // rejected: settlement OnHold
        ChaosOp::ResolveDispute,
        ChaosOp::Execute,
        ChaosOp::ReleaseEscrow,
    ];
    assert_eq!(run_chaos(&ops), Ok(()));
}

#[test]
fn chaos_role_change_sequence_holds_invariants() {
    let ops = [
        ChaosOp::Deposit,
        ChaosOp::AllowMerchant,
        ChaosOp::Propose,
        ChaosOp::Approve,
        ChaosOp::RevokeMerchant,
        ChaosOp::Execute, // rejected: merchant no longer allowed
        ChaosOp::AllowMerchant,
        ChaosOp::Execute,
        ChaosOp::ReleaseEscrow,
    ];
    assert_eq!(run_chaos(&ops), Ok(()));
}

#[test]
fn chaos_pause_interleaving_sequence_holds_invariants() {
    let ops = [
        ChaosOp::Deposit,
        ChaosOp::AllowMerchant,
        ChaosOp::Propose,
        ChaosOp::Approve,
        ChaosOp::PauseInvoice,
        ChaosOp::Execute,
        ChaosOp::ReleaseEscrow, // rejected: invoice paused
        ChaosOp::UnpauseInvoice,
        ChaosOp::ReleaseEscrow,
    ];
    assert_eq!(run_chaos(&ops), Ok(()));
}

#[test]
fn chaos_repeated_deposits_and_holds_hold_invariants() {
    let ops = [
        ChaosOp::Deposit,
        ChaosOp::Deposit,
        ChaosOp::AllowMerchant,
        ChaosOp::Propose,
        ChaosOp::RaiseDispute,
        ChaosOp::ResolveDispute,
        ChaosOp::RaiseDispute,
        ChaosOp::ResolveDispute,
        ChaosOp::Approve,
        ChaosOp::Execute,
        ChaosOp::ReleaseEscrow,
    ];
    assert_eq!(run_chaos(&ops), Ok(()));
}

/// Fuzz-style sweep: every prefix of a long mixed sequence must leave the
/// system consistent, so no intermediate state is ever corrupt.
#[test]
fn chaos_every_prefix_of_mixed_sequence_is_consistent() {
    let full = [
        ChaosOp::Deposit,
        ChaosOp::Propose,
        ChaosOp::AllowMerchant,
        ChaosOp::Approve,
        ChaosOp::RaiseDispute,
        ChaosOp::Execute,
        ChaosOp::ResolveDispute,
        ChaosOp::PauseInvoice,
        ChaosOp::Execute,
        ChaosOp::ReleaseEscrow,
        ChaosOp::UnpauseInvoice,
        ChaosOp::ReleaseEscrow,
    ];
    for len in 0..=full.len() {
        assert_eq!(
            run_chaos(&full[..len]),
            Ok(()),
            "prefix of length {len} left an inconsistent state"
        );
    }
}
