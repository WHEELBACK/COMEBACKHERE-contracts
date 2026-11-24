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

fn has_hold_total(env: &Env, contract: &Address, sid: u64) -> bool {
    env.as_contract(contract, || {
        env.storage().persistent().has(&DataKey::HoldTotal(sid))
    })
}

fn hold_total(env: &Env, contract: &Address, sid: u64) -> i128 {
    env.as_contract(contract, || {
        env.storage()
            .persistent()
            .get(&DataKey::HoldTotal(sid))
            .unwrap_or(0)
    })
}

#[test]
fn hold_total_matches_settlement_amount_while_held() {
    let (env, id, client, admin, sid) = setup();
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold, &Some(10_000));

    assert!(has_hold_total(&env, &id, sid));
    assert_eq!(hold_total(&env, &id, sid), 1_000_000);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::Held);
}

#[test]
fn hold_total_cleared_on_every_terminal_transition() {
    // Force cancel path.
    let (env, id, client, admin, sid) = setup();
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold, &Some(10_000));
    client.force_cancel_settlement(&admin, &sid);
    assert!(!has_hold_total(&env, &id, sid));
    assert_eq!(hold_total(&env, &id, sid), 0);

    // Release path.
    let (env, id, client, admin, sid) = setup();
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold, &Some(10_000));
    client.release_settlement(&admin, &sid);
    assert!(!has_hold_total(&env, &id, sid));
    assert_eq!(hold_total(&env, &id, sid), 0);
}

#[test]
fn invariant_verification_detects_hold_balance_mismatch() {
    let (env, id, client, admin, sid) = setup();
    client.hold_settlement(&admin, &sid, &SettlementHoldReason::AdminHold, &Some(10_000));

    // A consistent state passes verification.
    assert!(client.verify_invariants());

    // Corrupt the recorded hold total so it no longer matches the settlement.
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::HoldTotal(sid), &1_i128);
    });

    // Strict verification must reject the inconsistent state.
    let result = client.try_verify_invariants();
    assert!(result.is_err());
}

#[test]
fn invariant_verification_rejects_orphaned_hold_state() {
    let (env, id, client, admin, sid) = setup();
    client.cancel_settlement(&admin, &sid);
    assert!(client.verify_invariants());

    // Orphaned hold state on a terminal settlement must be flagged.
    env.as_contract(&id, || {
        env.storage()
            .persistent()
            .set(&DataKey::HoldTotal(sid), &1_000_000_i128);
    });

    let result = client.try_verify_invariants();
    assert!(result.is_err());
}
