use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    Address, Env,
};
use treasury::{TreasuryContract, TreasuryContractClient};

fn setup(env: &Env, threshold: u32) -> (TreasuryContractClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &threshold, &soroban_sdk::Vec::new(env));
    (client, admin)
}

#[test]
#[should_panic(expected = "RotationProposalCooldown")]
fn repeated_rotation_proposal_within_cooldown_is_rejected() {
    let env = Env::default();
    let (client, admin) = setup(&env, 2);
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let another_signer = Address::generate(&env);

    client.set_signer(&admin, &old_signer, &1);

    client.propose_signer_rotation(&admin, &old_signer, &new_signer);
    // Same proposer tries again immediately: should be rejected by the cooldown.
    client.propose_signer_rotation(&admin, &old_signer, &another_signer);
}

#[test]
fn rotation_proposal_allowed_after_cooldown_elapses() {
    let env = Env::default();
    let (client, admin) = setup(&env, 2);
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let another_signer = Address::generate(&env);

    client.set_signer(&admin, &old_signer, &1);

    client.propose_signer_rotation(&admin, &old_signer, &new_signer);

    // Advance ledger time beyond the cooldown window.
    env.ledger().with_mut(|l| l.timestamp += 60 * 60 + 1);

    let second_id = client.propose_signer_rotation(&admin, &old_signer, &another_signer);
    assert!(second_id > 0);
}

#[test]
fn different_proposers_are_not_subject_to_each_others_cooldown() {
    let env = Env::default();
    let (client, admin) = setup(&env, 2);
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let other_proposer = Address::generate(&env);

    client.set_signer(&admin, &old_signer, &1);
    client.set_signer(&admin, &other_proposer, &1);

    client.propose_signer_rotation(&admin, &old_signer, &new_signer);
    // A different proposer is unaffected by admin's cooldown.
    let second_id = client.propose_signer_rotation(&other_proposer, &old_signer, &new_signer);
    assert!(second_id > 0);
}
