#![no_main]

use libfuzzer_sys::fuzz_target;
use recovery_manager::{RecoveryManager, RecoveryManagerClient};
use soroban_sdk::{testutils::Address as _, Address, Env};

fuzz_target!(|data: (u64, u64, u64, u64)| {
    let (balance, settled, requested, extra) = data;

    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register_contract(None, RecoveryManager);
    let client = RecoveryManagerClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let user = Address::generate(&env);

    // Initialize the contract with the fuzzed balance.
    client.initialize(&admin, &user, &balance);

    // Attempt a partial settlement with arbitrary amounts. The contract must
    // never panic; underflow conditions must surface as typed errors.
    let result = client.try_partial_settlement(&user, &settled, &requested, &extra);

    match result {
        Ok(_) => {
            // If the settlement succeeded, the remaining balance must be
            // consistent and never have underflowed.
            let remaining = client.get_balance(&user);
            assert!(remaining <= balance);
        }
        Err(_) => {
            // Typed error surfaced instead of a panic. This is the expected
            // path for underflow conditions.
        }
    }

    // A second settlement attempt must also never panic.
    let _ = client.try_partial_settlement(&user, &requested, &settled, &extra);
});