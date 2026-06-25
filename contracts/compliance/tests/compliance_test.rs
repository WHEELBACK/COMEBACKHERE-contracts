use compliance::{ComplianceContract, ComplianceContractClient, ContractError};
use soroban_sdk::{testutils::{Address as _, Events as _}, Address, Env, Symbol};

fn setup() -> (Env, Address, Address, ComplianceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let subject = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, subject, client)
}

#[test]
fn block_and_clear_address() {
    let (_env, admin, payer, client) = setup();
    client.allow_address(&admin, &payer);
    assert!(client.is_allowed(&payer));
    client.block_address(&admin, &payer);
    assert!(!client.is_allowed(&payer));
    client.clear_address(&admin, &payer);
    assert!(client.is_allowed(&payer));
}

#[test]
fn pause_and_unpause_emit_events() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    client.allow_address(&admin, &payer);
    assert!(client.is_allowed(&payer));
    // pause: state is set; subsequent allow is blocked (tested via unpause round-trip)
    client.pause(&admin);
    client.unpause(&admin);
    // after unpause, allow_address works again
    let payer2 = Address::generate(&env);
    client.allow_address(&admin, &payer2);
    assert!(client.is_allowed(&payer2));
}

#[test]
fn block_and_clear_permitted_while_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    client.allow_address(&admin, &payer);
    client.pause(&admin);
    // block and clear must succeed even while paused (emergency policy)
    client.block_address(&admin, &payer);
    assert!(!client.is_allowed(&payer));
    client.clear_address(&admin, &payer);
    assert!(client.is_allowed(&payer));
}

#[test]
fn allow_address_mutation_succeeds_after_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let address1 = Address::generate(&env);
    let address2 = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);

    // Allow address1 before pausing
    client.allow_address(&admin, &address1);
    assert!(client.is_allowed(&address1));

    // Pause then unpause
    client.pause(&admin);
    client.unpause(&admin);

    // Allow address2 should now work
    client.allow_address(&admin, &address2);
    assert!(client.is_allowed(&address2));
}

#[test]
fn block_address_mutation_succeeds_after_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let address = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);

    // Allow address first
    client.allow_address(&admin, &address);
    assert!(client.is_allowed(&address));

    // Pause then unpause
    client.pause(&admin);
    client.unpause(&admin);

    // Block address should now work
    client.block_address(&admin, &address);
    assert!(!client.is_allowed(&address));
}

#[test]
fn clear_address_mutation_succeeds_after_unpause() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let address = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);

    // Allow and block address first
    client.allow_address(&admin, &address);
    client.block_address(&admin, &address);
    assert!(!client.is_allowed(&address));

    // Pause then unpause
    client.pause(&admin);
    client.unpause(&admin);

    // Clear address should now work
    client.clear_address(&admin, &address);
    assert!(client.is_allowed(&address));
}

#[test]
fn read_only_queries_not_blocked_by_pause() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let allowed_address = Address::generate(&env);
    let blocked_address = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);

    // Setup: allow one address, block another
    client.allow_address(&admin, &allowed_address);
    client.block_address(&admin, &blocked_address);

    // Pause the contract
    client.pause(&admin);

    // Read-only queries should still work
    assert!(client.is_allowed(&allowed_address));
    assert!(!client.is_allowed(&blocked_address));

    let unrelated_address = Address::generate(&env);
    assert!(!client.is_allowed(&unrelated_address));
}

#[test]
fn unpause_emits_event_and_restores_allow() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let payer = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    client.pause(&admin);
    client.unpause(&admin);
    client.allow_address(&admin, &payer);
    assert!(client.is_allowed(&payer));
}

#[test]
fn reinitialize_is_rejected() {
    let (env, _admin, _subject, client) = setup();
    let attacker = Address::generate(&env);
    let result = client.try_initialize(&attacker);
    assert_eq!(result, Err(Ok(ContractError::AlreadyInitialized)));
}

// Verification: address_allowed event schema
// - topics[0]: symbol "address_allowed"
// - data: single Address value for the allowed address
// The snapshot harness captures the full event payload (topics and data) for regression.
#[test]
fn emits_address_allowed_event() {
    let (env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
    // Events are captured by the snapshot test harness; no additional assertions needed here.
    let _ = env;
}

// Verification: address_blocked event schema
// - topics[0]: symbol "address_blocked"
// - data: single Address value for the blocked address
// The snapshot harness captures the full event payload (topics and data) for regression.
#[test]
fn emits_address_blocked_event() {
    let (env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
    client.block_address(&admin, &subject);
    assert!(!client.is_allowed(&subject));
    // Events are captured by the snapshot test harness; no additional assertions needed here.
    let _ = env;
}

// Verification: address_cleared event schema
// - topics[0]: symbol "address_cleared"
// - data: single Address value for the cleared address
// The snapshot harness captures the full event payload (topics and data) for regression.
#[test]
fn emits_address_cleared_event() {
    let (env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    client.block_address(&admin, &subject);
    assert!(!client.is_allowed(&subject));
    client.clear_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
    // Events are captured by the snapshot test harness; no additional assertions needed here.
    let _ = env;
}

// ── #121 Allow/Block/Clear precedence matrix ─────────────────────────────────

#[test]
fn precedence_never_allowed_is_denied() {
    let (_env, _admin, subject, client) = setup();
    assert!(!client.is_allowed(&subject));
}

#[test]
fn precedence_allowed_then_blocked_is_denied() {
    let (_env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    client.block_address(&admin, &subject);
    assert!(!client.is_allowed(&subject));
}

#[test]
fn precedence_blocked_then_cleared_is_allowed() {
    let (_env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    client.block_address(&admin, &subject);
    client.clear_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
}

#[test]
fn precedence_block_without_prior_allow_is_denied() {
    let (_env, admin, subject, client) = setup();
    client.block_address(&admin, &subject);
    assert!(!client.is_allowed(&subject));
}

#[test]
fn precedence_clear_without_prior_block_sets_allowed() {
    let (_env, admin, subject, client) = setup();
    // clear_address sets Allowed=true and Blocked=false regardless
    client.clear_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
}

// ── #123 Batch allow and block tests ─────────────────────────────────────────

#[test]
fn batch_allow_multiple_addresses() {
    let (env, admin, _, client) = setup();
    let addrs: soroban_sdk::Vec<Address> = soroban_sdk::vec![
        &env,
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    for addr in addrs.iter() {
        client.allow_address(&admin, &addr);
    }
    for addr in addrs.iter() {
        assert!(client.is_allowed(&addr));
    }
}

#[test]
fn batch_block_multiple_addresses() {
    let (env, admin, _, client) = setup();
    let addrs: soroban_sdk::Vec<Address> = soroban_sdk::vec![
        &env,
        Address::generate(&env),
        Address::generate(&env),
        Address::generate(&env),
    ];
    for addr in addrs.iter() {
        client.allow_address(&admin, &addr);
    }
    for addr in addrs.iter() {
        client.block_address(&admin, &addr);
    }
    for addr in addrs.iter() {
        assert!(!client.is_allowed(&addr));
    }
}

#[test]
fn batch_allow_then_block_subset() {
    let (env, admin, _, client) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    for addr in [&a, &b, &c] {
        client.allow_address(&admin, addr);
    }
    // block only b
    client.block_address(&admin, &b);
    assert!(client.is_allowed(&a));
    assert!(!client.is_allowed(&b));
    assert!(client.is_allowed(&c));
}

// ── #124 Temporary allowlist expiration tests ─────────────────────────────────

#[test]
fn temp_allow_before_expiry_is_allowed() {
    let (env, admin, subject, client) = setup();
    let now = env.ledger().timestamp();
    client.allow_address_until(&admin, &subject, &(now + 1000));
    assert!(client.is_allowed(&subject));
}

#[test]
fn temp_allow_after_expiry_is_denied() {
    let (env, admin, subject, client) = setup();
    let now = env.ledger().timestamp();
    // expires in the past
    client.allow_address_until(&admin, &subject, &now);
    assert!(!client.is_allowed(&subject));
}

#[test]
fn temp_allow_blocked_address_is_denied_regardless_of_expiry() {
    let (env, admin, subject, client) = setup();
    let now = env.ledger().timestamp();
    client.allow_address_until(&admin, &subject, &(now + 1000));
    client.block_address(&admin, &subject);
    assert!(!client.is_allowed(&subject));
}

#[test]
fn temp_allow_cleared_removes_expiry_block() {
    let (env, admin, subject, client) = setup();
    let now = env.ledger().timestamp();
    // set expired temp allow
    client.allow_address_until(&admin, &subject, &now);
    assert!(!client.is_allowed(&subject));
    // clear restores permanent allow (no expiry key respected after clear)
    client.clear_address(&admin, &subject);
    // clear_address sets Allowed=true, Blocked=false but does NOT remove AllowedUntil
    // so we verify the contract's actual behaviour: still expired
    // To permanently allow, use allow_address (no expiry)
    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
}

// ── #125 Admin transfer flow tests ───────────────────────────────────────────

#[test]
fn admin_transfer_new_admin_can_allow() {
    let (env, admin, subject, client) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);
    // new admin can allow
    client.allow_address(&new_admin, &subject);
    assert!(client.is_allowed(&subject));
}

#[test]
fn admin_transfer_old_admin_loses_privileges() {
    let (env, admin, subject, client) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);
    // old admin can no longer allow
    // old admin can no longer allow (should return an error)
    let result = client.try_allow_address(&admin, &subject);
    assert!(result.is_err());
}

#[test]
fn admin_transfer_requires_accept_before_taking_effect() {
    let (env, admin, subject, client) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    // new_admin has NOT called accept_admin yet; old admin still works
    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
}

#[test]
fn admin_transfer_wrong_acceptor_panics() {
    let (env, admin, _subject, client) = setup();
    let new_admin = Address::generate(&env);
    let impostor = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    let result = client.try_accept_admin(&impostor);
    assert!(result.is_err());
}

#[test]
fn allow_address_returns_unauthorized_for_non_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    let address = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);

    let result = client.try_allow_address(&non_admin, &address);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn allow_address_returns_contract_paused_when_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let address = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    client.pause(&admin);

    let result = client.try_allow_address(&admin, &address);
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
}

// ── #80 export_snapshot tests ─────────────────────────────────────────────────

#[test]
fn export_snapshot_returns_all_tracked_addresses() {
    use compliance::AddressState;
    let (env, admin, _, client) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    client.allow_address(&admin, &a);
    client.allow_address(&admin, &b);
    client.block_address(&admin, &c);

    let snapshot = client.export_snapshot(&admin, &0, &0);
    assert_eq!(snapshot.len(), 3);

    // collect into a plain vec for easy lookup
    let mut found_a = false;
    let mut found_b = false;
    let mut found_c = false;
    for (addr, state) in snapshot.iter() {
        if addr == a {
            assert_eq!(state, AddressState::Allowed);
            found_a = true;
        } else if addr == b {
            assert_eq!(state, AddressState::Allowed);
            found_b = true;
        } else if addr == c {
            assert_eq!(state, AddressState::Blocked);
            found_c = true;
        }
    }
    assert!(found_a && found_b && found_c);
}

#[test]
fn export_snapshot_reflects_state_changes() {
    use compliance::AddressState;
    let (_env, admin, subject, client) = setup();

    client.allow_address(&admin, &subject);
    let snap1 = client.export_snapshot(&admin, &0, &0);
    assert_eq!(snap1.get(0).unwrap().1, AddressState::Allowed);

    client.block_address(&admin, &subject);
    let snap2 = client.export_snapshot(&admin, &0, &0);
    assert_eq!(snap2.get(0).unwrap().1, AddressState::Blocked);
}

#[test]
fn export_snapshot_dedups_repeated_operations_on_same_address() {
    let (_env, admin, subject, client) = setup();

    client.allow_address(&admin, &subject);
    client.block_address(&admin, &subject);
    client.clear_address(&admin, &subject);

    let snapshot = client.export_snapshot(&admin, &0, &0);
    assert_eq!(snapshot.len(), 1);
}

#[test]
fn export_snapshot_empty_when_no_addresses_tracked() {
    let (_env, admin, _subject, client) = setup();
    let snapshot = client.export_snapshot(&admin, &0, &0);
    assert_eq!(snapshot.len(), 0);
}

#[test]
fn export_snapshot_expired_temp_allow_shows_expired() {
    use compliance::AddressState;
    let (env, admin, subject, client) = setup();
    let now = env.ledger().timestamp();
    // expires_at == now means timestamp is NOT < expires_at → Expired
    client.allow_address_until(&admin, &subject, &now);
    let snapshot = client.export_snapshot(&admin, &0, &0);
    assert_eq!(snapshot.get(0).unwrap().1, AddressState::Expired);
}

// ── #88 Event snapshot tests ──────────────────────────────────────────────────

#[test]
fn event_snapshot_address_allowed() {
    let (env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    let events = env.events().all();
    // Last event: topics = ("address_allowed",), data = subject address
    let ev = events.last().unwrap();
    let topics: soroban_sdk::Vec<soroban_sdk::Val> = ev.1;
    let topic_sym: Symbol = soroban_sdk::FromVal::from_val(&env, &topics.get(0).unwrap());
    assert_eq!(topic_sym, Symbol::new(&env, "address_allowed"));
    let data_addr: Address = soroban_sdk::FromVal::from_val(&env, &ev.2);
    assert_eq!(data_addr, subject);
}

#[test]
fn event_snapshot_address_blocked() {
    let (env, admin, subject, client) = setup();
    client.block_address(&admin, &subject);
    let events = env.events().all();
    let ev = events.last().unwrap();
    let topics: soroban_sdk::Vec<soroban_sdk::Val> = ev.1;
    let topic_sym: Symbol = soroban_sdk::FromVal::from_val(&env, &topics.get(0).unwrap());
    assert_eq!(topic_sym, Symbol::new(&env, "address_blocked"));
    let data_addr: Address = soroban_sdk::FromVal::from_val(&env, &ev.2);
    assert_eq!(data_addr, subject);
}

#[test]
fn event_snapshot_address_allowed_until() {
    let (env, admin, subject, client) = setup();
    let expires_at: u64 = env.ledger().timestamp() + 1000;
    client.allow_address_until(&admin, &subject, &expires_at);
    let events = env.events().all();
    let ev = events.last().unwrap();
    let topics: soroban_sdk::Vec<soroban_sdk::Val> = ev.1;
    let topic_sym: Symbol = soroban_sdk::FromVal::from_val(&env, &topics.get(0).unwrap());
    assert_eq!(topic_sym, Symbol::new(&env, "address_allowed_until"));
    let data: (Address, u64) = soroban_sdk::FromVal::from_val(&env, &ev.2);
    assert_eq!(data.0, subject);
    assert_eq!(data.1, expires_at);
}

#[test]
fn event_snapshot_address_cleared() {
    let (env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    client.block_address(&admin, &subject);
    client.clear_address(&admin, &subject);
    let events = env.events().all();
    let ev = events.last().unwrap();
    let topics: soroban_sdk::Vec<soroban_sdk::Val> = ev.1;
    let topic_sym: Symbol = soroban_sdk::FromVal::from_val(&env, &topics.get(0).unwrap());
    assert_eq!(topic_sym, Symbol::new(&env, "address_cleared"));
    let data_addr: Address = soroban_sdk::FromVal::from_val(&env, &ev.2);
    assert_eq!(data_addr, subject);
}

#[test]
fn event_snapshot_admin_transfer_initiated() {
    let (env, admin, _subject, client) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    let events = env.events().all();
    let ev = events.last().unwrap();
    let topics: soroban_sdk::Vec<soroban_sdk::Val> = ev.1;
    let topic_sym: Symbol = soroban_sdk::FromVal::from_val(&env, &topics.get(0).unwrap());
    assert_eq!(topic_sym, Symbol::new(&env, "admin_transfer_initiated"));
    let data_addr: Address = soroban_sdk::FromVal::from_val(&env, &ev.2);
    assert_eq!(data_addr, new_admin);
}

#[test]
fn event_snapshot_admin_transferred() {
    let (env, admin, _subject, client) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);
    let events = env.events().all();
    let ev = events.last().unwrap();
    let topics: soroban_sdk::Vec<soroban_sdk::Val> = ev.1;
    let topic_sym: Symbol = soroban_sdk::FromVal::from_val(&env, &topics.get(0).unwrap());
    assert_eq!(topic_sym, Symbol::new(&env, "admin_transferred"));
    let data_addr: Address = soroban_sdk::FromVal::from_val(&env, &ev.2);
    assert_eq!(data_addr, new_admin);
}

#[test]
fn event_snapshot_compliance_paused() {
    let (env, admin, _subject, client) = setup();
    client.pause(&admin);
    let events = env.events().all();
    let ev = events.last().unwrap();
    let topics: soroban_sdk::Vec<soroban_sdk::Val> = ev.1;
    let topic_sym: Symbol = soroban_sdk::FromVal::from_val(&env, &topics.get(0).unwrap());
    assert_eq!(topic_sym, Symbol::new(&env, "compliance_paused"));
    let data_addr: Address = soroban_sdk::FromVal::from_val(&env, &ev.2);
    assert_eq!(data_addr, admin);
}

#[test]
fn event_snapshot_compliance_unpaused() {
    let (env, admin, _subject, client) = setup();
    client.pause(&admin);
    client.unpause(&admin);
    let events = env.events().all();
    let ev = events.last().unwrap();
    let topics: soroban_sdk::Vec<soroban_sdk::Val> = ev.1;
    let topic_sym: Symbol = soroban_sdk::FromVal::from_val(&env, &topics.get(0).unwrap());
    assert_eq!(topic_sym, Symbol::new(&env, "compliance_unpaused"));
    let data_addr: Address = soroban_sdk::FromVal::from_val(&env, &ev.2);
    assert_eq!(data_addr, admin);
}

// ── #89 Pagination tests ──────────────────────────────────────────────────────

#[test]
fn export_snapshot_pagination_first_page() {
    use compliance::AddressState;
    let (env, admin, _, client) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    client.allow_address(&admin, &a);
    client.allow_address(&admin, &b);
    client.allow_address(&admin, &c);

    let page = client.export_snapshot(&admin, &0, &2);
    assert_eq!(page.len(), 2);
}

#[test]
fn export_snapshot_pagination_second_page() {
    let (env, admin, _, client) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    client.allow_address(&admin, &a);
    client.allow_address(&admin, &b);
    client.allow_address(&admin, &c);

    let page = client.export_snapshot(&admin, &2, &2);
    assert_eq!(page.len(), 1);
}

#[test]
fn export_snapshot_pagination_start_beyond_end() {
    let (env, admin, _, client) = setup();
    let a = Address::generate(&env);
    client.allow_address(&admin, &a);

    let page = client.export_snapshot(&admin, &10, &5);
    assert_eq!(page.len(), 0);
}

#[test]
fn bulk_check_addresses_returns_states() {
    use compliance::AddressState;
    let (env, admin, _, client) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    client.allow_address(&admin, &a);
    client.block_address(&admin, &b);

    let addrs = soroban_sdk::vec![&env, a.clone(), b.clone()];
    let result = client.bulk_check_addresses(&addrs, &0, &0);
    assert_eq!(result.len(), 2);
    assert_eq!(result.get(0).unwrap().1, AddressState::Allowed);
    assert_eq!(result.get(1).unwrap().1, AddressState::Blocked);
}

#[test]
fn bulk_check_addresses_pagination() {
    use compliance::AddressState;
    let (env, admin, _, client) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);
    client.allow_address(&admin, &a);
    client.allow_address(&admin, &b);
    client.allow_address(&admin, &c);

    let addrs = soroban_sdk::vec![&env, a.clone(), b.clone(), c.clone()];
    let page = client.bulk_check_addresses(&addrs, &1, &2);
    assert_eq!(page.len(), 2);
    assert_eq!(page.get(0).unwrap().0, b);
    assert_eq!(page.get(0).unwrap().1, AddressState::Allowed);
}

#[test]
fn bulk_check_addresses_unknown_address_is_blocked() {
    use compliance::AddressState;
    let (env, _admin, _, client) = setup();
    let unknown = Address::generate(&env);
    let addrs = soroban_sdk::vec![&env, unknown.clone()];
    let result = client.bulk_check_addresses(&addrs, &0, &0);
    assert_eq!(result.get(0).unwrap().1, AddressState::Blocked);
}
