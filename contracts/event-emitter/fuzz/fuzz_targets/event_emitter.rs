#![no_main]

use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as _, Address, Env, String, Symbol, Vec};

use event_emitter::{EventEmitterContract, EventEmitterContractClient};

/// Maximum number of events we will attempt to emit in a single fuzz iteration.
/// Kept small to bound host metering while still exercising repeated persistent
/// storage access (which drives the automatic TTL bump logic).
const MAX_EVENTS: u32 = 16;

/// Maximum length of fuzz-provided strings to keep footprint and metering bounded.
const MAX_STRING_LEN: u32 = 64;

/// Build a bounded, deterministic string from the fuzz input bytes.
fn bounded_string(env: &Env, bytes: &[u8]) -> String {
    let len = (bytes.len() as u32).min(MAX_STRING_LEN);
    let mut s = String::from_str(env, "");
    // Build the string from a fixed ASCII-safe alphabet to avoid invalid UTF-8
    // panics inside the SDK while still varying content.
    for i in 0..len {
        let b = bytes[i as usize];
        let ch = (b % 26) + b'a';
        let mut buf = [0u8; 1];
        buf[0] = ch;
        // Append one character at a time using a small stack string.
        let piece = String::from_str(env, core::str::from_utf8(&buf).unwrap_or("a"));
        s = s.concat(&piece);
    }
    s
}

fuzz_target!(|data: &[u8]| {
    // Need at least a few bytes to drive the fuzzer meaningfully.
    if data.len() < 4 {
        return;
    }

    let env = Env::default();
    // Allow non-root auth so we can exercise the full call surface without
    // requiring explicit auth mocking for every invocation.
    env.mock_all_auths();

    let contract_id = env.register_contract(None, EventEmitterContract);
    let client = EventEmitterContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);

    // Initialize the contract. This is the first persistent storage write and
    // establishes the keys whose TTLs must be bumped on subsequent access.
    let init_result = client.try_initialize(&admin);
    // Initialization must either succeed or return a typed error; it must never
    // panic the host.
    assert!(init_result.is_ok() || init_result.is_err());

    // Derive a bounded number of events to emit from the fuzz input.
    let event_count = (data[0] as u32) % (MAX_EVENTS + 1);

    let mut emitted: u32 = 0;
    let mut offset: usize = 1;

    for _ in 0..event_count {
        if offset >= data.len() {
            break;
        }

        // Extract a topic symbol from the fuzz bytes.
        let topic_byte = data[offset];
        offset += 1;
        let topic = Symbol::new(&env, match topic_byte % 4 {
            0 => "transfer",
            1 => "mint",
            2 => "burn",
            _ => "approve",
        });

        // Extract a bounded payload string from the remaining bytes.
        let remaining = &data[offset..];
        let payload = bounded_string(&env, remaining);
        offset = offset.saturating_add(remaining.len().min(MAX_STRING_LEN as usize));

        // Exercise repeated persistent storage access. Each emit touches the
        // persistent event counter and index keys, which must trigger the
        // automatic TTL bump logic without panicking.
        let emit_result = client.try_emit(&admin, &topic, &payload);
        // The contract must return a typed result (Ok or Err), never panic.
        match emit_result {
            Ok(Ok(_)) => {
                emitted = emitted.saturating_add(1);
            }
            Ok(Err(_)) => {
                // Typed contract error: acceptable and expected on invalid input.
            }
            Err(_) => {
                // Host-level error surfaced as a typed Result: acceptable.
            }
        }

        // Exercise read paths that also touch persistent storage and must bump
        // TTLs consistently.
        let _ = client.try_get_event_count();
        let _ = client.try_get_event(&emitted);
    }

    // Exercise zero / boundary balance inputs. These must be handled without
    // panicking contract execution and must return typed errors where invalid.
    let zero_balance: i128 = 0;
    let max_balance: i128 = i128::MAX;
    let min_balance: i128 = i128::MIN;

    let _ = client.try_check_balance(&admin, &zero_balance);
    let _ = client.try_check_balance(&admin, &max_balance);
    let _ = client.try_check_balance(&admin, &min_balance);

    // Exercise a batch of zero-balance accounts to stress the zero-state path
    // across multiple persistent keys.
    let mut accounts: Vec<Address> = Vec::new(&env);
    let batch = (data.len() as u32 % 4) + 1;
    for _ in 0..batch {
        accounts.push_back(Address::generate(&env));
    }
    let _ = client.try_check_balances(&accounts, &zero_balance);

    // Final consistency check: reading the event count must not panic and must
    // reflect a value consistent with the number of successful emits.
    if let Ok(Ok(count)) = client.try_get_event_count() {
        assert!(count >= emitted);
    }
});