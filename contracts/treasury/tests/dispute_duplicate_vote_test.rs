//! Tests for duplicate-vote rejection in `vote_dispute_resolution` (#573).

use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{DisputeStatus, TreasuryContract, TreasuryContractClient, TreasuryError};

/// A threshold of 2 with two weight-1 signers means a single signer's vote is never
/// enough to auto-resolve — the dispute stays `Raised`, so a second vote from the same
/// signer must hit the duplicate check rather than `DisputeAlreadyResolved`.
fn setup(env: &Env) -> (TreasuryContractClient, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let other_signer = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    let mut signers = soroban_sdk::Vec::new(env);
    signers.push_back((other_signer.clone(), 1u32));
    client.initialize(&admin, &2, &signers);
    (client, admin, other_signer)
}

#[test]
fn second_vote_from_same_signer_is_rejected() {
    let env = Env::default();
    let (client, admin, _other_signer) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

    client.vote_dispute_resolution(&admin, &did, &true);
    // Weight 1 < threshold 2: the dispute is still open here.
    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Raised);

    let result = client.try_vote_dispute_resolution(&admin, &did, &true);
    assert_eq!(
        result,
        Err(Ok(TreasuryError::DuplicateVote)),
        "a second vote from the same signer must be rejected with a typed error"
    );
}

#[test]
fn duplicate_vote_does_not_double_count_weight() {
    let env = Env::default();
    let (client, admin, other_signer) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

    client.vote_dispute_resolution(&admin, &did, &true);
    // Repeat votes from the same signer must be rejected, not silently absorbed
    // with the dispute still open — this locks in that the weight truly never
    // moves past what a single genuine vote contributed.
    let _ = client.try_vote_dispute_resolution(&admin, &did, &true);
    let _ = client.try_vote_dispute_resolution(&admin, &did, &true);
    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Raised);

    // A genuinely distinct signer's vote is what actually reaches the threshold.
    client.vote_dispute_resolution(&other_signer, &did, &true);
    assert_eq!(
        client.get_dispute(&did).status,
        DisputeStatus::ResolvedClaimant
    );
}
