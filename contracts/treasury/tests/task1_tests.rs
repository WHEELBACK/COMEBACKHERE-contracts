/// Tests for task1.md items:
/// #116 - Cancellation before execution
/// #117 - Event ordering snapshots
/// #118 - Signer rotation scenario coverage
/// #120 - Execute on non-existent settlement typed failure
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Events},
    Address, Env, FromVal, Symbol,
};
use treasury::{SettlementStatus, TreasuryContract, TreasuryContractClient};

fn setup(env: &Env, threshold: u32) -> (TreasuryContractClient, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &threshold, &soroban_sdk::Vec::new(env));
    (client, admin, id)
}

// Minimal token stub for execute_settlement tests
#[contract]
struct FakeToken;
#[contractimpl]
impl FakeToken {
    pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {}
}

// ─── #116 Cancellation before execution ──────────────────────────────────────

#[test]
fn cancel_pending_settlement_removes_it_from_pending() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 1);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.cancel_settlement(&admin, &sid);

    assert_eq!(client.get_pending_settlements().len(), 0);
}

#[test]
#[should_panic(expected = "AlreadyExecuted")]
fn execute_after_cancel_panics() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 1);
    let merchant = Address::generate(&env);
    let token_id = env.register_contract(None, FakeToken);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.cancel_settlement(&admin, &sid);
    client.execute_settlement(&admin, &sid, &token_id);
}

#[test]
#[should_panic(expected = "AlreadyExecuted")]
fn approve_after_cancel_panics() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 2);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.cancel_settlement(&admin, &sid);
    client.approve_settlement(&admin, &sid);
}

#[test]
#[should_panic(expected = "AlreadyExecuted")]
fn double_cancel_panics() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 1);
    let merchant = Address::generate(&env);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.cancel_settlement(&admin, &sid);
    client.cancel_settlement(&admin, &sid);
}

// ─── #117 Event ordering snapshots ───────────────────────────────────────────

fn event_symbol(env: &Env, topics: &soroban_sdk::Vec<soroban_sdk::Val>) -> String {
    let val = topics.get_unchecked(0);
    Symbol::from_val(env, &val).to_string()
}

#[test]
fn event_order_propose_approve_cancel() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 2);
    let backup = Address::generate(&env);
    let merchant = Address::generate(&env);
    client.set_signer(&admin, &backup, &1);

    let sid = client.propose_settlement(&admin, &merchant, &5_000_000);
    client.approve_settlement(&backup, &sid);
    client.cancel_settlement(&admin, &sid);

    let events = env.events().all();
    let symbols: std::vec::Vec<String> = events
        .iter()
        .map(|(_, topics, _)| event_symbol(&env, &topics))
        .collect();

    // Find positions of the three key events
    let proposed = symbols
        .iter()
        .position(|s| s == "settlement_proposed")
        .unwrap();
    let approved = symbols
        .iter()
        .position(|s| s == "settlement_approved")
        .unwrap();
    let cancelled = symbols
        .iter()
        .position(|s| s == "settlement_cancelled")
        .unwrap();

    assert!(proposed < approved, "proposed must come before approved");
    assert!(approved < cancelled, "approved must come before cancelled");
}

#[test]
fn event_order_propose_then_execute() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 1);
    let merchant = Address::generate(&env);
    let token_id = env.register_contract(None, FakeToken);

    let sid = client.propose_settlement(&admin, &merchant, &1_000);
    client.execute_settlement(&admin, &sid, &token_id);

    let events = env.events().all();
    let symbols: std::vec::Vec<String> = events
        .iter()
        .map(|(_, topics, _)| event_symbol(&env, &topics))
        .collect();

    let proposed = symbols
        .iter()
        .position(|s| s == "settlement_proposed")
        .unwrap();
    let executed = symbols
        .iter()
        .position(|s| s == "settlement_executed")
        .unwrap();

    assert!(proposed < executed, "proposed must come before executed");
}

// ─── #118 Signer rotation scenario coverage ──────────────────────────────────

#[test]
fn rotated_in_signer_can_propose() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 1);
    let new_signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    client.set_signer(&admin, &new_signer, &1);
    let sid = client.propose_settlement(&new_signer, &merchant, &1_000);
    assert_eq!(sid, 1);
}

#[test]
#[should_panic(expected = "UnauthorizedSigner")]
fn rotated_out_signer_cannot_propose() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 1);
    let old_signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    client.set_signer(&admin, &old_signer, &1);
    client.set_signer(&admin, &old_signer, &0); // rotate out
    client.propose_settlement(&old_signer, &merchant, &1_000);
}

#[test]
#[should_panic(expected = "UnauthorizedSigner")]
fn rotated_out_signer_cannot_approve() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 2);
    let signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    client.set_signer(&admin, &signer, &1);
    let sid = client.propose_settlement(&admin, &merchant, &1_000);
    client.set_signer(&admin, &signer, &0);
    client.approve_settlement(&signer, &sid);
}

#[test]
fn rotation_after_approval_preserves_weight_snapshot() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 2);
    let signer_b = Address::generate(&env);
    let merchant = Address::generate(&env);

    client.set_signer(&admin, &signer_b, &1);
    let sid = client.propose_settlement(&admin, &merchant, &1_000);
    client.approve_settlement(&signer_b, &sid);

    // rotate signer_b out after approval
    client.set_signer(&admin, &signer_b, &0);

    let pending = client.get_pending_settlements();
    assert_eq!(pending.get(0).unwrap().approval_weight, 2);
}

#[test]
fn new_signer_can_approve_after_rotation() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 2);
    let new_signer = Address::generate(&env);
    let merchant = Address::generate(&env);

    client.set_signer(&admin, &new_signer, &1);
    let sid = client.propose_settlement(&admin, &merchant, &1_000);
    let settlement = client.approve_settlement(&new_signer, &sid);

    assert_eq!(settlement.approval_weight, 2);
    assert_eq!(settlement.approvals.len(), 2);
}

// ─── #120 Execute on non-existent settlement typed failure ───────────────────

#[test]
#[should_panic(expected = "SettlementNotFound")]
fn execute_nonexistent_settlement_panics() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 1);
    let token = Address::generate(&env);
    client.execute_settlement(&admin, &9999, &token);
}

#[test]
#[should_panic(expected = "SettlementNotFound")]
fn approve_nonexistent_settlement_panics() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 1);
    client.approve_settlement(&admin, &9999);
}

#[test]
#[should_panic(expected = "SettlementNotFound")]
fn cancel_nonexistent_settlement_panics() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 1);
    client.cancel_settlement(&admin, &9999);
}

#[test]
fn try_execute_nonexistent_settlement_returns_err() {
    let env = Env::default();
    let (client, admin, _) = setup(&env, 1);
    let token = Address::generate(&env);
    assert!(client
        .try_execute_settlement(&admin, &9999, &token)
        .is_err());
}
