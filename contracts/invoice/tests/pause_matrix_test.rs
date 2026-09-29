//! Pause matrix test for the invoice contract.
//!
//! Asserts that every state-changing entrypoint is blocked while the contract
//! is paused (returning the typed `ContractPaused` error) and that read-only
//! entrypoints keep working. Table-driven so that adding a new entrypoint
//! without a pause check is caught immediately.

use comebackhere_invoice::{InvoiceContract, InvoiceContractClient, InvoiceError};
use soroban_sdk::{testutils::Address as _, Address, Env, String};

/// A single entrypoint under test and the arguments needed to invoke it.
struct Entrypoint {
    name: &'static str,
    /// `true` when the entrypoint mutates state and must be blocked while paused.
    mutating: bool,
}

fn entrypoints() -> [Entrypoint; 8] {
    [
        Entrypoint { name: "create_invoice", mutating: true },
        Entrypoint { name: "pay_invoice", mutating: true },
        Entrypoint { name: "cancel_invoice", mutating: true },
        Entrypoint { name: "refund_invoice", mutating: true },
        Entrypoint { name: "update_invoice", mutating: true },
        Entrypoint { name: "archive_invoice", mutating: true },
        Entrypoint { name: "get_invoice", mutating: false },
        Entrypoint { name: "list_invoices", mutating: false },
    ]
}

fn setup() -> (Env, InvoiceContractClient<'static>, Address) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    client.initialize(&admin);
    (env, client, admin)
}

#[test]
fn pause_matrix_blocks_mutating_entrypoints() {
    let (env, client, admin) = setup();
    client.pause(&admin);

    for entry in entrypoints().iter().filter(|e| e.mutating) {
        let result = match entry.name {
            "create_invoice" => client.try_create_invoice(
                &admin,
                &Address::generate(&env),
                &100_i128,
                &String::from_str(&env, "USD"),
            ),
            "pay_invoice" => client.try_pay_invoice(&admin, &1_u64),
            "cancel_invoice" => client.try_cancel_invoice(&admin, &1_u64),
            "refund_invoice" => client.try_refund_invoice(&admin, &1_u64),
            "update_invoice" => client.try_update_invoice(&admin, &1_u64, &200_i128),
            "archive_invoice" => client.try_archive_invoice(&admin, &1_u64),
            other => panic!("unhandled mutating entrypoint: {other}"),
        };

        assert_eq!(
            result,
            Err(Ok(InvoiceError::ContractPaused)),
            "entrypoint `{}` must be blocked while paused",
            entry.name
        );
    }
}

#[test]
fn pause_matrix_allows_read_only_entrypoints() {
    let (env, client, admin) = setup();
    client.pause(&admin);

    for entry in entrypoints().iter().filter(|e| !e.mutating) {
        match entry.name {
            "get_invoice" => {
                let _ = client.get_invoice(&1_u64);
            }
            "list_invoices" => {
                let _ = client.list_invoices();
            }
            other => panic!("unhandled read-only entrypoint: {other}"),
        }
    }
}
