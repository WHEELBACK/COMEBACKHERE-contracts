// #577 — signers can withdraw their approval of a settlement before it is executed.
// Weight tracking must follow: if revoking drops the total below the threshold,
// execution is blocked again.

use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Events},
    Address, Env, Symbol, TryFromVal,
};
use treasury::{
    Settlement, SettlementStatus, TreasuryContract, TreasuryContractClient, TreasuryError,
};

#[contract]
struct FakeToken;
#[contractimpl]
impl FakeToken {
    pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {}
}

/// Threshold 2; `admin` and `backup` each carry weight 1. The settlement is proposed
/// (and therefore approved) by `admin`.
fn setup(env: &Env) -> (TreasuryContractClient<'_>, Address, Address, u64) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let backup = Address::generate(env);
    let merchant = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &2, &soroban_sdk::Vec::new(env));
    client.set_signer(&admin, &backup, &1);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    (client, admin, backup, sid)
}

#[test]
fn revoke_removes_approval_and_its_weight() {
    let env = Env::default();
    let (client, _admin, backup, sid) = setup(&env);

    let approved = client.approve_settlement(&backup, &sid);
    assert_eq!(approved.approval_weight, 2);
    assert_eq!(approved.approvals.len(), 2);

    let revoked = client.revoke_approval(&backup, &sid);
    assert_eq!(revoked.approval_weight, 1);
    assert_eq!(revoked.approvals.len(), 1);
    assert!(!revoked.approvals.contains(&backup));
    assert_eq!(revoked.status, SettlementStatus::Pending);
    assert_eq!(client.get_settlement(&sid), revoked);
}

#[test]
fn revoke_then_execute_fails_when_below_threshold() {
    let env = Env::default();
    let (client, admin, backup, sid) = setup(&env);
    let token_id = env.register_contract(None, FakeToken);

    client.approve_settlement(&backup, &sid);
    client.revoke_approval(&backup, &sid);

    assert_eq!(
        client.try_execute_settlement(&admin, &sid, &token_id),
        Err(Ok(TreasuryError::ThresholdNotMet))
    );
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending
    );
}

#[test]
fn reapproving_after_revoke_restores_ability_to_execute() {
    let env = Env::default();
    let (client, admin, backup, sid) = setup(&env);
    let token_id = env.register_contract(None, FakeToken);

    client.approve_settlement(&backup, &sid);
    client.revoke_approval(&backup, &sid);
    client.approve_settlement(&backup, &sid);

    client.execute_settlement(&admin, &sid, &token_id);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Executed
    );
}

#[test]
fn revoking_a_weighted_approval_subtracts_the_full_weight() {
    let env = Env::default();
    let (client, admin, _backup, sid) = setup(&env);
    let heavy = Address::generate(&env);
    client.set_signer(&admin, &heavy, &5);

    let approved = client.approve_settlement(&heavy, &sid);
    assert_eq!(approved.approval_weight, 6);

    let revoked = client.revoke_approval(&heavy, &sid);
    assert_eq!(revoked.approval_weight, 1);
}

#[test]
fn revoking_one_approval_leaves_others_intact() {
    let env = Env::default();
    let (client, admin, backup, sid) = setup(&env);

    client.approve_settlement(&backup, &sid);
    let revoked = client.revoke_approval(&admin, &sid);

    assert_eq!(revoked.approval_weight, 1);
    assert_eq!(revoked.approvals.len(), 1);
    assert!(revoked.approvals.contains(&backup));
}

#[test]
fn revoke_without_prior_approval_fails() {
    let env = Env::default();
    let (client, _admin, backup, sid) = setup(&env);

    assert_eq!(
        client.try_revoke_approval(&backup, &sid),
        Err(Ok(TreasuryError::ApprovalNotFound))
    );
}

#[test]
fn revoking_twice_fails_the_second_time() {
    let env = Env::default();
    let (client, _admin, backup, sid) = setup(&env);

    client.approve_settlement(&backup, &sid);
    client.revoke_approval(&backup, &sid);
    assert_eq!(
        client.try_revoke_approval(&backup, &sid),
        Err(Ok(TreasuryError::ApprovalNotFound))
    );
}

#[test]
fn revoke_after_execution_is_rejected() {
    let env = Env::default();
    let (client, admin, backup, sid) = setup(&env);
    let token_id = env.register_contract(None, FakeToken);

    client.approve_settlement(&backup, &sid);
    client.execute_settlement(&admin, &sid, &token_id);

    assert_eq!(
        client.try_revoke_approval(&backup, &sid),
        Err(Ok(TreasuryError::AlreadyExecuted))
    );
    assert_eq!(client.get_settlement(&sid).approval_weight, 2);
}

#[test]
fn revoke_on_cancelled_settlement_is_rejected() {
    let env = Env::default();
    let (client, admin, _backup, sid) = setup(&env);

    client.cancel_settlement(&admin, &sid);
    assert_eq!(
        client.try_revoke_approval(&admin, &sid),
        Err(Ok(TreasuryError::AlreadyExecuted))
    );
}

#[test]
fn revoke_unknown_settlement_fails() {
    let env = Env::default();
    let (client, admin, _backup, _sid) = setup(&env);

    assert_eq!(
        client.try_revoke_approval(&admin, &999),
        Err(Ok(TreasuryError::SettlementNotFound))
    );
}

#[test]
fn revoke_by_non_signer_is_rejected() {
    let env = Env::default();
    let (client, _admin, _backup, sid) = setup(&env);
    let outsider = Address::generate(&env);

    assert_eq!(
        client.try_revoke_approval(&outsider, &sid),
        Err(Ok(TreasuryError::UnauthorizedSigner))
    );
}

#[test]
fn revoke_is_blocked_while_paused() {
    let env = Env::default();
    let (client, admin, backup, sid) = setup(&env);

    client.approve_settlement(&backup, &sid);
    client.pause(&admin);

    assert_eq!(
        client.try_revoke_approval(&backup, &sid),
        Err(Ok(TreasuryError::ContractPaused))
    );
}

#[test]
fn revoke_emits_settlement_approval_revoked_event() {
    let env = Env::default();
    let (client, _admin, backup, sid) = setup(&env);

    client.approve_settlement(&backup, &sid);
    let revoked = client.revoke_approval(&backup, &sid);

    let events = env.events().all();
    let (_, topics, data) = events.last().unwrap();
    assert_eq!(
        Symbol::try_from_val(&env, &topics.get_unchecked(0)).unwrap(),
        Symbol::new(&env, "settlement_approval_revoked")
    );
    assert_eq!(
        u64::try_from_val(&env, &topics.get_unchecked(1)).unwrap(),
        sid
    );
    let (event_signer, event_settlement) =
        <(Address, Settlement)>::try_from_val(&env, &data).unwrap();
    assert_eq!(event_signer, backup);
    assert_eq!(event_settlement, revoked);
}
