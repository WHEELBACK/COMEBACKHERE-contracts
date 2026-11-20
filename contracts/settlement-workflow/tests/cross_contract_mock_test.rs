//! #877: cross-contract invocation mock tests for `settlement-workflow`.
//!
//! The workflow's job is pure orchestration: ask compliance about the
//! merchant, then — only if allowed — ask treasury to execute as the workflow
//! itself. These tests replace both counterparties with recording mocks
//! registered through the Soroban testutils host, so every cross-contract
//! call can be asserted on directly: whether it happened, in what order,
//! with which arguments, under whose authorization, and what the workflow
//! does with each possible response.

use settlement_workflow::{SettlementWorkflowContract, SettlementWorkflowContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    token, Address, Env, Symbol, TryFromVal, Vec,
};
use treasury::TreasuryError;

const AMOUNT: i128 = 5_000;

mod mocks {
    use soroban_sdk::{
        contract, contracterror, contractimpl, symbol_short, Address, Env, Vec,
    };
    use treasury::{MaybeAddress, Settlement, SettlementHoldReason, SettlementStatus};

    /// Compliance double: returns a configurable verdict and records every
    /// `is_allowed` call plus a global call sequence number.
    #[contract]
    pub struct MockCompliance;

    #[contractimpl]
    impl MockCompliance {
        pub fn set_verdict(env: Env, allowed: bool) {
            env.storage().instance().set(&symbol_short!("verdict"), &allowed);
        }

        pub fn is_allowed(env: Env, address: Address) -> bool {
            let calls: u32 = env.storage().instance().get(&symbol_short!("calls")).unwrap_or(0);
            env.storage().instance().set(&symbol_short!("calls"), &(calls + 1));
            env.storage().instance().set(&symbol_short!("last"), &address);
            env.storage()
                .instance()
                .get(&symbol_short!("verdict"))
                .unwrap_or(true)
        }

        pub fn is_allowed_for_amount(env: Env, address: Address, _amount: i128) -> bool {
            Self::is_allowed(env, address)
        }

        pub fn bulk_check_addresses(env: Env, addresses: Vec<Address>) -> Vec<bool> {
            let mut out = Vec::new(&env);
            for a in addresses.iter() {
                out.push_back(Self::is_allowed(env.clone(), a));
            }
            out
        }

        pub fn calls(env: Env) -> u32 {
            env.storage().instance().get(&symbol_short!("calls")).unwrap_or(0)
        }

        pub fn last_checked(env: Env) -> Option<Address> {
            env.storage().instance().get(&symbol_short!("last"))
        }
    }

    #[contracterror]
    #[derive(Copy, Clone, Debug, Eq, PartialEq)]
    #[repr(u32)]
    pub enum MockError {
        Refused = 77,
    }

    /// Treasury double: records `execute_settlement` arguments, requires the
    /// signer's auth like the real treasury, can refuse selected IDs, and
    /// serves `get_settlement` from its own state.
    #[contract]
    pub struct MockTreasury;

    #[contractimpl]
    impl MockTreasury {
        pub fn configure(env: Env, merchant: Address, amount: i128, refuse_odd: bool) {
            let s = env.storage().instance();
            s.set(&symbol_short!("merchant"), &merchant);
            s.set(&symbol_short!("amount"), &amount);
            s.set(&symbol_short!("refuse"), &refuse_odd);
        }

        pub fn execute_settlement(
            env: Env,
            signer: Address,
            settlement_id: u64,
            token_contract: Address,
        ) -> Result<(), MockError> {
            signer.require_auth();
            let refuse_odd: bool = env.storage().instance().get(&symbol_short!("refuse")).unwrap_or(false);
            if refuse_odd && settlement_id % 2 == 1 {
                return Err(MockError::Refused);
            }
            let s = env.storage().instance();
            let calls: u32 = s.get(&symbol_short!("calls")).unwrap_or(0);
            s.set(&symbol_short!("calls"), &(calls + 1));
            s.set(&symbol_short!("signer"), &signer);
            s.set(&symbol_short!("token"), &token_contract);
            let mut ids: Vec<u64> = s.get(&symbol_short!("ids")).unwrap_or(Vec::new(&env));
            ids.push_back(settlement_id);
            s.set(&symbol_short!("ids"), &ids);
            Ok(())
        }

        pub fn get_signer_weight(_env: Env, _signer: Address) -> u32 {
            1
        }

        pub fn get_settlement(env: Env, settlement_id: u64) -> Settlement {
            let s = env.storage().instance();
            let executed: Vec<u64> = s.get(&symbol_short!("ids")).unwrap_or(Vec::new(&env));
            Settlement {
                id: settlement_id,
                merchant_address: s.get(&symbol_short!("merchant")).unwrap(),
                amount: s.get(&symbol_short!("amount")).unwrap_or(0),
                approvals: Vec::new(&env),
                approval_weight: 1,
                status: if executed.contains(settlement_id) {
                    SettlementStatus::Executed
                } else {
                    SettlementStatus::Pending
                },
                hold_reason: SettlementHoldReason::None,
                proposed_at: 0,
                execution_deadline: 0,
                token: MaybeAddress::None,
            }
        }

        pub fn calls(env: Env) -> u32 {
            env.storage().instance().get(&symbol_short!("calls")).unwrap_or(0)
        }

        pub fn last_signer(env: Env) -> Option<Address> {
            env.storage().instance().get(&symbol_short!("signer"))
        }

        pub fn last_token(env: Env) -> Option<Address> {
            env.storage().instance().get(&symbol_short!("token"))
        }

        pub fn executed_ids(env: Env) -> Vec<u64> {
            env.storage().instance().get(&symbol_short!("ids")).unwrap_or(Vec::new(&env))
        }
    }
}

use mocks::{MockCompliance, MockComplianceClient, MockTreasury, MockTreasuryClient};

struct Ctx {
    env: Env,
    merchant: Address,
    token: Address,
    compliance: MockComplianceClient<'static>,
    treasury: MockTreasuryClient<'static>,
    workflow_id: Address,
    workflow: SettlementWorkflowContractClient<'static>,
}

fn ctx_with(amount: i128, refuse_odd: bool, treasury_funds: i128) -> Ctx {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);

    let compliance_id = env.register(MockCompliance, ());
    let compliance = MockComplianceClient::new(&env, &compliance_id);
    let treasury_id = env.register(MockTreasury, ());
    let treasury = MockTreasuryClient::new(&env, &treasury_id);
    treasury.configure(&merchant, &amount, &refuse_odd);

    let token = env.register_stellar_asset_contract(admin.clone());
    if treasury_funds > 0 {
        token::StellarAssetClient::new(&env, &token).mint(&treasury_id, &treasury_funds);
    }

    let workflow_id = env.register(
        SettlementWorkflowContract,
        (admin.clone(), compliance_id.clone(), treasury_id.clone()),
    );
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);

    Ctx {
        env,
        merchant,
        token,
        compliance,
        treasury,
        workflow_id,
        workflow,
    }
}

fn ctx() -> Ctx {
    ctx_with(AMOUNT, false, AMOUNT * 10)
}

/// `(settlement_id, merchant, token, amount)` for every
/// `settlement_workflow_executed` event the workflow published in the last
/// invocation.
fn executed_events(c: &Ctx) -> std::vec::Vec<(u64, Address, Address, i128)> {
    let name = Symbol::new(&c.env, "settlement_workflow_executed");
    c.env
        .events()
        .all()
        .iter()
        .filter(|(contract, _, _)| *contract == c.workflow_id)
        .filter_map(|(_, topics, data)| {
            let first = Symbol::try_from_val(&c.env, &topics.get(0)?).ok()?;
            if first != name {
                return None;
            }
            let id = u64::try_from_val(&c.env, &topics.get(1)?).ok()?;
            let (merchant, token, amount) =
                <(Address, Address, i128)>::try_from_val(&c.env, &data).ok()?;
            Some((id, merchant, token, amount))
        })
        .collect()
}

#[test]
fn allowed_merchant_flows_compliance_then_treasury_with_exact_arguments() {
    let c = ctx();
    c.workflow
        .execute_with_compliance(&7, &c.token, &c.merchant);

    assert_eq!(c.compliance.calls(), 1);
    assert_eq!(c.compliance.last_checked(), Some(c.merchant.clone()));

    assert_eq!(c.treasury.calls(), 1);
    // The workflow executes as itself, never as the merchant or caller.
    assert_eq!(c.treasury.last_signer(), Some(c.workflow_id.clone()));
    assert_eq!(c.treasury.last_token(), Some(c.token.clone()));
    assert_eq!(c.treasury.executed_ids(), Vec::from_array(&c.env, [7u64]));

    // The summary event carries the amount read back from treasury.
    assert_eq!(
        executed_events(&c),
        std::vec![(7, c.merchant.clone(), c.token.clone(), AMOUNT)]
    );
}

#[test]
fn denied_merchant_never_reaches_treasury() {
    let c = ctx();
    c.compliance.set_verdict(&false);

    let err = c
        .workflow
        .try_execute_with_compliance(&7, &c.token, &c.merchant)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, TreasuryError::ComplianceCheckFailed);
    assert_eq!(c.treasury.calls(), 0);
    assert!(executed_events(&c).is_empty());
}

#[test]
fn treasury_refusal_propagates_and_rolls_back() {
    // Funded, so the failure is the mock's own refusal, not a balance issue.
    let c = ctx_with(AMOUNT, true, AMOUNT * 10);
    let res = c.workflow.try_execute_with_compliance(&3, &c.token, &c.merchant);

    assert!(res.is_err());
    assert_ne!(res, Err(Ok(TreasuryError::InsufficientBalance)));
    // The whole invocation reverted: no recorded execution, no event.
    assert_eq!(c.treasury.calls(), 0);
    assert!(c.treasury.executed_ids().is_empty());
    assert!(executed_events(&c).is_empty());
}

#[test]
fn treasury_refusal_with_zero_balance_is_reported_as_insufficient_balance() {
    let c = ctx_with(AMOUNT, true, 0);
    assert_eq!(
        c.workflow
            .try_execute_with_compliance(&3, &c.token, &c.merchant),
        Err(Ok(TreasuryError::InsufficientBalance))
    );
    assert_eq!(c.treasury.calls(), 0);
}

#[test]
fn batch_checks_compliance_once_and_skips_refused_ids() {
    let c = ctx_with(AMOUNT, true, AMOUNT * 10);
    let ids = Vec::from_array(&c.env, [1u64, 2, 3, 4, 5, 6]);

    let executed = c
        .workflow
        .execute_with_compliance_batch(&ids, &c.token, &c.merchant);

    assert_eq!(executed, Vec::from_array(&c.env, [2u64, 4, 6]));
    assert_eq!(c.compliance.calls(), 1);
    assert_eq!(c.treasury.calls(), 3);
    assert_eq!(c.treasury.executed_ids(), executed);
    let events: std::vec::Vec<u64> = executed_events(&c).into_iter().map(|e| e.0).collect();
    assert_eq!(events, std::vec![2, 4, 6]);
}

#[test]
fn batch_denied_merchant_never_reaches_treasury() {
    let c = ctx();
    c.compliance.set_verdict(&false);
    let ids = Vec::from_array(&c.env, [1u64, 2]);
    assert_eq!(
        c.workflow
            .try_execute_with_compliance_batch(&ids, &c.token, &c.merchant)
            .unwrap_err()
            .unwrap(),
        TreasuryError::ComplianceCheckFailed
    );
    assert_eq!(c.treasury.calls(), 0);
}

#[test]
fn zero_amount_settlement_from_treasury_does_not_panic() {
    let c = ctx_with(0, false, 0);
    c.workflow
        .execute_with_compliance(&9, &c.token, &c.merchant);
    assert_eq!(
        executed_events(&c),
        std::vec![(9, c.merchant.clone(), c.token.clone(), 0)]
    );
}

#[test]
fn workflow_authorizes_the_treasury_call_as_itself_without_user_signatures() {
    let c = ctx();
    // Switch from mock-all to strict auth: no user signs anything.
    c.env.set_auths(&[]);

    c.workflow
        .execute_with_compliance(&7, &c.token, &c.merchant);
    // Succeeding under strict auth proves the treasury's
    // `signer.require_auth()` was satisfied by the workflow's own
    // contract-invoker authorization.
    assert_eq!(c.treasury.last_signer(), Some(c.workflow_id.clone()));
}

#[test]
fn unregistered_counterparties_fail_without_side_effects() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let token = env.register_stellar_asset_contract(admin.clone());

    // Neither address hosts a contract.
    let workflow_id = env.register(
        SettlementWorkflowContract,
        (admin, Address::generate(&env), Address::generate(&env)),
    );
    let workflow = SettlementWorkflowContractClient::new(&env, &workflow_id);
    assert!(workflow
        .try_execute_with_compliance(&1, &token, &merchant)
        .is_err());
    assert!(workflow
        .try_execute_with_compliance_batch(&Vec::from_array(&env, [1u64]), &token, &merchant)
        .is_err());
    assert_eq!(token::Client::new(&env, &token).balance(&merchant), 0);
}

