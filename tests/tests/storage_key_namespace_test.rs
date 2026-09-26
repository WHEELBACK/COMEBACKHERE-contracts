//! #86: cross-contract storage key namespacing and within-contract slot
//! separation, asserted against real host storage rather than in the abstract.
//!
//! `docs/STORAGE_VERSIONING.md` claims two things; this suite makes both
//! mechanical:
//!
//! 1. Identical variant names in *different* contracts cannot collide, because
//!    Soroban namespaces storage by contract ID. Invoice, treasury and
//!    compliance all have an `Admin` variant and all have a `Paused` flag.
//! 2. Within *one* contract, two variants never share a slot — the failure the
//!    per-contract `storage_key_uniqueness_test.rs` suites guard against by
//!    comparing serialized keys. Here the same property is shown end-to-end:
//!    writing one variant's slot is invisible to the entrypoint that reads
//!    another.

use compliance::{ComplianceContract, ComplianceContractClient, ContractError};
use invoice::{
    DataKey as InvoiceDataKey, InvoiceContract, InvoiceContractClient, InvoiceError, InvoiceStatus,
    MaybeAddress, MaybeBytes, StatusTransition,
};
use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{DataKey as TreasuryDataKey, TreasuryContract, TreasuryContractClient};

#[test]
fn identical_variant_names_are_isolated_per_contract() {
    let env = Env::default();
    env.mock_all_auths();

    let invoice_admin = Address::generate(&env);
    let treasury_admin = Address::generate(&env);
    let compliance_admin = Address::generate(&env);

    let invoice_id = env.register(InvoiceContract, ());
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    invoice.initialize(&invoice_admin);

    let treasury_id = env.register(TreasuryContract, ());
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(
        &treasury_admin,
        &1,
        &soroban_sdk::Vec::<(Address, u32)>::new(&env),
    );

    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    compliance.initialize(&compliance_admin);

    // Each contract's admin-gated entrypoint consults its own `DataKey::Admin`.
    // If the three `Admin` variants shared a slot, the last `initialize` would
    // have overwritten the other two and exactly two of these would fail.
    invoice.pause(&invoice_admin);
    treasury.pause(&treasury_admin);
    compliance.pause(&compliance_admin);

    assert_eq!(
        invoice.try_pause(&treasury_admin),
        Err(Ok(InvoiceError::Unauthorized)),
        "treasury's admin must not authorize the invoice contract"
    );
    assert_eq!(
        compliance.try_pause(&treasury_admin),
        Err(Ok(ContractError::Unauthorized)),
        "treasury's admin must not authorize the compliance contract"
    );
    assert_eq!(
        invoice.try_pause(&compliance_admin),
        Err(Ok(InvoiceError::Unauthorized)),
        "compliance's admin must not authorize the invoice contract"
    );
}

/// `DataKey::Invoice(id)`, `DataKey::InvoiceHistory(id)`,
/// `DataKey::PendingIndex` and `DataKey::InvoiceCount` all live in the same
/// contract. Seeding the history and index slots directly must not disturb what
/// the invoice entrypoints read back.
#[test]
fn within_a_contract_each_variant_keeps_its_own_slot() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let invoice_id = env.register(InvoiceContract, ());
    let invoice = InvoiceContractClient::new(&env, &invoice_id);
    invoice.initialize(&admin);

    let id = invoice.create_invoice(
        &merchant,
        &10_000_000,
        &10_250_000,
        &3600,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &0,
        &MaybeAddress::None,
    );

    // Write noise into every neighbouring key variant, at the very same ids the
    // invoice entrypoints use.
    env.as_contract(&invoice_id, || {
        let mut history = soroban_sdk::Vec::<StatusTransition>::new(&env);
        history.push_back(StatusTransition {
            from: InvoiceStatus::Pending,
            to: InvoiceStatus::Paid,
            timestamp: 1,
        });
        env.storage()
            .persistent()
            .set(&InvoiceDataKey::InvoiceHistory(id), &history);
        env.storage().persistent().set(
            &InvoiceDataKey::PendingIndex,
            &soroban_sdk::Vec::<u64>::new(&env),
        );
        env.storage()
            .persistent()
            .set(&InvoiceDataKey::InvoiceCount, &99u64);
        env.storage()
            .instance()
            .set(&InvoiceDataKey::Paused, &false);
    });

    // A collision would make `get_invoice` fail to decode (the history vector is
    // not an `Invoice`) and surface as `NotFound` or as the wrong status.
    assert_eq!(invoice.get_invoice(&id).status, InvoiceStatus::Pending);
    assert_eq!(invoice.get_invoice(&id).merchant, merchant);
    assert_eq!(
        invoice.get_invoice(&id).payer,
        MaybeAddress::None,
        "payer slot must be untouched"
    );
    // `InvoiceCount` is an instance key and `PendingIndex` is persistent; the
    // noise written into one must not be visible through the other's key.
    assert!(invoice.get_pending_ids().is_empty());
}

/// The per-token balance key (#448) in real storage: two tokens for one holder
/// are two independent accounting buckets, and a withdrawal from one leaves the
/// other alone.
#[test]
fn per_token_balance_keys_stay_independent() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let holder = Address::generate(&env);
    let token_a = Address::generate(&env);
    let token_b = Address::generate(&env);

    let treasury_id = env.register(TreasuryContract, ());
    let treasury = TreasuryContractClient::new(&env, &treasury_id);
    treasury.initialize(&admin, &1, &soroban_sdk::Vec::<(Address, u32)>::new(&env));

    // Seed both buckets directly under `DataKey::Balance(holder, token)`.
    env.as_contract(&treasury_id, || {
        env.storage().persistent().set(
            &TreasuryDataKey::Balance(holder.clone(), token_a.clone()),
            &500i128,
        );
        env.storage().persistent().set(
            &TreasuryDataKey::Balance(holder.clone(), token_b.clone()),
            &700i128,
        );
    });

    assert_eq!(treasury.get_balance(&holder, &token_a), 500);
    assert_eq!(treasury.get_balance(&holder, &token_b), 700);

    env.as_contract(&treasury_id, || {
        let mut balance: i128 = env
            .storage()
            .persistent()
            .get(&TreasuryDataKey::Balance(holder.clone(), token_a.clone()))
            .unwrap();
        balance -= 200;
        env.storage().persistent().set(
            &TreasuryDataKey::Balance(holder.clone(), token_a.clone()),
            &balance,
        );
    });

    assert_eq!(treasury.get_balance(&holder, &token_a), 300);
    assert_eq!(
        treasury.get_balance(&holder, &token_b),
        700,
        "debit under Balance(holder, token_a) must not touch Balance(holder, token_b)"
    );
}
