//! Tests for the pause reason (#606) and Merkle-root blocklist import (#608).

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

fn node(env: &Env, a: &BytesN<32>, b: &BytesN<32>) -> BytesN<32> {
    let (x, y) = if a.to_array() <= b.to_array() {
        (a, b)
    } else {
        (b, a)
    };
    let mut buf = Bytes::from_array(env, &x.to_array());
    buf.extend_from_array(&y.to_array());
    env.crypto().sha256(&buf).into()
}

#[test]
fn pause_stores_reason_and_unpause_clears_it() {
    let (_env, admin, client) = setup();
    assert_eq!(client.get_pause_reason(), None);
    client.pause(&admin, &symbol_short!("incident"));
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
