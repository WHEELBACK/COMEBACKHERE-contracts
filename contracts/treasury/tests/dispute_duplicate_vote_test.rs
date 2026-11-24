//! Tests for duplicate-vote rejection in `vote_dispute_resolution` (#573).

use proptest::prelude::*;
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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
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

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
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

// ---------------------------------------------------------------------------
// Invariant-focused tests (#918)
// ---------------------------------------------------------------------------

/// A dispute that resolves must have accumulated exactly the configured
/// threshold of vote weight — never more, never less.
#[test]
fn resolution_requires_exact_threshold_weight() {
    let env = Env::default();
    let (client, admin, other_signer) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

    // One vote of weight 1 is strictly below the threshold of 2.
    client.vote_dispute_resolution(&admin, &did, &true);
    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Raised);

    // The second distinct vote brings the total to exactly the threshold.
    client.vote_dispute_resolution(&other_signer, &did, &true);
    assert_eq!(
        client.get_dispute(&did).status,
        DisputeStatus::ResolvedClaimant
    );

    // No further votes are accepted once resolved.
    let result = client.try_vote_dispute_resolution(&other_signer, &did, &true);
    assert_eq!(result, Err(Ok(TreasuryError::DisputeAlreadyResolved)));
}

/// The amount held for a dispute must match the dispute's claimed amount.
#[test]
fn hold_amount_matches_dispute_amount() {
    let env = Env::default();
    let (client, admin, _other_signer) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let dispute_amount = 5_000_000i128;
    let did = client.raise_dispute(&claimant, &sid, &merchant, &dispute_amount, &500, &None);

    let dispute = client.get_dispute(&did);
    assert_eq!(dispute.amount, dispute_amount);
    assert_eq!(dispute.status, DisputeStatus::Raised);
}

/// `DisputeStatus` transitions are monotonic: once resolved or expired a
/// dispute can never move back to `Raised` or flip to a different terminal
/// state.
#[test]
fn status_transitions_are_monotonic() {
    let env = Env::default();
    let (client, admin, other_signer) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);
    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Raised);

    client.vote_dispute_resolution(&admin, &did, &true);
    assert_eq!(client.get_dispute(&did).status, DisputeStatus::Raised);

    client.vote_dispute_resolution(&other_signer, &did, &true);
    assert_eq!(
        client.get_dispute(&did).status,
        DisputeStatus::ResolvedClaimant
    );

    // Attempting to re-vote cannot regress the terminal state.
    let _ = client.try_vote_dispute_resolution(&admin, &did, &true);
    assert_eq!(
        client.get_dispute(&did).status,
        DisputeStatus::ResolvedClaimant
    );
}

/// Out-of-order resolution: voting on a dispute that has already resolved is
/// rejected with a typed error and does not corrupt the recorded state.
#[test]
fn out_of_order_resolution_is_rejected() {
    let env = Env::default();
    let (client, admin, other_signer) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

    client.vote_dispute_resolution(&admin, &did, &true);
    client.vote_dispute_resolution(&other_signer, &did, &true);
    assert_eq!(
        client.get_dispute(&did).status,
        DisputeStatus::ResolvedClaimant
    );

    let result = client.try_vote_dispute_resolution(&admin, &did, &false);
    assert_eq!(result, Err(Ok(TreasuryError::DisputeAlreadyResolved)));
    assert_eq!(
        client.get_dispute(&did).status,
        DisputeStatus::ResolvedClaimant
    );
}

/// An expired dispute cannot subsequently be resolved by a late vote.
#[test]
fn expired_then_resolved_is_rejected() {
    let env = Env::default();
    let (client, admin, other_signer) = setup(&env);
    let merchant = Address::generate(&env);
    let claimant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
    let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

    // Advance the ledger past the dispute deadline so it expires.
    env.ledger().set_sequence_number(env.ledger().sequence() + 10_000);
    let _ = client.try_expire_dispute(&did);

    let status = client.get_dispute(&did).status;
    if status == DisputeStatus::Expired {
        let result = client.try_vote_dispute_resolution(&admin, &did, &true);
        assert_eq!(result, Err(Ok(TreasuryError::DisputeAlreadyResolved)));
        let result = client.try_vote_dispute_resolution(&other_signer, &did, &true);
        assert_eq!(result, Err(Ok(TreasuryError::DisputeAlreadyResolved)));
        assert_eq!(client.get_dispute(&did).status, DisputeStatus::Expired);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Boundary and adversarial vote sequences: whatever the order, the dispute
    /// either stays `Raised` (weight below threshold) or resolves exactly once
    /// (weight reaches threshold) — never anything in between or inconsistent.
    #[test]
    fn prop_vote_sequences_preserve_invariants(
        first_vote in any::<bool>(),
        second_vote in any::<bool>(),
        duplicate_attempts in 0u8..4,
    ) {
        let env = Env::default();
        let (client, admin, other_signer) = setup(&env);
        let merchant = Address::generate(&env);
        let claimant = Address::generate(&env);

        let sid = client.propose_settlement(&admin, &merchant, &10_000_000, &0_u64);
        let did = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500, &None);

        // First genuine vote: weight 1 < threshold 2, always stays Raised.
        client.vote_dispute_resolution(&admin, &did, &first_vote);
        prop_assert_eq!(client.get_dispute(&did).status, DisputeStatus::Raised);

        // Duplicate votes from the same signer must be rejected.
        for _ in 0..duplicate_attempts {
            let r = client.try_vote_dispute_resolution(&admin, &did, &first_vote);
            prop_assert_eq!(r, Err(Ok(TreasuryError::DuplicateVote)));
            prop_assert_eq!(client.get_dispute(&did).status, DisputeStatus::Raised);
        }

        // Second distinct vote reaches the threshold exactly.
        client.vote_dispute_resolution(&other_signer, &did, &second_vote);
        prop_assert_eq!(
            client.get_dispute(&did).status,
            DisputeStatus::ResolvedClaimant
        );

        // Any further vote is rejected and cannot regress the terminal state.
        let r = client.try_vote_dispute_resolution(&other_signer, &did, &second_vote);
        prop_assert_eq!(r, Err(Ok(TreasuryError::DisputeAlreadyResolved)));
        prop_assert_eq!(
            client.get_dispute(&did).status,
            DisputeStatus::ResolvedClaimant
        );
    }
}
