// Differential fuzzing test: compares Rust invoice amount validation against
// an independent Python reference implementation with the same test vectors.
// This catches bugs that might be shared between implementation and test if
// both were written in the same language with the same mental model.

use invoice::{InvoiceContract, InvoiceContractClient, MaybeAddress, MaybeBytes};
use serde_json::Value;
use soroban_sdk::{testutils::Address as _, Address, Env};
use std::io::Write;
use std::process::Command;

const USDC_FACTOR: i128 = 10_000_000;

fn client() -> (Env, InvoiceContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let id = env.register_contract(None, InvoiceContract);
    let c = InvoiceContractClient::new(&env, &id);
    c.initialize(&admin);
    (env, c)
}

/// Calls the Python reference implementation with the given amounts.
/// Returns (valid, error_name) where valid=true means no error, valid=false means error with error_name.
fn python_validate(amount_usdc: i128, gross_usdc: i128) -> (bool, Option<String>) {
    // Use string representation to avoid JSON number overflow issues with large i128 values
    let input = format!(
        r#"{{"amount_usdc": {}, "gross_usdc": {}}}"#,
        amount_usdc, gross_usdc
    );

    // Resolve the reference script from the workspace root relative to this
    // crate, so the test works regardless of where the checkout lives (a
    // devcontainer, a CI runner, a local clone).
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/reference_amount_validation.py"
    );
    let mut child = Command::new("python3")
        .arg(script)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn Python process");

    {
        let stdin = child.stdin.as_mut().expect("failed to open stdin");
        stdin
            .write_all(input.as_bytes())
            .expect("failed to write to stdin");
    }

    let output = child.wait_with_output().expect("failed to wait on Python");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let result: Value =
        serde_json::from_str(&stdout).expect(&format!("failed to parse Python output: {}", stdout));

    let valid = result["valid"].as_bool().expect("missing 'valid' field");
    let error = if let Some(e) = result["error"].as_str() {
        Some(e.to_string())
    } else if result["error"].is_null() {
        None
    } else {
        Some(result["error"].to_string())
    };

    (valid, error)
}

/// Attempts to create an invoice with the given amounts via the Rust implementation.
/// Returns true if successful, false if rejected.
fn rust_validate(amount_usdc: i128, gross_usdc: i128) -> bool {
    let (env, client) = client();
    let merchant = Address::generate(&env);

    client
        .try_create_invoice(
            &merchant,
            &amount_usdc,
            &gross_usdc,
            &3600,
            &MaybeBytes::None,
            &MaybeBytes::None,
            &0,
            &MaybeAddress::None,
        )
        .is_ok()
}

#[test]
fn differential_fuzz_canonical_test_cases() {
    // Test cases that must be handled the same way in both implementations.
    let test_cases: &[(i128, i128)] = &[
        // Invalid: negative amounts
        (-1, USDC_FACTOR),
        (-USDC_FACTOR, USDC_FACTOR),
        (i128::MIN, i128::MAX),
        // Invalid: zero amounts
        (0, 0),
        (0, USDC_FACTOR),
        // Invalid: gross < amount
        (USDC_FACTOR, USDC_FACTOR - 1),
        (2 * USDC_FACTOR, USDC_FACTOR),
        // Invalid: below USDC_FACTOR precision
        (1, 1),
        (100, USDC_FACTOR),
        (USDC_FACTOR - 1, USDC_FACTOR - 1),
        (USDC_FACTOR - 1, USDC_FACTOR),
        (USDC_FACTOR, USDC_FACTOR - 1),
        // Valid: minimum valid amounts
        (USDC_FACTOR, USDC_FACTOR),
        (USDC_FACTOR, 2 * USDC_FACTOR),
        // Valid: round numbers
        (10 * USDC_FACTOR, 10 * USDC_FACTOR),
        (100 * USDC_FACTOR, 100 * USDC_FACTOR),
        (1000 * USDC_FACTOR, 1000 * USDC_FACTOR),
        // Valid: large amounts
        (i128::MAX / 2, i128::MAX / 2),
        (i128::MAX, i128::MAX),
    ];

    for &(amount, gross) in test_cases {
        let rust_ok = rust_validate(amount, gross);
        let (python_ok, _python_error) = python_validate(amount, gross);

        assert_eq!(
            rust_ok, python_ok,
            "Differential mismatch for amount={} gross={}: Rust said {}, Python said {}",
            amount, gross, rust_ok, python_ok
        );
    }
}

#[test]
fn differential_fuzz_boundary_cases() {
    // Systematic boundary testing around USDC_FACTOR
    let boundaries: &[i128] = &[
        USDC_FACTOR - 2,
        USDC_FACTOR - 1,
        USDC_FACTOR,
        USDC_FACTOR + 1,
        USDC_FACTOR + 2,
        2 * USDC_FACTOR - 1,
        2 * USDC_FACTOR,
        2 * USDC_FACTOR + 1,
    ];

    for &a in boundaries {
        for &g in boundaries {
            if a > 0 && g > 0 && g >= a {
                // Skip cases that would be too slow to test with Python subprocess each time
                // (but this is a boundary test, not exhaustive)
                let rust_ok = rust_validate(a, g);
                let (python_ok, python_error) = python_validate(a, g);

                assert_eq!(
                    rust_ok, python_ok,
                    "Differential mismatch at boundary: amount={} gross={}: Rust={}, Python={} ({:?})",
                    a, g, rust_ok, python_ok, python_error
                );
            }
        }
    }
}

#[test]
fn differential_fuzz_off_by_one() {
    // Test values one off from precision boundaries
    let values: &[i128] = &[
        USDC_FACTOR - 1,
        USDC_FACTOR,
        USDC_FACTOR + 1,
        10 * USDC_FACTOR - 1,
        10 * USDC_FACTOR,
        10 * USDC_FACTOR + 1,
    ];

    for &amount in values {
        for &gross in values {
            if amount > 0 && gross >= amount {
                let rust_ok = rust_validate(amount, gross);
                let (python_ok, python_error) = python_validate(amount, gross);

                assert_eq!(
                    rust_ok, python_ok,
                    "Off-by-one mismatch: amount={} gross={}: Rust={}, Python={} ({:?})",
                    amount, gross, rust_ok, python_ok, python_error
                );
            }
        }
    }
}
