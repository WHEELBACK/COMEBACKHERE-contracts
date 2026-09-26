extern crate std;

use comebackhere_treasury::{
    TreasuryContract, TreasuryContractClient, SettlementHoldReason, SettlementStatus,
};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{
    testutils::{Ledger, Logs},
    token::StellarAssetClient,
    token::TokenClient,
    Address, Env,
};

struct Tok<'a> {
    address: Address,
    client: TokenClient<'a>,
    admin: StellarAssetClient<'a>,
}

fn make_token(env: &Env) -> Tok<'_> {
    let issuer = Address::generate(env);
    let sac = env.register_stellar_asset_contract_v2(issuer);
    let address = sac.address();
    Tok {
        client: TokenClient::new(env, &address),
        admin: StellarAssetClient::new(env, &address),
        address,
    }
}

struct TestSetup<'a> {
    env: &'a Env,
    contract: TreasuryContractClient<'a>,
    token: Tok<'a>,
    admin: Address,
    signer: Address,
    merchant: Address,
}

fn setup(env: &Env) -> TestSetup<'_> {
    env.mock_all_auths();

    let admin = Address::generate(env);
    let signer = Address::generate(env);
    let merchant = Address::generate(env);
    let token = make_token(env);

    let contract_id = env.register(TreasuryContract, ());
    let contract = TreasuryContractClient::new(env, &contract_id);

    contract.initialize(&admin, &100u32, &[(signer.clone(), 100u32)]);

    token.admin.mint(&admin, &10000);
    token.admin.mint(&contract_id, &10000);

    TestSetup {
        env,
        contract,
        token,
        admin,
        signer,
        merchant,
    }
}

fn propose_settlement(setup: &TestSetup, amount: i128) -> u64 {
    setup.contract.propose_settlement(
        &setup.signer,
        &setup.merchant,
        &amount,
    )
}

fn approve_settlement(setup: &TestSetup, settlement_id: u64) {
    setup.contract.approve_settlement(&setup.signer, &settlement_id);
}

fn execute_settlement(setup: &TestSetup, settlement_id: u64) {
    setup.contract.execute_settlement(&setup.signer, &settlement_id, &setup.token.address);
}

fn expire_settlement(setup: &TestSetup, settlement_id: u64) {
    let expiry_secs = setup.contract.get_settlement_expiry();
    setup.env.ledger().with_mut(|li| {
        li.timestamp += expiry_secs + 1;
    });
    setup.contract.expire_settlement(&setup.admin, &settlement_id);
}

fn cancel_settlement(setup: &TestSetup, settlement_id: u64) {
    setup.contract.cancel_settlement(&setup.signer, &settlement_id);
}

#[test]
fn hold_release_pending() {
    let env = Env::default();
    let setup = setup(&env);
    let sid = propose_settlement(&setup, 100);

    setup.contract.hold_settlement(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::Manual);
    assert_eq!(settlement.status, SettlementStatus::Pending);

    setup.contract.release_hold(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::None);
}

#[test]
fn hold_release_approved() {
    let env = Env::default();
    let setup = setup(&env);
    let sid = propose_settlement(&setup, 100);
    approve_settlement(&setup, sid);

    setup.contract.hold_settlement(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::Manual);
    assert_eq!(settlement.status, SettlementStatus::Pending);

    setup.contract.release_hold(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::None);
}

#[test]
fn hold_release_partially_executed() {
    let env = Env::default();
    let setup = setup(&env);
    let sid = propose_settlement(&setup, 100);
    approve_settlement(&setup, sid);
    setup.contract.partially_execute_settlement(&setup.signer, &sid, &50i128, &setup.token.address);

    setup.contract.hold_settlement(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::Manual);
    assert_eq!(settlement.status, SettlementStatus::PartiallyExecuted);

    setup.contract.release_hold(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::None);
}

#[test]
fn hold_release_executed() {
    let env = Env::default();
    let setup = setup(&env);
    let sid = propose_settlement(&setup, 100);
    approve_settlement(&setup, sid);
    execute_settlement(&setup, sid);

    setup.contract.hold_settlement(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::Manual);
    assert_eq!(settlement.status, SettlementStatus::Executed);

    setup.contract.release_hold(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::None);
}

#[test]
fn hold_release_cancelled() {
    let env = Env::default();
    let setup = setup(&env);
    let sid = propose_settlement(&setup, 100);
    approve_settlement(&setup, sid);
    cancel_settlement(&setup, sid);

    setup.contract.hold_settlement(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::Manual);
    assert_eq!(settlement.status, SettlementStatus::Cancelled);

    setup.contract.release_hold(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::None);
}

#[test]
fn hold_release_expired() {
    let env = Env::default();
    let setup = setup(&env);
    let sid = propose_settlement(&setup, 100);
    expire_settlement(&setup, sid);

    setup.contract.hold_settlement(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::Manual);
    assert_eq!(settlement.status, SettlementStatus::Expired);

    setup.contract.release_hold(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::None);
}

#[test]
fn double_hold_updates_reason() {
    let env = Env::default();
    let setup = setup(&env);
    let sid = propose_settlement(&setup, 100);

    setup.contract.hold_settlement(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::Manual);

    setup.contract.hold_settlement(&setup.admin, &sid);
    let settlement = setup.contract.get_settlement(&sid);
    assert_eq!(settlement.hold_reason, SettlementHoldReason::Manual);
}
