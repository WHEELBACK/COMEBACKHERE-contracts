// #29 added a MAX_ALLOWED_TOKENS cap on treasury's TokenAllowlist. Reading
// add_allowed_token/remove_allowed_token in contracts/treasury/src/settlements.rs
// shows the cap is checked against `allowlist.len()` freshly read from
// instance storage on every add_allowed_token call, and that removal writes
// the updated list back to storage synchronously before returning -- there is
// no batching, deferred check, or transaction-boundary behavior that could
// let the list momentarily exceed the cap. This suite exercises that reading
// under a rapid remove/re-add cycling pattern, rather than only the
// straightforward monotonic add-until-full sequences already covered in
// treasury_token_allowlist_test.rs.
//
// Verified finding: no bug found. The guard is checked against the
// allowlist's current size at the moment each token is added, and cannot be
// bypassed by rapid remove-then-readd cycling.

extern crate std;

use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

/// Mirrors treasury::MAX_ALLOWED_TOKENS, which is `pub(crate)` and so not
/// visible from an external test crate.
const MAX_ALLOWED_TOKENS: u32 = 20;

fn setup(env: &Env) -> (TreasuryContractClient, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

fn fill_to_cap(
    env: &Env,
    client: &TreasuryContractClient,
    admin: &Address,
) -> std::vec::Vec<Address> {
    let mut tokens = std::vec::Vec::with_capacity(MAX_ALLOWED_TOKENS as usize);
    for _ in 0..MAX_ALLOWED_TOKENS {
        let token = Address::generate(env);
        client.add_allowed_token(admin, &token);
        tokens.push(token);
    }
    tokens
}

#[test]
fn cap_is_never_exceeded_across_rapid_remove_readd_cycles() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let tokens = fill_to_cap(&env, &client, &admin);
    assert_eq!(client.get_allowed_tokens().len() as u32, MAX_ALLOWED_TOKENS);

    // Rapidly remove the current front token and add a brand-new one in its
    // place, many times in a row, checking the invariant after *every single*
    // storage-mutating call -- not just at the end of each cycle -- so that
    // any deferred-check or transaction-boundary window where the list could
    // momentarily exceed the cap would be caught.
    let mut current_front = tokens[0].clone();
    for _ in 0..200 {
        client.remove_allowed_token(&admin, &current_front);
        assert!(client.get_allowed_tokens().len() as u32 <= MAX_ALLOWED_TOKENS);

        let new_token = Address::generate(&env);
        client.add_allowed_token(&admin, &new_token);
        assert!(client.get_allowed_tokens().len() as u32 <= MAX_ALLOWED_TOKENS);

        current_front = new_token;
    }
    assert_eq!(client.get_allowed_tokens().len() as u32, MAX_ALLOWED_TOKENS);
}

#[test]
fn adding_at_cap_without_removing_first_is_still_rejected_during_cycling() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    fill_to_cap(&env, &client, &admin);

    // Confirm the guard actually blocks growth past the cap -- i.e. that the
    // invariant above isn't holding merely because the test always removes
    // before adding. Attempting to add without a preceding remove, even in a
    // cycling-style scenario, must still be rejected.
    let extra = Address::generate(&env);
    assert!(client.try_add_allowed_token(&admin, &extra).is_err());
    assert_eq!(client.get_allowed_tokens().len() as u32, MAX_ALLOWED_TOKENS);

    // Now remove one and confirm the add that follows succeeds -- the guard
    // reacts to the *current* length, not to a stale count from before.
    let removed = client.get_allowed_tokens().get(0).unwrap();
    client.remove_allowed_token(&admin, &removed);
    client.add_allowed_token(&admin, &extra);
    assert_eq!(client.get_allowed_tokens().len() as u32, MAX_ALLOWED_TOKENS);
}

#[test]
fn readding_a_token_already_present_is_a_no_op_and_does_not_grow_past_cap() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let tokens = fill_to_cap(&env, &client, &admin);

    // Re-adding an already-allowed token while at the cap must be a silent
    // no-op (the `!allowlist.contains(&token)` guard short-circuits before
    // the cap check is even reached), not a panic and not a duplicate entry.
    client.add_allowed_token(&admin, &tokens[3]);
    assert_eq!(client.get_allowed_tokens().len() as u32, MAX_ALLOWED_TOKENS);
}

#[test]
fn removing_and_readding_the_same_token_many_times_keeps_len_stable() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let tokens = fill_to_cap(&env, &client, &admin);
    let cycling_token = tokens[10].clone();

    for _ in 0..100 {
        client.remove_allowed_token(&admin, &cycling_token);
        assert_eq!(
            client.get_allowed_tokens().len() as u32,
            MAX_ALLOWED_TOKENS - 1
        );
        client.add_allowed_token(&admin, &cycling_token);
        assert_eq!(client.get_allowed_tokens().len() as u32, MAX_ALLOWED_TOKENS);
    }
}
