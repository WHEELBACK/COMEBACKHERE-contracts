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

// Property test: for a fixed signer set voting the same resolution direction via
// `vote_dispute_resolution`, the outcome (which vote index triggers resolution and
// the resulting status) must not depend on the order signers vote in — only on the
// configured threshold and the direction chosen.
mod dispute_vote_ordering_proptest {
    use super::*;
    use proptest::prelude::*;
    use treasury::DisputeStatus;

    const SIGNER_COUNT: usize = 5;
    const THRESHOLD: u32 = 3;

    fn setup_multi_signer(env: &Env) -> (TreasuryContractClient<'static>, Address, Vec<Address>) {
        env.mock_all_auths();
        let admin = Address::generate(env);
        let signers: Vec<Address> = (0..SIGNER_COUNT).map(|_| Address::generate(env)).collect();
        let mut signer_pairs = soroban_sdk::Vec::new(env);
        for signer in &signers {
            signer_pairs.push_back((signer.clone(), 1u32));
        }
        let id = env.register_contract(None, TreasuryContract);
        let client = TreasuryContractClient::new(env, &id);
        client.initialize(&admin, &THRESHOLD, &signer_pairs);
        (client, admin, signers)
    }

    proptest! {
        #[test]
        fn resolution_is_order_independent(
            order_keys in proptest::collection::vec(0u32..1_000, SIGNER_COUNT),
            in_favor_of_claimant in any::<bool>(),
        ) {
            let env = Env::default();
            let (client, admin, signers) = setup_multi_signer(&env);
            let merchant = Address::generate(&env);
            let claimant = Address::generate(&env);

            let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
            let dispute_id = client.raise_dispute(&claimant, &sid, &merchant, &5_000_000, &500);

            let mut order: Vec<usize> = (0..SIGNER_COUNT).collect();
            order.sort_by_key(|&i| order_keys[i]);

            let mut resolved_after: Option<usize> = None;
            for (cast, &idx) in order.iter().enumerate() {
                client.vote_dispute_resolution(&signers[idx], &dispute_id, &in_favor_of_claimant);
                if client.get_dispute(&dispute_id).status != DisputeStatus::Raised {
                    resolved_after = Some(cast + 1);
                    break;
                }
            }

            // Resolution must trigger exactly once cumulative distinct-signer weight
            // (1 per signer here) reaches THRESHOLD, regardless of vote order.
            prop_assert_eq!(resolved_after, Some(THRESHOLD as usize));

            let expected_status = if in_favor_of_claimant {
                DisputeStatus::ResolvedClaimant
            } else {
                DisputeStatus::ResolvedCounterparty
            };
            prop_assert_eq!(client.get_dispute(&dispute_id).status, expected_status);
        }
    }
}
