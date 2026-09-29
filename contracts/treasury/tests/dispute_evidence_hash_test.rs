//! Tests for the optional evidence hash attached to disputes (#574).

use soroban_sdk::{testutils::Address as _, Address, BytesN, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

fn setup(env: &Env) -> (TreasuryContractClient, Address) {
    let admin = Address::generate(env);
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

#[test]
fn raise_dispute_with_evidence_hash_returns_it_via_get_dispute() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    let hash = BytesN::from_array(&env, &[7u8; 32]);
    let did = client.raise_dispute(
        &claimant,
        &sid,
        &merchant,
        &5_000_000,
        &500,
        &Some(hash.clone()),
    );

    let dispute = client.get_dispute(&did);
    assert_eq!(dispute.evidence_hash, Some(hash));
}

#[test]
fn raise_dispute_without_evidence_hash_returns_none() {
    let env = Env::default();
    env.mock_all_auths();
    let (client, admin) = setup(&env);

    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);

    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

    let dispute = client.get_dispute(&did);
    assert!(dispute.evidence_hash.is_none());
}
