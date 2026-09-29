//! Tests pinning the behaviour of pending settlement approvals when signers rotate.
//!
//! # Current behaviour (as of this commit)
//!
//! Approval weights are **snapshotted at approval time**: `record_approval()` reads
//! `signer_weight()` live at the moment each individual signer calls
//! `approve_settlement` (or `propose_settlement`, which auto-approves the proposer).
//! The accumulated `Settlement.approval_weight` is then stored immutably on the
//! settlement struct in persistent storage.
//!
//! Because `execute_settlement` only checks
//! `settlement.approval_weight >= threshold`, **neither signer rotation, weight
//! reduction, nor removal retroactively changes the stored weight on existing
//! pending settlements**.  Concretely:
//!
//! 1. **Pre-rotation approvals still count.**  Weight accumulated by `old_signer`
//!    before a rotation executes is frozen in `settlement.approval_weight` and
//!    continues to count toward threshold even though `old_signer`'s live registry
//!    weight drops to 0 after the rotation.
//!
//! 2. **`old_signer` cannot approve new settlements after rotation.**  Once the
//!    rotation executes, `signer_weight(&env, old_signer) == 0`, so any attempt by
//!    `old_signer` to call `approve_settlement` (or `propose_settlement`) is
//!    rejected with `UnauthorizedSigner`.
//!
//! 3. **`new_signer` can approve settlements proposed before the rotation.**  The
//!    settlement's `approvals` list only contains `old_signer`'s address; because
//!    `new_signer` is a distinct address, `record_approval` does not treat it as a
//!    duplicate and will add `new_signer`'s weight to the accumulated total.
//!
//! 4. **`new_signer` cannot double-count weight on settlements already approved by
//!    `old_signer`.**  If the settlement is already approved by `old_signer` and
//!    `new_signer` approves too, the total weight accurately reflects both
//!    contributors (each address appears once).
//!
//! 5. **A settlement that reached threshold before rotation can still be executed
//!    after rotation**, as long as the accumulated `approval_weight` already meets
//!    the threshold; the rotation has no effect on the stored weight.
//!
//! 6. **A settlement that was below threshold before rotation may reach threshold
//!    after rotation** by `new_signer` adding its own approval on top of the
//!    pre-rotation weight snapshot.
//!
//! # Security implications
//!
//! - Rotating a signer does **not** automatically invalidate in-flight approvals
//!   given by the outgoing signer.  Any time a rotation is executed, operators
//!   should review pending settlements that already hold the outgoing signer's
//!   approval and, if those approvals should no longer count, force-cancel the
//!   affected settlements via `force_cancel_settlement`.
//!
//! - Similarly, the incoming `new_signer` immediately gains the ability to push
//!   pre-rotation settlements over the threshold.  If the operator intent is to
//!   require the *new* quorum to re-approve from scratch, affected pending
//!   settlements must be cancelled and re-proposed.
//!
//! Any intentional change to this behaviour MUST be deliberate, code-reviewed, and
//! reflected by updating this header comment accordingly.

use soroban_sdk::{testutils::Address as _, Address, Env, Vec};
use treasury::{RotationStatus, SettlementStatus, TreasuryContract, TreasuryContractClient};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Creates a treasury with:
/// - `admin` (weight 1 — also counted toward threshold)
/// - `signer_a` (weight 2)
/// - `signer_b` (weight 2)
/// - threshold = 2
///
/// Returns `(client, admin, signer_a, signer_b)`.
fn setup(env: &Env) -> (TreasuryContractClient<'_>, Address, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let signer_a = Address::generate(env);
    let signer_b = Address::generate(env);

    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &2, &Vec::new(env));
    client.set_signer(&admin, &signer_a, &2);
    client.set_signer(&admin, &signer_b, &2);
    (client, admin, signer_a, signer_b)
}

/// Executes a full signer rotation (`propose` + `approve` by a second signer so
/// the threshold is met). Returns the rotation id.
fn rotate(
    client: &TreasuryContractClient<'_>,
    proposer: &Address,
    second_approver: &Address,
    old_signer: &Address,
    new_signer: &Address,
) -> u64 {
    let rid = client.propose_signer_rotation(proposer, old_signer, new_signer);
    let proposal = client.approve_signer_rotation(second_approver, &rid);
    assert_eq!(
        proposal.status,
        RotationStatus::Executed,
        "rotation must have executed"
    );
    rid
}

// ── test 1: pre-rotation approval weight is frozen on the settlement ──────────

/// An approval given by `old_signer` BEFORE the rotation executes is permanently
/// frozen in `settlement.approval_weight`. After the rotation `old_signer`'s live
/// weight drops to 0, but the snapshot on the settlement is unchanged.
#[test]
fn pre_rotation_approval_weight_is_frozen_on_settlement() {
    let env = Env::default();
    let (client, admin, signer_a, signer_b) = setup(&env);
    let new_signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    // signer_a (weight 2) proposes a settlement — its weight is snapshotted.
    let sid = client.propose_settlement(&signer_a, &merchant, &1_000);

    // Sanity: approval_weight starts at 2 (signer_a's snapshot).
    let before = client.get_settlement(&sid);
    assert_eq!(before.approval_weight, 2);

    // Rotate signer_a out (admin proposes, signer_b approves → threshold met).
    rotate(&client, &admin, &signer_b, &signer_a, &new_signer);

    // signer_a's live weight is now 0 ...
    assert_eq!(client.get_signer_weight(&signer_a), 0);

    // ... but the settlement's snapshotted weight is still 2.
    let after = client.get_settlement(&sid);
    assert_eq!(
        after.approval_weight, 2,
        "pre-rotation weight snapshot must not change after rotation"
    );
    assert!(
        after.approvals.contains(&signer_a),
        "signer_a must remain in the approvals list"
    );
}

// ── test 2: old_signer cannot approve new settlements after rotation ──────────

/// Once the rotation executes, `old_signer` has weight 0 and is rejected with
/// `UnauthorizedSigner` for any call that requires signer authorization.
#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn old_signer_cannot_approve_settlement_after_rotation() {
    let env = Env::default();
    let (client, admin, signer_a, signer_b) = setup(&env);
    let new_signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    // Propose a settlement BEFORE the rotation.
    let sid = client.propose_settlement(&admin, &merchant, &1_000);

    // Rotate signer_a out.
    rotate(&client, &admin, &signer_b, &signer_a, &new_signer);

    // signer_a (now weight 0) tries to approve — must be rejected.
    client.approve_settlement(&signer_a, &sid);
}

// ── test 3: new_signer can approve a pre-rotation settlement ─────────────────

/// `new_signer` is a distinct address that was NOT in the settlement's `approvals`
/// list when the settlement was proposed, so `record_approval` lets it add its
/// weight after the rotation completes.
#[test]
fn new_signer_can_approve_settlement_proposed_before_rotation() {
    let env = Env::default();
    let (client, admin, signer_a, signer_b) = setup(&env);
    let new_signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    // admin proposes (weight 1 snapshotted).
    let sid = client.propose_settlement(&admin, &merchant, &1_000);

    // Rotate signer_a → new_signer (admin proposes, signer_b approves).
    rotate(&client, &admin, &signer_b, &signer_a, &new_signer);

    // new_signer (weight == signer_a's captured weight == 2) approves.
    let settlement = client.approve_settlement(&new_signer, &sid);

    assert!(
        settlement.approvals.contains(&new_signer),
        "new_signer must appear in the approvals list"
    );
    // weight: admin's 1 (at propose time) + new_signer's 2 (at approve time) = 3
    assert_eq!(
        settlement.approval_weight, 3,
        "new_signer's weight must be added on top of the pre-rotation snapshot"
    );
}

// ── test 4: both old_signer (pre) and new_signer (post) approvals accumulate ─

/// `old_signer` approves before the rotation; `new_signer` approves after.
/// Both contributions accumulate correctly — neither is dropped, neither
/// double-counts. `new_signer` is a different address so it is NOT treated as
/// a duplicate by `record_approval`.
#[test]
fn old_and_new_signer_approvals_both_accumulate() {
    let env = Env::default();
    let (client, admin, signer_a, signer_b) = setup(&env);
    let new_signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    // signer_a (weight 2) proposes — snapshot recorded.
    let sid = client.propose_settlement(&signer_a, &merchant, &1_000);

    // Rotate signer_a → new_signer.
    rotate(&client, &admin, &signer_b, &signer_a, &new_signer);

    // new_signer (weight 2) approves after the rotation.
    let settlement = client.approve_settlement(&new_signer, &sid);

    // signer_a's pre-rotation approval (2) + new_signer's post-rotation approval (2) = 4.
    assert_eq!(
        settlement.approval_weight, 4,
        "both pre- and post-rotation approvals must accumulate"
    );
    assert!(settlement.approvals.contains(&signer_a));
    assert!(settlement.approvals.contains(&new_signer));
}

// ── test 5: settlement that met threshold before rotation executes after ──────

/// The approval weight accumulated BEFORE the rotation is enough to meet the
/// threshold. After the rotation, `execute_settlement` must still succeed because
/// the stored `approval_weight` is unchanged.
#[test]
fn settlement_that_met_threshold_before_rotation_can_execute_after() {
    let env = Env::default();
    let (client, admin, signer_a, signer_b) = setup(&env);
    let new_signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    // Register a token and fund the treasury so execute_settlement can transfer.
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token_id.address();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);
    let treasury_address = client.address.clone();
    token_client.mint(&treasury_address, &100_000);

    // signer_a (weight 2) proposes a settlement, then signer_b also approves.
    // Combined weight = 2 + 2 = 4 >= threshold 2 → threshold is met.
    let sid = client.propose_settlement(&signer_a, &merchant, &1_000);
    client.approve_settlement(&signer_b, &sid);

    // Confirm threshold was met before rotation.
    let before = client.get_settlement(&sid);
    assert!(before.approval_weight >= 2, "threshold must already be met");

    // Rotate signer_a → new_signer.
    rotate(&client, &admin, &signer_b, &signer_a, &new_signer);

    // Execute with signer_b (which is still a valid signer).
    client.execute_settlement(&signer_b, &sid, &token_address);

    let after = client.get_settlement(&sid);
    assert_eq!(
        after.status,
        SettlementStatus::Executed,
        "settlement must be executed even though one approver was rotated out"
    );
}

// ── test 6: settlement below threshold before rotation can reach it after ─────

/// `old_signer` approved (weight 2), but threshold is 4.  After rotating
/// `old_signer` → `new_signer`, `new_signer` approves (weight 2), pushing the
/// total to 4 and satisfying the threshold.
#[test]
fn settlement_below_threshold_can_reach_it_via_new_signer_post_rotation() {
    let env = Env::default();
    // Use a higher threshold (4) so one signer alone cannot satisfy it.
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let signer_a = Address::generate(&env);
    let signer_b = Address::generate(&env);
    let new_signer = Address::generate(&env);

    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &id);
    client.initialize(&admin, &4, &Vec::new(&env));
    client.set_signer(&admin, &signer_a, &2);
    client.set_signer(&admin, &signer_b, &2);

    let merchant = Address::generate(&env);

    // Register a token and fund the treasury.
    let token_admin = Address::generate(&env);
    let token_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token_id.address();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &token_address);
    let treasury_address = client.address.clone();
    token_client.mint(&treasury_address, &100_000);

    // signer_a (weight 2) proposes — total = 2 < threshold 4.
    let sid = client.propose_settlement(&signer_a, &merchant, &1_000);
    let before = client.get_settlement(&sid);
    assert!(
        before.approval_weight < 4,
        "approval_weight must be below threshold before rotation"
    );

    // Rotate signer_a → new_signer (admin + signer_b approve the rotation; combined
    // weight = 1 + 2 = 3 … but threshold is 4! We need a third approval.)
    // Instead let signer_b propose the rotation and admin approves — still only 1+2=3.
    // Use the higher-weight setup: give signer_b weight 3 so admin(1)+signer_b(3)=4.
    client.set_signer(&admin, &signer_b, &3);
    let rid = client.propose_signer_rotation(&admin, &signer_a, &new_signer);
    let proposal = client.approve_signer_rotation(&signer_b, &rid);
    assert_eq!(
        proposal.status,
        RotationStatus::Executed,
        "rotation must execute"
    );

    // new_signer inherits signer_a's captured weight (2). Now new_signer approves
    // the pending settlement, adding weight 2 → total = 2 + 2 = 4 >= threshold 4.
    client.approve_settlement(&new_signer, &sid);
    let mid = client.get_settlement(&sid);
    assert!(
        mid.approval_weight >= 4,
        "approval_weight must meet threshold after new_signer approves"
    );

    // Execute must succeed.
    client.execute_settlement(&new_signer, &sid, &token_address);
    let after = client.get_settlement(&sid);
    assert_eq!(
        after.status,
        SettlementStatus::Executed,
        "settlement must be executed after new_signer's post-rotation approval"
    );
}

// ── test 7: old_signer cannot propose new settlements after rotation ──────────

/// After rotation, `old_signer` has weight 0 and `propose_settlement` (which calls
/// `require_authorized_signer`) must reject it with `UnauthorizedSigner`.
#[test]
#[should_panic(expected = "Error(Contract, #10)")]
fn old_signer_cannot_propose_settlement_after_rotation() {
    let env = Env::default();
    let (client, admin, signer_a, signer_b) = setup(&env);
    let new_signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    rotate(&client, &admin, &signer_b, &signer_a, &new_signer);

    // signer_a (now weight 0) tries to propose — must be rejected.
    client.propose_settlement(&signer_a, &merchant, &1_000);
}

// ── test 8: new_signer cannot re-add weight that old_signer already counted ──

/// If `old_signer` already approved a settlement AND `new_signer` also approves
/// after the rotation, the total weight must reflect both addresses once each —
/// it must NOT be double-counted just because both addresses belonged to the
/// "same logical signer" at different times.
#[test]
fn new_signer_weight_does_not_double_count_old_signer_approval() {
    let env = Env::default();
    let (client, admin, signer_a, signer_b) = setup(&env);
    let new_signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    // signer_a (weight 2) proposes; signer_b (weight 2) approves → total = 4.
    let sid = client.propose_settlement(&signer_a, &merchant, &1_000);
    client.approve_settlement(&signer_b, &sid);

    // Rotate signer_a → new_signer.
    rotate(&client, &admin, &signer_b, &signer_a, &new_signer);

    // new_signer (weight 2) approves — it is NOT in the approvals list so weight
    // is added once: 4 + 2 = 6. signer_a is still in the list but its live weight
    // is now 0; the new approval only counts new_signer's current weight.
    let settlement = client.approve_settlement(&new_signer, &sid);

    assert_eq!(
        settlement.approval_weight, 6,
        "new_signer adds its own weight; old_signer's frozen snapshot is preserved"
    );
    assert!(settlement.approvals.contains(&signer_a));
    assert!(settlement.approvals.contains(&new_signer));
    // signer_b is also still listed (approved before the rotation).
    assert!(settlement.approvals.contains(&signer_b));
}

// ── test 9: rotation does not affect approval_weight on multiple settlements ──

/// A rotation must affect only the live signer registry; ALL pre-existing
/// settlement approval snapshots must remain unchanged regardless of how many
/// settlements are pending.
#[test]
fn rotation_does_not_affect_approval_weight_on_multiple_settlements() {
    let env = Env::default();
    let (client, admin, signer_a, signer_b) = setup(&env);
    let new_signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    // Propose three settlements with different approval states.
    let sid1 = client.propose_settlement(&signer_a, &merchant, &1_000); // weight 2
    let sid2 = client.propose_settlement(&signer_b, &merchant, &2_000); // weight 2
    let sid3 = client.propose_settlement(&admin, &merchant, &3_000); // weight 1
                                                                     // Give sid3 an extra approval from signer_a.
    client.approve_settlement(&signer_a, &sid3); // sid3 weight → 3

    let w1_before = client.get_settlement(&sid1).approval_weight;
    let w2_before = client.get_settlement(&sid2).approval_weight;
    let w3_before = client.get_settlement(&sid3).approval_weight;

    // Rotate signer_a out.
    rotate(&client, &admin, &signer_b, &signer_a, &new_signer);

    // All three snapshots must be identical after the rotation.
    assert_eq!(
        client.get_settlement(&sid1).approval_weight,
        w1_before,
        "settlement 1 approval_weight must be unchanged after rotation"
    );
    assert_eq!(
        client.get_settlement(&sid2).approval_weight,
        w2_before,
        "settlement 2 approval_weight must be unchanged after rotation"
    );
    assert_eq!(
        client.get_settlement(&sid3).approval_weight,
        w3_before,
        "settlement 3 approval_weight must be unchanged after rotation"
    );
}
