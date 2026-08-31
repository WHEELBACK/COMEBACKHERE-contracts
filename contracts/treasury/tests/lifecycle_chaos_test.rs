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
        .propose_settlement(&ctx.admin, &ctx.merchant, &AMOUNT);
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
            ctx.treasury
                .raise_dispute(&claimant, &settlement_id, &ctx.merchant, &1, &u64::MAX);
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
