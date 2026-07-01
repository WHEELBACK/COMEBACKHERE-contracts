use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

#[test]
#[should_panic(expected = "RotationAlreadyExecuted")]
fn executed_rotation_prevents_further_approvals() {
    let env = Env::default();
    env.mock_all_auths();
    
    let admin = Address::generate(&env);
    let signer1 = Address::generate(&env);
    let signer2 = Address::generate(&env);
    let new_signer = Address::generate(&env);
    
    let treasury_id = env.register_contract(None, TreasuryContract);
    let treasury_client = TreasuryContractClient::new(&env, &treasury_id);
    treasury_client.initialize(&admin, &2, &soroban_sdk::Vec::new(&env));
    
    // Set up signers with weight 1 each
    treasury_client.set_signer(&admin, &signer1, &1);
    treasury_client.set_signer(&admin, &signer2, &1);
    
    // Propose rotation
    let rotation_id = treasury_client.propose_signer_rotation(&admin, &signer1, &new_signer);
    
    // Approve from signer2 to reach threshold and execute
    treasury_client.approve_signer_rotation(&signer2, &rotation_id);
    
    // Try to approve again - should panic with RotationAlreadyExecuted
    treasury_client.approve_signer_rotation(&signer1, &rotation_id);
}
