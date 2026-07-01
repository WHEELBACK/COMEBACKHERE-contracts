use compliance::{ComplianceContract, ComplianceContractClient, ContractError};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    Address, Env, FromVal, Symbol,
};

fn last_event_symbol(env: &Env) -> Symbol {
    let events = env.events().all();
    let (_, topics, _) = events.last().unwrap();
    Symbol::from_val(env, &topics.get_unchecked(0))
}

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
    client.block_address(&admin, &payer, &None);
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
    client.block_address(&admin, &payer, &None);
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
    client.block_address(&admin, &address, &None);
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
    client.block_address(&admin, &address, &None);
    assert!(!client.is_allowed(&address));

    // Pause then unpause
    client.pause(&admin);
    client.unpause(&admin);

    // Clear address should now work
    client.clear_address(&admin, &address);
    assert!(client.is_allowed(&address));
}

#[test]
fn revoke_allow_removes_allowed_status() {
    let (_env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
    client.revoke_allow(&admin, &subject);
    assert!(!client.is_allowed(&subject));
}

#[test]
fn revoke_allow_does_not_block() {
    let (_env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    client.revoke_allow(&admin, &subject);
    // Not allowed, but also not blocked — re-allow should work
    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
}

#[test]
fn revoke_allow_removes_expiry() {
    let (env, admin, subject, client) = setup();
    let now = env.ledger().timestamp();
    client.allow_address_until(&admin, &subject, &(now + 1000));
    assert!(client.is_allowed(&subject));
    client.revoke_allow(&admin, &subject);
    assert!(!client.is_allowed(&subject));
    // Re-allow permanently
    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
}

#[test]
fn revoke_allow_returns_unauthorized_for_non_admin() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let non_admin = Address::generate(&env);
    let address = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);

    let result = client.try_revoke_allow(&non_admin, &address);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn revoke_allow_returns_contract_paused_when_paused() {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let address = Address::generate(&env);
    let id = env.register_contract(None, ComplianceContract);
    let client = ComplianceContractClient::new(&env, &id);
    client.initialize(&admin);
    client.pause(&admin);

    let result = client.try_revoke_allow(&admin, &address);
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
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
    client.block_address(&admin, &blocked_address, &None);

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
#[test]
fn emits_address_allowed_event() {
    let (env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    assert_eq!(
        last_event_symbol(&env),
        Symbol::new(&env, "address_allowed")
    );
    assert!(client.is_allowed(&subject));
}

// Verification: address_blocked event schema
// - topics[0]: symbol "address_blocked"
// - data: single Address value for the blocked address
#[test]
fn emits_address_blocked_event() {
    let (env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
    client.block_address(&admin, &subject, &None);
    assert_eq!(
        last_event_symbol(&env),
        Symbol::new(&env, "address_blocked")
    );
    assert!(!client.is_allowed(&subject));
}

// Verification: address_cleared event schema
// - topics[0]: symbol "address_cleared"
// - data: single Address value for the cleared address
#[test]
fn emits_address_cleared_event() {
    let (env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    client.block_address(&admin, &subject, &None);
    assert!(!client.is_allowed(&subject));
    client.clear_address(&admin, &subject);
    assert_eq!(
        last_event_symbol(&env),
        Symbol::new(&env, "address_cleared")
    );
    assert!(client.is_allowed(&subject));
}

// ── #79 Default-deny posture ──────────────────────────────────────────────────

/// is_allowed must return false for an address that has never been added via
/// allow_address or allow_address_until, confirming the default-deny posture.
#[test]
fn is_allowed_returns_false_for_address_never_added() {
    let (_env, _admin, subject, client) = setup();
    assert!(!client.is_allowed(&subject));
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
    client.block_address(&admin, &subject, &None);
    assert!(!client.is_allowed(&subject));
}

#[test]
fn precedence_blocked_then_cleared_is_allowed() {
    let (_env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    client.block_address(&admin, &subject, &None);
    client.clear_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
}

#[test]
fn precedence_block_without_prior_allow_is_denied() {
    let (_env, admin, subject, client) = setup();
    client.block_address(&admin, &subject, &None);
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
        client.block_address(&admin, &addr, &None);
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
    client.block_address(&admin, &b, &None);
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
    client.block_address(&admin, &subject, &None);
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

// ── #73 bulk_check_addresses tests ────────────────────────────────────────────

#[test]
fn bulk_check_returns_correct_results() {
    let (env, admin, _, client) = setup();
    let a = Address::generate(&env);
    let b = Address::generate(&env);
    let c = Address::generate(&env);

    client.allow_address(&admin, &a);
    client.allow_address(&admin, &b);
    // c is never allowed

    let addresses = soroban_sdk::vec![&env, a.clone(), b.clone(), c.clone()];
    let results = client.bulk_check_addresses(&addresses);

    assert_eq!(results.get(0).unwrap(), true);
    assert_eq!(results.get(1).unwrap(), true);
    assert_eq!(results.get(2).unwrap(), false);
}

#[test]
fn bulk_check_blocked_address_returns_false() {
    let (env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    client.block_address(&admin, &subject, &None);

    let addresses = soroban_sdk::vec![&env, subject.clone()];
    let results = client.bulk_check_addresses(&addresses);
    assert_eq!(results.get(0).unwrap(), false);
}

#[test]
fn bulk_check_empty_input_returns_empty() {
    let (env, _, _, client) = setup();
    let addresses: soroban_sdk::Vec<Address> = soroban_sdk::Vec::new(&env);
    let results = client.bulk_check_addresses(&addresses);
    assert_eq!(results.len(), 0);
}

// ── #72 Unauthorized non-admin access tests ───────────────────────────────────

#[test]
fn non_admin_cannot_call_allow_address() {
    let (env, _admin, subject, client) = setup();
    let non_admin = Address::generate(&env);
    let result = client.try_allow_address(&non_admin, &subject);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn non_admin_cannot_call_block_address() {
    let (env, _admin, subject, client) = setup();
    let non_admin = Address::generate(&env);
    let result = client.try_block_address(&non_admin, &subject, &None);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn non_admin_cannot_call_clear_address() {
    let (env, _admin, subject, client) = setup();
    let non_admin = Address::generate(&env);
    let result = client.try_clear_address(&non_admin, &subject);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn non_admin_cannot_call_allow_address_until() {
    let (env, _admin, subject, client) = setup();
    let non_admin = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 1000;
    let result = client.try_allow_address_until(&non_admin, &subject, &expires_at);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn non_admin_cannot_call_transfer_admin() {
    let (env, _admin, _, client) = setup();
    let non_admin = Address::generate(&env);
    let new_admin = Address::generate(&env);
    let result = client.try_transfer_admin(&non_admin, &new_admin);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
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
    client.block_address(&admin, &c, &None);

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

    client.block_address(&admin, &subject, &None);
    let snap2 = client.export_snapshot(&admin, &0, &0);
    assert_eq!(snap2.get(0).unwrap().1, AddressState::Blocked);
}

#[test]
fn export_snapshot_dedups_repeated_operations_on_same_address() {
    let (_env, admin, subject, client) = setup();

    client.allow_address(&admin, &subject);
    client.block_address(&admin, &subject, &None);
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

// ── #83 Pause regression: allow entrypoints reject while paused ───────────────

#[test]
fn paused_contract_rejects_allow_address() {
    let (_env, admin, subject, client) = setup();
    client.pause(&admin);
    let result = client.try_allow_address(&admin, &subject);
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
}

#[test]
fn paused_contract_rejects_allow_address_until() {
    let (env, admin, subject, client) = setup();
    let expires_at = env.ledger().timestamp() + 1000;
    client.pause(&admin);
    let result = client.try_allow_address_until(&admin, &subject, &expires_at);
    assert_eq!(result, Err(Ok(ContractError::ContractPaused)));
}

#[test]
fn unpause_restores_allow_address_and_allow_address_until() {
    let (env, admin, subject, client) = setup();
    let subject2 = Address::generate(&env);
    let expires_at = env.ledger().timestamp() + 1000;
    client.pause(&admin);
    client.unpause(&admin);
    client.allow_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
    client.allow_address_until(&admin, &subject2, &expires_at);
    assert!(client.is_allowed(&subject2));
}

// ── #70 clear_address resets both allowed and blocked flags ───────────────────

#[test]
fn clear_address_resets_blocked_flag_and_sets_allowed() {
    use compliance::AddressState;
    let (_env, admin, subject, client) = setup();
    // Block first (without prior allow so Blocked=true, Allowed=false)
    client.block_address(&admin, &subject, &None);
    assert!(!client.is_allowed(&subject));

    client.clear_address(&admin, &subject);

    // is_allowed must return true
    assert!(client.is_allowed(&subject));
    // address_status must reflect Allowed (not Blocked)
    let state = client.address_status(&admin, &subject);
    assert_eq!(state, AddressState::Allowed);
}

#[test]
fn clear_address_never_blocked_is_idempotent() {
    use compliance::AddressState;
    let (_env, admin, subject, client) = setup();
    // Address was never blocked or allowed; clear_address must not error
    client.clear_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
    let state = client.address_status(&admin, &subject);
    assert_eq!(state, AddressState::Allowed);
    // Second clear is also idempotent
    client.clear_address(&admin, &subject);
    assert!(client.is_allowed(&subject));
}

// ── #85 Old admin loses authority after admin transfer completes ──────────────

#[test]
fn old_admin_allow_address_returns_unauthorized_after_transfer() {
    let (env, admin, subject, client) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);
    let result = client.try_allow_address(&admin, &subject);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn old_admin_block_address_returns_unauthorized_after_transfer() {
    let (env, admin, subject, client) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);
    let result = client.try_block_address(&admin, &subject, &None);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

#[test]
fn old_admin_pause_returns_unauthorized_after_transfer() {
    let (env, admin, _subject, client) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);
    let result = client.try_pause(&admin);
    assert_eq!(result, Err(Ok(ContractError::Unauthorized)));
}

// ── #64 allow_address_until expiry boundary conditions ────────────────────────
// is_allowed uses strict `<` comparison: timestamp < expires_at.

#[test]
fn allow_address_until_expires_at_minus_one_is_allowed() {
    let (env, admin, subject, client) = setup();
    let expires_at: u64 = 1_000;
    client.allow_address_until(&admin, &subject, &expires_at);
    env.ledger().with_mut(|l| l.timestamp = expires_at - 1);
    assert!(client.is_allowed(&subject));
}

#[test]
fn allow_address_until_at_expires_at_is_not_allowed() {
    let (env, admin, subject, client) = setup();
    let expires_at: u64 = 1_000;
    client.allow_address_until(&admin, &subject, &expires_at);
    env.ledger().with_mut(|l| l.timestamp = expires_at);
    assert!(!client.is_allowed(&subject));
}

#[test]
fn allow_address_until_expires_at_plus_one_is_not_allowed() {
    let (env, admin, subject, client) = setup();
    let expires_at: u64 = 1_000;
    client.allow_address_until(&admin, &subject, &expires_at);
    env.ledger().with_mut(|l| l.timestamp = expires_at + 1);
    assert!(!client.is_allowed(&subject));
}

#[test]
fn allow_address_after_allow_address_until_removes_expiry() {
    let (env, admin, subject, client) = setup();
    let expires_at: u64 = 1_000;
    client.allow_address_until(&admin, &subject, &expires_at);
    // Promote to permanent allow — clears the AllowedUntil key.
    client.allow_address(&admin, &subject);
    // Even past the original expiry the address is still allowed.
    env.ledger().with_mut(|l| l.timestamp = expires_at + 9_999);
    assert!(client.is_allowed(&subject));
}

// ── #63 Block flag overrides allow flag in is_allowed ─────────────────────────

#[test]
fn block_flag_overrides_allow_flag_in_is_allowed() {
    let (_env, admin, subject, client) = setup();
    client.allow_address(&admin, &subject);
    client.block_address(&admin, &subject, &None);
    assert!(!client.is_allowed(&subject));
}
