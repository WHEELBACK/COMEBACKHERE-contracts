// #4 and #25 audited that events get emitted at all for the right invoice and
// treasury (hold_settlement/release_hold) state transitions. This suite goes
// one step further for a representative, high-value sample of those events
// (settlement_executed, invoice_created, address_blocked) and asserts that
// the *data* published in each event is actually consistent with what is
// sitting in contract storage at the instant the event fires -- not a stale
// copy captured earlier in the function, and not a value mutated after the
// event was constructed but before it was published. A mismatch here would
// leave the contract's own internal state entirely correct while silently
// breaking any off-chain indexer that trusts event data as authoritative.
//
// Scope: this intentionally covers only the three events named in the issue
// as the highest-value representative sample. The remaining state-changing
// entrypoints across compliance/invoice/treasury (settlement_proposed,
// settlement_approved, invoice_paid, invoice_cancelled, address_allowed,
// hold_settlement/release_hold, token_allowed/token_removed, etc.) are NOT
// covered here and are a natural follow-up scope, not a silently dropped one.

use compliance::{ComplianceContract, ComplianceContractClient, DataKey as ComplianceDataKey};
use invoice::{
    DataKey as InvoiceDataKey, Invoice, InvoiceContract, InvoiceContractClient, MaybeAddress,
    MaybeBytes,
};
use soroban_sdk::{
    contract, contractimpl,
    testutils::{Address as _, Events},
    Address, Env, Symbol, TryFromVal, Val,
};
use treasury::{DataKey as TreasuryDataKey, Settlement, TreasuryContract, TreasuryContractClient};

// Minimal token stub -- these tests only need execute_settlement's transfer
// call to succeed, not to move real balances.
#[contract]
struct FakeToken;
#[contractimpl]
impl FakeToken {
    pub fn transfer(_env: Env, _from: Address, _to: Address, _amount: i128) {}
}

fn last_event_symbol(env: &Env) -> Symbol {
    let events = env.events().all();
    let (_, topics, _) = events.last().unwrap();
    Symbol::try_from_val(env, &topics.get_unchecked(0)).unwrap()
}

fn last_event_data(env: &Env) -> Val {
    let events = env.events().all();
    let (_, _, data) = events.last().unwrap();
    data.clone()
}

// ── settlement_executed round-trip ──────────────────────────────────────────

#[test]
fn settlement_executed_event_data_matches_storage_at_emission() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &treasury_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    let token_id = env.register_contract(None, FakeToken);

    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    client.execute_settlement(&admin, &sid, &token_id);

    assert_eq!(
        last_event_symbol(&env),
        Symbol::new(&env, "settlement_executed")
    );
    let event_settlement = Settlement::try_from_val(&env, &last_event_data(&env)).unwrap();

    // Read storage directly (not via a getter entrypoint that could take a
    // different code path) so the comparison is against exactly what is
    // persisted at the same point right after the call that published the
    // event, with no intervening transaction able to have mutated it.
    let env2 = env.clone();
    let stored_settlement: Settlement = env.as_contract(&treasury_id, || {
        env2.storage()
            .persistent()
            .get(&TreasuryDataKey::Settlement(sid))
            .unwrap()
    });

    assert_eq!(event_settlement, stored_settlement);
    assert_eq!(
        event_settlement.status,
        treasury::SettlementStatus::Executed
    );
}

#[test]
fn settlement_executed_event_data_matches_storage_for_each_of_several_settlements() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(&env, &treasury_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(&env));
    let token_id = env.register_contract(None, FakeToken);

    let merchant_a = Address::generate(&env);
    let merchant_b = Address::generate(&env);
    let sid_a = client.propose_settlement(&admin, &merchant_a, &1_000_000);
    let sid_b = client.propose_settlement(&admin, &merchant_b, &2_000_000);

    client.execute_settlement(&admin, &sid_a, &token_id);
    let event_a = Settlement::try_from_val(&env, &last_event_data(&env)).unwrap();
    client.execute_settlement(&admin, &sid_b, &token_id);
    let event_b = Settlement::try_from_val(&env, &last_event_data(&env)).unwrap();

    let env2 = env.clone();
    let stored_a: Settlement = env.as_contract(&treasury_id, || {
        env2.storage()
            .persistent()
            .get(&TreasuryDataKey::Settlement(sid_a))
            .unwrap()
    });
    let stored_b: Settlement = env.as_contract(&treasury_id, || {
        env2.storage()
            .persistent()
            .get(&TreasuryDataKey::Settlement(sid_b))
            .unwrap()
    });

    // Each event must match its own settlement's storage, not the other's --
    // a stale-copy bug would most plausibly surface as event_a accidentally
    // matching sid_b's state (or vice versa) when two mutations interleave.
    assert_eq!(event_a, stored_a);
    assert_eq!(event_b, stored_b);
    assert_ne!(event_a.id, event_b.id);
    assert_eq!(event_a.amount, 1_000_000);
    assert_eq!(event_b.amount, 2_000_000);
}

// ── invoice_created round-trip ──────────────────────────────────────────────

#[test]
fn invoice_created_event_data_matches_storage_at_emission() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let invoice_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &invoice_id);
    client.initialize(&admin);

    let id = client.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );

    assert_eq!(last_event_symbol(&env), Symbol::new(&env, "invoice_created"));
    let event_invoice = Invoice::try_from_val(&env, &last_event_data(&env)).unwrap();

    let env2 = env.clone();
    let stored_invoice: Invoice = env.as_contract(&invoice_id, || {
        env2.storage()
            .persistent()
            .get(&InvoiceDataKey::Invoice(id))
            .unwrap()
    });

    assert_eq!(event_invoice, stored_invoice);
    assert_eq!(event_invoice.id, id);
}

// ── address_blocked round-trip ──────────────────────────────────────────────

#[test]
fn address_blocked_event_data_matches_storage_at_emission() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let subject = Address::generate(&env);
    let compliance_id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &compliance_id);
    client.initialize(&admin);

    client.block_address(&admin, &subject, &None);

    assert_eq!(last_event_symbol(&env), Symbol::new(&env, "address_blocked"));
    let event_address = Address::try_from_val(&env, &last_event_data(&env)).unwrap();

    let env2 = env.clone();
    let stored_blocked: bool = env.as_contract(&compliance_id, || {
        env2.storage()
            .persistent()
            .get(&ComplianceDataKey::Blocked(event_address.clone()))
            .unwrap_or(false)
    });

    assert_eq!(event_address, subject);
    assert!(
        stored_blocked,
        "storage must show Blocked=true for the address named in the event, \
         read at the same point the event fired"
    );
}
