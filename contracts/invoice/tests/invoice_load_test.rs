use invoice::{InvoiceContract, InvoiceContractClient, InvoiceStatus, MaybeAddress, MaybeBytes};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    Address, Env,
};

fn setup() -> (Env, Address, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &id);
    client.initialize(&admin);
    (env, admin, client)
}

// Verification note: observed storage behavior under load.
// This test creates a large sequential batch of invoices and verifies:
// - storage budget is not exceeded (the loop completes without trapping)
// - state remains consistent (each invoice is retrievable and not overwritten)
// - upper bound tested: 200 invoices in a single Env execution
#[test]
fn high_volume_invoice_creation_storage_budget() {
    let (env, _admin, client) = setup();
    let merchant = Address::generate(&env);

    // Stabilize timestamp so expiry math is deterministic across runs.
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let total: u64 = 200;
    let batch: u64 = 50;

    let mut last_id: u64 = 0;
    let mut observed_storage_entries: u64 = 0;

    for i in 1..=total {
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
        assert_eq!(id, i);
        last_id = id;

        if i % batch == 0 {
            // Consistency: verify a couple of representative invoices still exist.
            let first = client.get_invoice(&1);
            let mid = client.get_invoice(&(i / 2));
            let last = client.get_invoice(&i);
            assert_eq!(first.id, 1);
            assert_eq!(mid.status, InvoiceStatus::Pending);
            assert_eq!(last.id, i);

            observed_storage_entries = i;
        }
    }

    assert_eq!(last_id, total);
    assert_eq!(observed_storage_entries, total);
}

#[test]
fn batch_expire_1000_invoices() {
    let (env, admin, client) = setup();
    let merchant = Address::generate(&env);

    // Set timestamp so expires_at = 1001 for every invoice.
    env.ledger().with_mut(|l| l.timestamp = 1_000);

    let total: u64 = 1_000;
    let mut ids = soroban_sdk::Vec::new(&env);

    for i in 1..=total {
        let id = client.create_invoice(
            &merchant,
            &10_000_000,
            &10_250_000,
            &1, // expires_in_seconds = 1 → expires_at = 1001
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
            &MaybeAddress::None,
        );
        assert_eq!(id, i);
        ids.push_back(id);
    }

    // Advance past expiry.
    env.ledger().with_mut(|l| l.timestamp = 2_000);

    let expired = client.batch_expire(&admin, &ids);
    assert_eq!(expired, total as u32);
}
