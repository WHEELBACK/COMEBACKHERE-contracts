//! #876: zero/insufficient treasury balance must surface as a typed
//! `TreasuryError::InsufficientBalance` from `execute_with_compliance` instead
//! of an opaque trap inside the token transfer, and must never mask the
//! errors treasury already reports for other failures.

use comebackhere_tests::fixtures::{setup_with_workflow, WorkflowFixture};
use soroban_sdk::{token, Address, Env, Vec};
use treasury::{SettlementStatus, TreasuryError};

const AMOUNT: i128 = 10_000_000;

struct Ctx {
    env: Env,
    f: WorkflowFixture,
    token: Address,
}

fn ctx() -> Ctx {
    let env = Env::default();
    let f = setup_with_workflow(&env, true);
    let token = env.register_stellar_asset_contract(f.admin.clone());
    f.compliance.allow_address(&f.admin, &f.merchant);
    Ctx { env, f, token }
}

impl Ctx {
    fn propose(&self) -> u64 {
        self.f
            .treasury
            .propose_settlement(&self.f.admin, &self.f.merchant, &AMOUNT, &0_u64)
    }

    fn fund_treasury(&self, amount: i128) {
        if amount > 0 {
            token::StellarAssetClient::new(&self.env, &self.token)
                .mint(&self.f.treasury_contract_id, &amount);
        }
    }

    fn balance(&self, who: &Address) -> i128 {
        token::Client::new(&self.env, &self.token).balance(who)
    }

    fn execute_err(&self, id: u64) -> TreasuryError {
        self.f
            .workflow
            .try_execute_with_compliance(&id, &self.token, &self.f.merchant)
            .unwrap_err()
            .unwrap()
    }
}

#[test]
fn zero_treasury_balance_returns_insufficient_balance() {
    let c = ctx();
    let id = c.propose();

    assert_eq!(c.execute_err(id), TreasuryError::InsufficientBalance);
    // Nothing moved and the settlement is still executable once funded.
    assert_eq!(c.balance(&c.f.merchant), 0);
    assert_eq!(
        c.f.treasury.get_settlement(&id).status,
        SettlementStatus::Pending
    );

    c.fund_treasury(AMOUNT);
    c.f.workflow
        .execute_with_compliance(&id, &c.token, &c.f.merchant);
    assert_eq!(c.balance(&c.f.merchant), AMOUNT);
}

#[test]
fn balances_below_the_amount_are_rejected_and_exact_balance_succeeds() {
    for funded in [0, 1, AMOUNT / 2, AMOUNT - 1] {
        let c = ctx();
        let id = c.propose();
        c.fund_treasury(funded);
        assert_eq!(
            c.execute_err(id),
            TreasuryError::InsufficientBalance,
            "funded {funded}"
        );
        assert_eq!(c.balance(&c.f.treasury_contract_id), funded);
    }

    let c = ctx();
    let id = c.propose();
    c.fund_treasury(AMOUNT);
    c.f.workflow
        .execute_with_compliance(&id, &c.token, &c.f.merchant);
    // The treasury is now at exactly zero.
    assert_eq!(c.balance(&c.f.treasury_contract_id), 0);
}

#[test]
fn drained_treasury_still_reports_already_executed_on_retry() {
    let c = ctx();
    let id = c.propose();
    c.fund_treasury(AMOUNT);
    c.f.workflow
        .execute_with_compliance(&id, &c.token, &c.f.merchant);
    assert_eq!(c.balance(&c.f.treasury_contract_id), 0);

    assert_eq!(c.execute_err(id), TreasuryError::AlreadyExecuted);
}

#[test]
fn zero_balance_does_not_mask_missing_settlement() {
    let c = ctx();
    for id in [0_u64, 42, u64::MAX] {
        assert_eq!(c.execute_err(id), TreasuryError::SettlementNotFound);
    }
}

#[test]
fn zero_balance_does_not_mask_compliance_failure() {
    let c = ctx();
    let id = c.propose();
    c.f.compliance.revoke_allow(&c.f.admin, &c.f.merchant);
    assert_eq!(c.execute_err(id), TreasuryError::ComplianceCheckFailed);
}

#[test]
fn batch_with_zero_balance_skips_without_panicking() {
    let c = ctx();
    let ids = Vec::from_array(&c.env, [c.propose(), c.propose()]);

    let executed =
        c.f.workflow
            .execute_with_compliance_batch(&ids, &c.token, &c.f.merchant);
    assert!(executed.is_empty());
    assert_eq!(c.balance(&c.f.merchant), 0);

    // Funding for one settlement executes exactly one.
    c.fund_treasury(AMOUNT);
    let executed =
        c.f.workflow
            .execute_with_compliance_batch(&ids, &c.token, &c.f.merchant);
    assert_eq!(executed.len(), 1);
    assert_eq!(c.balance(&c.f.merchant), AMOUNT);
    assert_eq!(c.balance(&c.f.treasury_contract_id), 0);
}

#[test]
fn empty_batch_is_a_no_op() {
    let c = ctx();
    let executed =
        c.f.workflow
            .execute_with_compliance_batch(&Vec::new(&c.env), &c.token, &c.f.merchant);
    assert!(executed.is_empty());
}
