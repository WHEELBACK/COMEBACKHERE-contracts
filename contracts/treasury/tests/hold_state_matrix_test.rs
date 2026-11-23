//! #745: every terminal settlement transition clears leftover hold state, so a
//! settlement that was held with an expiry never leaves `HoldExpiry` behind.
use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{DataKey, SettlementHoldReason, SettlementStatus, TreasuryContract, TreasuryContractClient};

fn setup() -> (Env, Address, TreasuryContractClient<'static>, Address, u64) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    client.set_signer(&admin, &admin, &1);
    let sid = client.propose_settlement(&admin, &merchant, &1_000_000, &0_u64);
    (env, id, client, admin, sid)
}

fn has_hold_expiry(env: &Env, contract: &Address, sid: u64) -> bool {
    env.as_contract(contract, || {
        env.storage().persistent().has(&DataKey::HoldExpiry(sid))
    })
}

#[test]
fn hold_then_force_cancel_releases_hold_state() {
    let (env, id, client, admin, sid) = setup();
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold, &Some(10_000));
    assert!(has_hold_expiry(&env, &id, sid));

    client.force_cancel_settlement(&admin, &sid);

    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::Cancelled);
    assert!(!has_hold_expiry(&env, &id, sid));
}

#[test]
fn settlement_without_hold_has_no_hold_state_after_cancel() {
    let (env, id, client, admin, sid) = setup();
    client.cancel_settlement(&admin, &sid);
    assert!(!has_hold_expiry(&env, &id, sid));
}
