// #567 — `remove_allowed_token` must refuse to remove an allowlisted token while
// settlements are still pending, and succeed once they are resolved.

use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient, TreasuryError};

/// `remove_allowed_token` returns `()` and panics with a contract error, so the
/// generated `try_` client reports it as a raw `soroban_sdk::Error`.
fn pending_settlements_error() -> soroban_sdk::Error {
    soroban_sdk::Error::from_contract_error(TreasuryError::TokenHasPendingSettlements as u32)
}

fn setup(env: &Env) -> (TreasuryContractClient<'_>, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

#[test]
fn removal_without_pending_settlements_succeeds() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let token = Address::generate(&env);

    client.add_allowed_token(&admin, &token);
    client.remove_allowed_token(&admin, &token);

    assert_eq!(client.get_allowed_tokens().len(), 0);
}

#[test]
fn removal_with_pending_settlement_is_rejected_with_typed_error() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    client.add_allowed_token(&admin, &token);
    client.propose_settlement(&admin, &merchant, &10_000_000);

    assert_eq!(
        client.try_remove_allowed_token(&admin, &token),
        Err(Ok(pending_settlements_error()))
    );
    // The allowlist is untouched by the failed removal.
    assert!(client.get_allowed_tokens().contains(&token));
}

#[test]
fn removal_succeeds_once_pending_settlement_is_cancelled() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    client.add_allowed_token(&admin, &token);
    let sid = client.propose_settlement(&admin, &merchant, &10_000_000);
    assert!(client.try_remove_allowed_token(&admin, &token).is_err());

    client.cancel_settlement(&admin, &sid);
    client.remove_allowed_token(&admin, &token);

    assert_eq!(client.get_allowed_tokens().len(), 0);
}

#[test]
fn removal_is_blocked_while_any_one_of_several_settlements_is_pending() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let token = Address::generate(&env);

    client.add_allowed_token(&admin, &token);
    let first = client.propose_settlement(&admin, &merchant, &1_000_000);
    let second = client.propose_settlement(&admin, &merchant, &2_000_000);

    client.cancel_settlement(&admin, &first);
    assert_eq!(
        client.try_remove_allowed_token(&admin, &token),
        Err(Ok(pending_settlements_error()))
    );

    client.cancel_settlement(&admin, &second);
    client.remove_allowed_token(&admin, &token);
}

#[test]
fn removing_a_token_not_on_the_allowlist_is_not_blocked() {
    let env = Env::default();
    let (client, admin) = setup(&env);
    let merchant = Address::generate(&env);
    let unlisted = Address::generate(&env);

    client.propose_settlement(&admin, &merchant, &10_000_000);
    client.remove_allowed_token(&admin, &unlisted);
}
