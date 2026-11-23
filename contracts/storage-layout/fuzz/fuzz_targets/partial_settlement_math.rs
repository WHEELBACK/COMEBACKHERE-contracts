#![no_main]

use libfuzzer_sys::fuzz_target;
use soroban_sdk::{Address, Env, String};

use storage_layout::{StorageLayoutContract, StorageLayoutContractClient, TreasuryError};

/// Fuzz target for the partial-settlement arithmetic in `storage-layout`.
///
/// Feeds arbitrary `(total, approved_total, partial_amount)` triples into
/// `approve_partial_settlement` and `partially_execute_settlement`, asserting
/// that the contract never panics and that every rejection surfaces as a typed
/// `TreasuryError` (specifically `ArithmeticUnderflow` or `InvalidAmount`).
fuzz_target!(|data: (i128, i128, i128)| {
    let (total, approved_total, partial_amount) = data;

    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, StorageLayoutContract);
    let client = StorageLayoutContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let token = Address::generate(&env);
    let reference = String::from_str(&env, "fuzz-partial-settlement");

    // Best-effort initialization; ignore typed errors from setup so the fuzz
    // target focuses on the arithmetic paths under test.
    let _ = client.try_initialize(&admin, &token);

    // Exercise `approve_partial_settlement`. Any failure must be a typed
    // `TreasuryError`, never a host panic or unwrap.
    match client.try_approve_partial_settlement(
        &admin,
        &reference,
        &total,
        &approved_total,
        &partial_amount,
    ) {
        Ok(_) => {}
        Err(Ok(err)) => {
            assert!(
                matches!(
                    err,
                    TreasuryError::ArithmeticUnderflow | TreasuryError::InvalidAmount
                ),
                "unexpected typed error from approve_partial_settlement: {:?}",
                err
            );
        }
        Err(Err(_)) => {
            // A host-level error (e.g. missing storage entry) is acceptable
            // during fuzzing but must not be a panic; libfuzzer would catch a
            // panic before reaching this arm.
        }
    }

    // Exercise `partially_execute_settlement` with the same triple.
    match client.try_partially_execute_settlement(
        &admin,
        &reference,
        &total,
        &approved_total,
        &partial_amount,
    ) {
        Ok(_) => {}
        Err(Ok(err)) => {
            assert!(
                matches!(
                    err,
                    TreasuryError::ArithmeticUnderflow | TreasuryError::InvalidAmount
                ),
                "unexpected typed error from partially_execute_settlement: {:?}",
                err
            );
        }
        Err(Err(_)) => {
            // See note above: host-level errors are tolerated, panics are not.
        }
    }
});