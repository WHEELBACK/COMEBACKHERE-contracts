#![no_main]

// Fuzz harness (cargo-fuzz) complementing invoice_amount_invariant_test.rs and
// invoice_amount_property_test.rs. Those cover known logic invariants with
// hand-picked/proptest cases; this harness throws arbitrary i128/u64 inputs at
// create_invoice's amount_usdc/gross_usdc validation path to catch panics
// (overflow, unwrap-on-None, etc.) that logic-only assertions would miss.
// Keep iteration cost low (single contract call per input) so this stays
// CI-runnable within a bounded time/iteration budget.

use arbitrary::Arbitrary;
use invoice::{InvoiceContract, InvoiceContractClient, MaybeAddress, MaybeBytes};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as _, Address, Env};

#[derive(Debug, Arbitrary)]
struct Input {
    amount_usdc: i128,
    gross_usdc: i128,
    expires_in_seconds: u64,
    merchant_nonce: u64,
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let contract_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    // Only the Result matters here: any Ok/Err is acceptable, a panic is not.
    let _ = client.try_create_invoice(
        &merchant,
        &input.amount_usdc,
        &input.gross_usdc,
        &input.expires_in_seconds,
        &MaybeBytes::None,
        &MaybeBytes::None,
        &input.merchant_nonce,
        &MaybeAddress::None,
    );
});
