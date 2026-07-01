use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env) -> (TreasuryContractClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

#[test]
fn second_dispute_does_not_double_transition() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant_a = Address::generate(&env);
    let claimant_b = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    client.raise_dispute(&claimant_a, &sid, &merchant, &5_000_000, &500);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    client.raise_dispute(&claimant_b, &sid, &merchant, &3_000_000, &500);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);
}

#[test]
fn settlement_stays_on_hold_while_any_dispute_open() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant_a = Address::generate(&env);
    let claimant_b = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    let did_a = client.raise_dispute(&claimant_a, &sid, &merchant, &5_000_000, &500);
    let did_b = client.raise_dispute(&claimant_b, &sid, &merchant, &3_000_000, &500);

    // Resolve dispute A; dispute B is still open so settlement stays OnHold
    client.resolve_dispute(&admin, &did_a, &true);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    // Resolve dispute B in the opposite direction; now no open disputes remain
    client.resolve_dispute(&admin, &did_b, &false);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending
    );
}

#[test]
fn both_disputes_resolved_same_direction_releases_hold() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant_a = Address::generate(&env);
    let claimant_b = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    let did_a = client.raise_dispute(&claimant_a, &sid, &merchant, &5_000_000, &500);
    let did_b = client.raise_dispute(&claimant_b, &sid, &merchant, &3_000_000, &500);

    client.resolve_dispute(&admin, &did_a, &false);
    assert_eq!(client.get_settlement(&sid).status, SettlementStatus::OnHold);

    client.resolve_dispute(&admin, &did_b, &false);
    assert_eq!(
        client.get_settlement(&sid).status,
        SettlementStatus::Pending
    );
}
