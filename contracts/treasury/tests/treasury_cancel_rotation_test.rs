use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{RotationStatus, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env, threshold: u32) -> (TreasuryContractClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &threshold);
    (client, admin)
}

#[test]
fn admin_can_cancel_pending_rotation() {
    let env = Env::default();
    let (client, admin) = setup(&env, 2);
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);

    client.set_signer(&admin, &old_signer, &1);
    let rotation_id = client.propose_signer_rotation(&admin, &old_signer, &new_signer);

    let proposal = client.cancel_rotation(&admin, &rotation_id);
    assert_eq!(proposal.status, RotationStatus::Cancelled);
}

#[test]
#[should_panic(expected = "RotationAlreadyExecuted")]
fn cancelled_rotation_cannot_be_approved() {
    let env = Env::default();
    let (client, admin) = setup(&env, 2);
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let approver = Address::generate(&env);

    client.set_signer(&admin, &old_signer, &1);
    client.set_signer(&admin, &approver, &1);
    let rotation_id = client.propose_signer_rotation(&admin, &old_signer, &new_signer);
    client.cancel_rotation(&admin, &rotation_id);

    client.approve_signer_rotation(&approver, &rotation_id);
}

#[test]
#[should_panic(expected = "Unauthorized")]
fn non_admin_cannot_cancel_rotation() {
    let env = Env::default();
    let (client, admin) = setup(&env, 2);
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);
    let non_admin = Address::generate(&env);

    client.set_signer(&admin, &old_signer, &1);
    let rotation_id = client.propose_signer_rotation(&admin, &old_signer, &new_signer);

    client.cancel_rotation(&non_admin, &rotation_id);
}

#[test]
#[should_panic(expected = "RotationAlreadyExecuted")]
fn executed_rotation_cannot_be_cancelled() {
    let env = Env::default();
    let (client, admin) = setup(&env, 1);
    let old_signer = Address::generate(&env);
    let new_signer = Address::generate(&env);

    client.set_signer(&admin, &old_signer, &1);
    let rotation_id = client.propose_signer_rotation(&admin, &old_signer, &new_signer);
    client.approve_signer_rotation(&admin, &rotation_id);

    client.cancel_rotation(&admin, &rotation_id);
}

#[test]
#[should_panic(expected = "RotationNotFound")]
fn cancel_missing_rotation_panics() {
    let env = Env::default();
    let (client, admin) = setup(&env, 2);

    client.cancel_rotation(&admin, &999);
}
