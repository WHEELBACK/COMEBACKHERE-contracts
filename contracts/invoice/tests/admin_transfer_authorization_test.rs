//! Coverage check for invoice's two-step admin transfer (`transfer_admin` /
//! `accept_admin`, contracts/invoice/src/entrypoints/admin.rs:46-72).
//!
//! #55 added a test confirming "only PendingAdmin can call accept_admin",
//! but reading contracts/invoice/tests/invoice_test.rs shows that coverage
//! only ever exercises `accept_admin`/`transfer_admin` indirectly, via a
//! static string-list membership check for the contract's entrypoint names
//! (see the `expected_entrypoints` set around invoice_test.rs:800-820) — the
//! two-step flow's actual authorization behavior (who can call what, in what
//! order, with what effect) has no functional test in this contract, unlike
//! compliance's equivalent flow. This file closes that gap.
//!
//! The two-step pattern exists specifically so a transfer to an unreachable
//! or mistyped address never takes effect — the new admin must explicitly
//! accept before control changes hands. These tests exercise exactly that
//! failure mode: a caller other than the currently-pending admin must never
//! be able to complete the transfer.

use invoice::{InvoiceContract, InvoiceContractClient, InvoiceError};
use soroban_sdk::{testutils::Address as _, Address, Env};

fn setup() -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

#[test]
fn accept_admin_with_no_pending_transfer_is_rejected() {
    let (_env, _admin, client) = setup();
    let rando = Address::generate(&_env);
    let err = client.try_accept_admin(&rando).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::NoPendingAdmin);
}

#[test]
fn only_the_pending_admin_can_call_accept_admin() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);
    let impostor = Address::generate(&env);

    client.transfer_admin(&admin, &new_admin);

    // A caller who is not the nominated pending admin must be rejected, even
    // though they can produce a valid auth for themselves.
    let err = client.try_accept_admin(&impostor).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);

    // Admin role must not have moved as a side effect of the rejected call:
    // the impostor still cannot perform admin-gated actions.
    let err2 = client
        .try_set_grace_window(&impostor, &60)
        .unwrap_err()
        .unwrap();
    assert_eq!(err2, InvoiceError::Unauthorized);
}

#[test]
fn pending_admin_can_accept_and_gains_admin_rights() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);

    client.transfer_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    // New admin can now perform admin-gated actions.
    assert!(client.try_set_grace_window(&new_admin, &120).is_ok());
    assert_eq!(client.get_grace_window(), 120);

    // Old admin has lost admin rights — same action now rejected.
    let err = client
        .try_set_grace_window(&admin, &1)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);
}

#[test]
fn accepting_clears_pending_admin_so_it_cannot_be_reused() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);

    client.transfer_admin(&admin, &new_admin);
    client.accept_admin(&new_admin);

    // A second accept_admin call by the same (now-actual) admin must fail:
    // PendingAdmin was cleared on acceptance, so there is nothing to accept.
    let err = client.try_accept_admin(&new_admin).unwrap_err().unwrap();
    assert_eq!(err, InvoiceError::NoPendingAdmin);
}

#[test]
fn transfer_admin_can_be_called_again_before_acceptance_to_change_pending_admin() {
    let (env, admin, client) = setup();
    let first_nominee = Address::generate(&env);
    let second_nominee = Address::generate(&env);

    client.transfer_admin(&admin, &first_nominee);
    // Admin changes their mind before the first nominee ever accepts.
    client.transfer_admin(&admin, &second_nominee);

    // The superseded nominee can no longer accept — they are not the
    // currently pending admin.
    let err = client
        .try_accept_admin(&first_nominee)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);

    // The latest nominee can accept and becomes admin.
    client.accept_admin(&second_nominee);
    assert!(client.try_set_grace_window(&second_nominee, &30).is_ok());
    assert_eq!(client.get_grace_window(), 30);
}

#[test]
fn transfer_admin_requires_current_admin_auth() {
    let (env, _admin, client) = setup();
    let rogue = Address::generate(&env);
    let new_admin = Address::generate(&env);

    let err = client
        .try_transfer_admin(&rogue, &new_admin)
        .unwrap_err()
        .unwrap();
    assert_eq!(err, InvoiceError::Unauthorized);
}

#[test]
fn accept_admin_requires_new_admin_auth() {
    let (env, admin, client) = setup();
    let new_admin = Address::generate(&env);
    client.transfer_admin(&admin, &new_admin);

    // Without new_admin's own authorization in the auth set, accept_admin
    // must not succeed on their behalf.
    let result = client.mock_auths(&[]).try_accept_admin(&new_admin);
    assert!(result.is_err());
}
