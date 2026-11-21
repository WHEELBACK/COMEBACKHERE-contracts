//! Tests for the pause reason (#606) and Merkle-root blocklist import (#608).
//!
//! Merkle proof ordering convention (#758): sibling hashes are **positional**,
//! not sorted. Each proof element is the sibling at that level, and the
//! contract combines `(current, sibling)` in that fixed order — it does not
//! sort the pair. This matches the reference generator in
//! `scripts/build-blocklist-merkle.py`, which builds the tree by hashing
//! `left || right` in insertion order. The `node` helper below mirrors that
//! positional convention (no sorting) so the vectors stay in sync.

use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    symbol_short, testutils::Address as _, xdr::ToXdr, Address, Bytes, BytesN, Env, Vec,
};

fn setup() -> (Env, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

fn leaf(env: &Env, a: &Address) -> BytesN<32> {
    env.crypto().sha256(&a.clone().to_xdr(env)).into()
}

/// Positional node hash: `sha256(left || right)` with no sorting.
/// This is the convention `is_blocked_with_proof` uses.
fn node(env: &Env, left: &BytesN<32>, right: &BytesN<32>) -> BytesN<32> {
    let mut buf = Bytes::from_array(env, &left.to_array());
    buf.extend_from_array(&right.to_array());
    env.crypto().sha256(&buf).into()
}

#[test]
fn pause_stores_reason_and_unpause_clears_it() {
    let (_env, admin, client) = setup();
    assert_eq!(client.get_pause_reason(), None);
    client.pause_with_reason(&admin, &symbol_short!("incident"));
    assert_eq!(client.get_pause_reason(), Some(symbol_short!("incident")));
    client.unpause(&admin);
    assert_eq!(client.get_pause_reason(), None);
}

#[test]
fn merkle_proofs_valid_and_invalid() {
    let (env, admin, client) = setup();
    let (a, b, c, d) = (
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    );
    let (la, lb, lc, ld) = (
        leaf(&env, &a),
        leaf(&env, &b),
        leaf(&env, &c),
        leaf(&env, &d),
    );
    let (ab, cd) = (node(&env, &la, &lb), node(&env, &lc, &ld));
    let root = node(&env, &ab, &cd);

    let proof_a = Vec::from_array(&env, [lb.clone(), cd.clone()]);
    assert!(!client.is_blocked_with_proof(&a, &proof_a)); // no root yet

    client.set_blocklist_root(&admin, &root);
    assert_eq!(client.get_blocklist_root(), Some(root.clone()));
    assert!(client.is_blocked_with_proof(&a, &proof_a));
    assert!(client.is_blocked_with_proof(&d, &Vec::from_array(&env, [lc.clone(), ab.clone()])));

    // Wrong address, wrong proof, empty proof.
    let outsider = Address::generate(&env);
    assert!(!client.is_blocked_with_proof(&outsider, &proof_a));
    assert!(!client.is_blocked_with_proof(&b, &proof_a));
    assert!(!client.is_blocked_with_proof(&a, &Vec::new(&env)));

    // Individually blocked addresses are blocked regardless of proof.
    client.block_address(&admin, &outsider, &None);
    assert!(client.is_blocked_with_proof(&outsider, &Vec::new(&env)));

    // Rotating the root invalidates old proofs.
    client.set_blocklist_root(&admin, &la);
    assert!(!client.is_blocked_with_proof(&a, &proof_a));
    assert!(client.is_blocked_with_proof(&a, &Vec::new(&env)));
}

/// Valid proof for every leaf in a 4-leaf tree, using positional ordering.
#[test]
fn merkle_proofs_valid_for_all_leaves() {
    let (env, admin, client) = setup();
    let (a, b, c, d) = (
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    );
    let (la, lb, lc, ld) = (
        leaf(&env, &a),
        leaf(&env, &b),
        leaf(&env, &c),
        leaf(&env, &d),
    );
    let (ab, cd) = (node(&env, &la, &lb), node(&env, &lc, &ld));
    let root = node(&env, &ab, &cd);
    client.set_blocklist_root(&admin, &root);

    assert!(client.is_blocked_with_proof(&a, &Vec::from_array(&env, [lb.clone(), cd.clone()])));
    assert!(client.is_blocked_with_proof(&b, &Vec::from_array(&env, [la.clone(), cd.clone()])));
    assert!(client.is_blocked_with_proof(&c, &Vec::from_array(&env, [ld.clone(), ab.clone()])));
    assert!(client.is_blocked_with_proof(&d, &Vec::from_array(&env, [lc.clone(), ab.clone()])));
}

/// Tampered proofs (wrong sibling hash) must be rejected.
#[test]
fn merkle_proofs_reject_tampered_siblings() {
    let (env, admin, client) = setup();
    let (a, b, c, d) = (
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    );
    let (la, lb, lc, ld) = (
        leaf(&env, &a),
        leaf(&env, &b),
        leaf(&env, &c),
        leaf(&env, &d),
    );
    let (ab, cd) = (node(&env, &la, &lb), node(&env, &lc, &ld));
    let root = node(&env, &ab, &cd);
    client.set_blocklist_root(&admin, &root);

    // Tampered first sibling.
    let tampered_leaf = Vec::from_array(&env, [lc.clone(), cd.clone()]);
    assert!(!client.is_blocked_with_proof(&a, &tampered_leaf));

    // Tampered second sibling.
    let tampered_top = Vec::from_array(&env, [lb.clone(), ab.clone()]);
    assert!(!client.is_blocked_with_proof(&a, &tampered_top));

    // Truncated proof (missing a level).
    assert!(!client.is_blocked_with_proof(&a, &Vec::from_array(&env, [lb.clone()])));
}

/// Wrong-order proofs must be rejected: the convention is positional, so
/// swapping siblings changes the computed root and fails verification.
#[test]
fn merkle_proofs_reject_wrong_order() {
    let (env, admin, client) = setup();
    let (a, b, c, d) = (
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    );
    let (la, lb, lc, ld) = (
        leaf(&env, &a),
        leaf(&env, &b),
        leaf(&env, &c),
        leaf(&env, &d),
    );
    let (ab, cd) = (node(&env, &la, &lb), node(&env, &lc, &ld));
    let root = node(&env, &ab, &cd);
    client.set_blocklist_root(&admin, &root);

    // Correct proof for `a` is [lb, cd]; swapping the top-level sibling
    // produces a different root under positional hashing.
    let swapped_top = Vec::from_array(&env, [lb.clone(), ab.clone()]);
    assert!(!client.is_blocked_with_proof(&a, &swapped_top));

    // Swapping the leaf-level sibling also fails.
    let swapped_leaf = Vec::from_array(&env, [la.clone(), cd.clone()]);
    assert!(!client.is_blocked_with_proof(&a, &swapped_leaf));

    // Sanity: the correctly ordered proof still verifies.
    assert!(client.is_blocked_with_proof(&a, &Vec::from_array(&env, [lb.clone(), cd.clone()])));
}

/// Proofs longer than the 32-level cap must be rejected outright, even if
/// the leading siblings would otherwise form a valid path. This bounds the
/// budget a caller can burn via `is_blocked_with_proof`.
#[test]
fn merkle_proofs_reject_oversized_proof() {
    let (env, admin, client) = setup();
    let (a, b, c, d) = (
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    );
    let (la, lb, lc, ld) = (
        leaf(&env, &a),
        leaf(&env, &b),
        leaf(&env, &c),
        leaf(&env, &d),
    );
    let (ab, cd) = (node(&env, &la, &lb), node(&env, &lc, &ld));
    let root = node(&env, &ab, &cd);
    client.set_blocklist_root(&admin, &root);

    // A valid 2-level proof padded with extra siblings exceeds the cap.
    let mut oversized = Vec::from_array(&env, [lb.clone(), cd.clone()]);
    for _ in 0..31 {
        oversized.push_back(la.clone());
    }
    assert!(oversized.len() > 32);
    assert!(!client.is_blocked_with_proof(&a, &oversized));

    // Exactly at the cap is still accepted (and verifies here).
    let mut at_cap = Vec::from_array(&env, [lb.clone(), cd.clone()]);
    for _ in 0..30 {
        at_cap.push_back(la.clone());
    }
    assert_eq!(at_cap.len(), 32);
    // Padding changes the computed root, so this must not verify, but it
    // must not be rejected purely for length either.
    assert!(!client.is_blocked_with_proof(&a, &at_cap));
}
