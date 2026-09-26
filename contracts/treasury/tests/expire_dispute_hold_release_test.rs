/// Regression tests for `expire_dispute` hold-release behaviour (issue #586).
///
/// Raising a dispute places a settlement `OnHold`. If that dispute expires
/// without resolution the hold must be released so the settlement can proceed
/// to execution. These tests pin the full round-trip: raise dispute →
/// advance time → expire dispute → settlement back to Pending → executable.
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env, Vec,
};
use treasury::{DisputeStatus, SettlementHoldReason, SettlementStatus, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env) -> (TreasuryContractClient, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &1, &Vec::new(env));
    let token_id = env.register_stellar_asset_contract(admin.clone());
    (client, admin, token_id)
}

// ── Core regression: settlement is executable after dispute expires ───────

#[test]
fn settlement_executable_after_dispute_expires() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, token_id) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);
    let contract_addr = env.register_contract(None, TreasuryContract);

    // Re-create client pointed at the already-initialised contract for mint.
    // Mint treasury balance so execute_settlement can transfer funds.
    soroban_sdk::token::StellarAssetClient::new(&env, &token_id)
        .mint(&contract_addr, &20_000_000);

    // Use a fresh contract for the actual test.
    let contract_id = {
        let id = env.register_contract(None, TreasuryContract);
        let c = TreasuryContractClient::new(&env, &id);
        c.initialize(&admin, &1, &Vec::new(&env));
        soroban_sdk::token::StellarAssetClient::new(&env, &token_id)
            .mint(&id, &20_000_000);
        id
    };
    let client = TreasuryContractClient::new(&env, &contract_id);

    // Propose a settlement.
    let sid = client.propose_settlement(&admin, &merchant, &5_000_000);

    // Confirm it starts as Pending.
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::Pending);

    // Raise a dispute — places settlement OnHold.
    let expires_at: u64 = 1_000;
    let did = client.raise_dispute(&claimant, &sid, &merchant, &1_000_000, &expires_at);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Trying to execute while on-hold should fail.
    let result = client.try_execute_settlement(&admin, &sid, &token_id);
    assert!(result.is_err(), "execute_settlement must fail while OnHold");

    // Advance time past the dispute deadline and expire it.
    env.ledger().with_mut(|l| l.timestamp = expires_at + 1);
    client.expire_dispute(&admin, &did);

    // Dispute transitions to Expired.
    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Expired);

    // Settlement must be released back to Pending.
    let s = client.get_settlement(&sid);
    assert_eq!(s.status, SettlementStatus::Pending);
    assert_eq!(s.hold_reason, SettlementHoldReason::None);

    // Settlement is now executable without error.
    client.execute_settlement(&admin, &sid, &token_id);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::Executed);
}

// ── Hold reason is cleared on expiry ─────────────────────────────────────

#[test]
fn hold_reason_cleared_when_dispute_expires() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin, _) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let contract_id = {
        let id = env.register_contract(None, TreasuryContract);
        let c = TreasuryContractClient::new(&env, &id);
        c.initialize(&admin, &1, &Vec::new(&env));
        id
    };
    let client = TreasuryContractClient::new(&env, &contract_id);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500);

    // Confirm OnHold.
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    env.ledger().with_mut(|l| l.timestamp = 600);
    client.expire_dispute(&admin, &did);

    let s = client.get_settlement(&sid);
    assert_eq!(s.status, SettlementStatus::Pending);
    assert_eq!(s.hold_reason, SettlementHoldReason::None);
}

// ── Multiple disputes: hold released only when all open disputes expire ───

#[test]
fn hold_released_only_after_all_disputes_expire() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &contract_id);
    client.initialize(&admin, &1, &Vec::new(&env));

    let merchant = Address::generate(&env);
    let claimant_a = Address::generate(&env);
    let claimant_b = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    // Two separate disputes against the same settlement.
    let did_a = client.raise_dispute(&claimant_a, &sid, &merchant, &3_000_000, &500);
    let did_b = client.raise_dispute(&claimant_b, &sid, &merchant, &2_000_000, &800);

    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Expire the first dispute; second is still Raised, so hold must stay.
    env.ledger().with_mut(|l| l.timestamp = 600);
    client.expire_dispute(&admin, &did_a);
    // Settlement should still be OnHold because did_b is still Raised.
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::OnHold,
        "hold must remain while a second dispute is still open"
    );

    // Now expire the second dispute.
    env.ledger().with_mut(|l| l.timestamp = 900);
    client.expire_dispute(&admin, &did_b);

    // Both disputes expired — hold must be released.
    let s = client.get_settlement(&sid);
    assert_eq!(s.status, SettlementStatus::Pending);
    assert_eq!(s.hold_reason, SettlementHoldReason::None);
}
