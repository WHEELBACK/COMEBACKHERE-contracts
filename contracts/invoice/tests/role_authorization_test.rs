//! #879: granular role-based authorization for invoice parameters.
//!
//! The admin implicitly holds every role. A `ConfigManager` may change
//! operational limits only, a `FeeManager` fee parameters only, and a
//! `PauseGuardian` may pause but not unpause. Only the admin grants and
//! revokes roles, and nobody else can modify any parameter.

use invoice::{InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceRole, MaybeAddress, MaybeBytes};
use soroban_sdk::{testutils::Address as _, Address, Env};

struct Fixture {
    env: Env,
    admin: Address,
    client: InvoiceContractClient<'static>,
}

fn setup() -> Fixture {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let contract_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);
    Fixture { env, admin, client }
}

/// Which parameter setter `who` could call, keyed by the role that gates it.
fn config_calls_ok(f: &Fixture, who: &Address) -> [bool; 4] {
    [
        f.client.try_set_grace_window(who, &60).is_ok(),
        f.client.try_set_max_open_invoices(who, &5).is_ok(),
        f.client
            .try_set_merchant_max_open_invoices(who, &Address::generate(&f.env), &Some(2))
            .is_ok(),
        f.client.try_set_refund_window(who, &3_600).is_ok(),
    ]
}

fn fee_calls_ok(f: &Fixture, who: &Address) -> [bool; 2] {
    [
        f.client.try_set_late_fee_bps(who, &100).is_ok(),
        f.client.try_set_refund_network_fee(who, &200).is_ok(),
    ]
}

#[test]
fn admin_holds_every_role_implicitly() {
    let f = setup();
    assert_eq!(config_calls_ok(&f, &f.admin), [true; 4]);
    assert_eq!(fee_calls_ok(&f, &f.admin), [true; 2]);
    f.client.pause(&f.admin);
    f.client.unpause(&f.admin);
}

#[test]
fn outsider_cannot_modify_any_parameter() {
    let f = setup();
    let outsider = Address::generate(&f.env);
    assert_eq!(config_calls_ok(&f, &outsider), [false; 4]);
    assert_eq!(fee_calls_ok(&f, &outsider), [false; 2]);
    assert_eq!(
        f.client.try_pause(&outsider),
        Err(Ok(InvoiceError::Unauthorized))
    );
    assert_eq!(
        f.client.try_set_late_fee_bps(&outsider, &1),
        Err(Ok(InvoiceError::Unauthorized))
    );
    // Nothing changed.
    assert_eq!(f.client.get_grace_window(), 0);
    assert_eq!(f.client.get_late_fee_bps(), 0);
}

#[test]
fn config_manager_is_scoped_to_config_parameters() {
    let f = setup();
    let manager = Address::generate(&f.env);
    f.client
        .grant_role(&f.admin, &InvoiceRole::ConfigManager, &manager);
    assert!(f.client.has_role(&InvoiceRole::ConfigManager, &manager));

    assert_eq!(config_calls_ok(&f, &manager), [true; 4]);
    assert_eq!(fee_calls_ok(&f, &manager), [false; 2]);
    assert!(f.client.try_pause(&manager).is_err());
    assert_eq!(f.client.get_grace_window(), 60);
}

#[test]
fn config_manager_can_extend_invoice_expiry() {
    let f = setup();
    let manager = Address::generate(&f.env);
    f.client
        .grant_role(&f.admin, &InvoiceRole::ConfigManager, &manager);
    let id = f.client.create_invoice(
        &Address::generate(&f.env),
        &10_000_000,
        &10_250_000,
        &3_600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );
    let before = f.client.get_invoice(&id).expires_at;
    f.client.extend_expiry(&manager, &id, &60);
    assert_eq!(f.client.get_invoice(&id).expires_at, before + 60);

    let outsider = Address::generate(&f.env);
    assert_eq!(
        f.client.try_extend_expiry(&outsider, &id, &60),
        Err(Ok(InvoiceError::Unauthorized))
    );
}

#[test]
fn fee_manager_is_scoped_to_fee_parameters() {
    let f = setup();
    let manager = Address::generate(&f.env);
    f.client.grant_role(&f.admin, &InvoiceRole::FeeManager, &manager);

    assert_eq!(fee_calls_ok(&f, &manager), [true; 2]);
    assert_eq!(config_calls_ok(&f, &manager), [false; 4]);
    assert_eq!(f.client.get_late_fee_bps(), 100);
    assert_eq!(f.client.get_refund_network_fee(), 200);
    // Role holders are still bound by the setters' own validation.
    assert_eq!(
        f.client.try_set_late_fee_bps(&manager, &10_001),
        Err(Ok(InvoiceError::InvalidLateFee))
    );
}

#[test]
fn pause_guardian_can_pause_but_not_unpause() {
    let f = setup();
    let guardian = Address::generate(&f.env);
    f.client
        .grant_role(&f.admin, &InvoiceRole::PauseGuardian, &guardian);

    f.client.pause(&guardian);
    assert!(f.client.is_paused());
    assert_eq!(
        f.client.try_unpause(&guardian),
        Err(Ok(InvoiceError::Unauthorized))
    );
    f.client.unpause(&f.admin);
    assert!(!f.client.is_paused());
    assert_eq!(config_calls_ok(&f, &guardian), [false; 4]);
}

#[test]
fn revoked_role_loses_access() {
    let f = setup();
    let manager = Address::generate(&f.env);
    f.client.grant_role(&f.admin, &InvoiceRole::FeeManager, &manager);
    f.client.set_late_fee_bps(&manager, &50);

    f.client.revoke_role(&f.admin, &InvoiceRole::FeeManager, &manager);
    assert!(!f.client.has_role(&InvoiceRole::FeeManager, &manager));
    assert_eq!(
        f.client.try_set_late_fee_bps(&manager, &75),
        Err(Ok(InvoiceError::Unauthorized))
    );
    assert_eq!(f.client.get_late_fee_bps(), 50);
}

#[test]
fn only_admin_can_grant_or_revoke_roles() {
    let f = setup();
    let manager = Address::generate(&f.env);
    let outsider = Address::generate(&f.env);
    f.client
        .grant_role(&f.admin, &InvoiceRole::ConfigManager, &manager);

    // Neither outsiders nor role holders can escalate or self-grant.
    for caller in [&outsider, &manager] {
        assert_eq!(
            f.client
                .try_grant_role(caller, &InvoiceRole::FeeManager, caller),
            Err(Ok(InvoiceError::Unauthorized))
        );
        assert_eq!(
            f.client
                .try_revoke_role(caller, &InvoiceRole::ConfigManager, &manager),
            Err(Ok(InvoiceError::Unauthorized))
        );
    }
    assert!(!f.client.has_role(&InvoiceRole::FeeManager, &manager));
    assert!(f.client.has_role(&InvoiceRole::ConfigManager, &manager));
    // Role holders cannot rotate the admin either.
    assert!(f.client.try_transfer_admin(&manager, &manager).is_err());
}

#[test]
fn roles_follow_admin_rotation() {
    let f = setup();
    let new_admin = Address::generate(&f.env);
    f.client.transfer_admin(&f.admin, &new_admin);
    f.client.accept_admin(&new_admin);

    // The old admin no longer holds implicit roles; the new one does.
    assert_eq!(fee_calls_ok(&f, &f.admin), [false; 2]);
    assert_eq!(fee_calls_ok(&f, &new_admin), [true; 2]);
}
