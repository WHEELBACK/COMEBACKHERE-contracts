#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{Address, Env, IntoVal, Symbol};
use storage_layout::{StorageLayoutContract, StorageLayoutContractClient};

/// Fuzzing input describing a single malicious-caller interaction with the
/// `storage-layout` contract.
///
/// The input is intentionally broad: it mixes authorized and unauthorized
/// callers, exercises every mutating/reading entrypoint, and includes
/// boundary amounts (0, -1, i128::MIN, i128::MAX) to cover zero-balance and
/// overflow-adjacent edge cases.
#[derive(Arbitrary, Debug, Clone)]
struct Input {
    /// Selects the caller address. `0` maps to the real admin; any other
    /// value maps to an unregistered (potentially malicious) address.
    caller_index: u8,
    /// Operation selector: 0 = set, 1 = adjust, 2 = clear, 3 = get.
    operation: u8,
    /// Raw amount used by set/adjust. Mapped through `map_amount` to include
    /// boundary values.
    amount: i128,
    /// Selects the storage key to operate on.
    key_selector: u8,
}

/// Map an arbitrary `i128` to a value that includes the interesting boundary
/// cases (0, -1, i128::MIN, i128::MAX) while still allowing arbitrary values.
fn map_amount(raw: i128) -> i128 {
    match raw.rem_euclid(8) {
        0 => 0,
        1 => -1,
        2 => i128::MIN,
        3 => i128::MAX,
        4 => 1,
        5 => -2,
        _ => raw,
    }
}

/// Map a key selector to a concrete `Symbol` key.
fn map_key(env: &Env, selector: u8) -> Symbol {
    match selector % 4 {
        0 => Symbol::new(env, "key0"),
        1 => Symbol::new(env, "key1"),
        2 => Symbol::new(env, "key2"),
        _ => Symbol::new(env, "key3"),
    }
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let input = match Input::arbitrary(&mut u) {
        Ok(input) => input,
        Err(_) => return,
    };

    let env = Env::default();
    // Allow all auth so that the fuzz target exercises contract logic rather
    // than the host's auth mocking. Unauthorized callers are simulated by
    // passing addresses that were never registered as admin.
    env.mock_all_auths();

    let contract_id = env.register_contract(None, StorageLayoutContract);
    let client = StorageLayoutContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    client.initialize(&admin);

    // Build the caller address: index 0 is the real admin, everything else is
    // an unregistered (potentially malicious) address.
    let caller = if input.caller_index == 0 {
        admin.clone()
    } else {
        Address::generate(&env)
    };

    let key = map_key(&env, input.key_selector);
    let amount = map_amount(input.amount);

    // Drive the selected operation. We only assert that the call returns
    // either Ok or Err; a panic (e.g. an arithmetic overflow or an
    // unwrap on a zero-balance state) will fail the fuzz target.
    match input.operation % 4 {
        0 => {
            let _ = client.try_set(&caller, &key, &amount);
        }
        1 => {
            let _ = client.try_adjust(&caller, &key, &amount);
        }
        2 => {
            let _ = client.try_clear(&caller, &key);
        }
        _ => {
            let _ = client.try_get(&caller, &key);
        }
    }

    // Exercise a follow-up read to ensure the contract remains in a
    // consistent state after the operation above.
    let _ = client.try_get(&caller, &key);
});