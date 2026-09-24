// #474: Measure the instruction cost of set_signer and remove_signer at varying
// signer-list sizes to establish concrete scaling data for the audit document.
//
// Both operations maintain a `DataKey::SignerList` as a `soroban_sdk::Vec<Address>`:
//
//   set_signer (weight > 0):  O(n) contains() scan; push only if not present.
//   set_signer (weight = 0):  O(n) contains() scan + O(n) full-rewrite to exclude the address.
//   remove_signer:            O(n) full-rewrite unconditionally.
//
// Neither path has a MAX_SIGNERS bound in the code (confirmed: issue #21 added
// MAX_BATCH_SIZE = 50, not a signer cap). These tests measure how instruction
// counts grow with list size so the audit doc can cite actual numbers rather
// than big-O asymptotic descriptions.
//
// The Soroban SDK v22 `cost_estimate().resources()` returns the resources consumed
// by the *last* top-level contract invocation. We therefore isolate a single call
// per measurement — each helper function registers a fresh contract, populates the
// list to the desired size, and then makes exactly one measured call.
//
// Run with:
//   cargo test --package comebackhere-treasury --test signer_list_scaling_test -- --nocapture

use soroban_sdk::{testutils::Address as _, Address, Env};
use treasury::{TreasuryContract, TreasuryContractClient};

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

fn setup(env: &Env) -> (TreasuryContractClient<'static>, Address) {
    let admin = Address::generate(env);
    #[allow(deprecated)]
    let contract_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &contract_id);
    client.initialize(&admin, &1, &soroban_sdk::Vec::new(env));
    (client, admin)
}

/// Register `n` distinct signers each with weight 1. Returns their addresses.
fn add_signers(
    client: &TreasuryContractClient,
    admin: &Address,
    env: &Env,
    n: u32,
) -> std::vec::Vec<Address> {
    (0..n)
        .map(|_| {
            let s = Address::generate(env);
            client.set_signer(admin, &s, &1);
            s
        })
        .collect()
}

// ---------------------------------------------------------------------------
// set_signer (add a new signer) at varying list sizes
// ---------------------------------------------------------------------------
// Each call hits the weight > 0 branch:
//   1. contains() scan over the existing list      O(n)
//   2. push_back (signer not already present)      O(1) amortised
//   3. instance().set(SignerList)                  O(n) serialise
//
// The list serialisation is proportional to its length, so the per-call
// instruction cost is O(n) in the current list size before the addition.

fn measure_set_signer_add(list_size_before: u32) -> i64 {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    add_signers(&client, &admin, &env, list_size_before);

    // This is the measured call: add one new signer to a list of size
    // (1 admin + list_size_before).
    let new_signer = Address::generate(&env);
    client.set_signer(&admin, &new_signer, &1);

    env.cost_estimate().resources().instructions
}

// ---------------------------------------------------------------------------
// set_signer (zero-weight / deactivate) at varying list sizes
// ---------------------------------------------------------------------------
// Hits the weight == 0 branch:
//   1. contains() scan                             O(n)
//   2. full rewrite loop (excluding the signer)    O(n)
//   3. instance().set(SignerList) on the new list  O(n)

fn measure_set_signer_zero(list_size: u32) -> i64 {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    let signers = add_signers(&client, &admin, &env, list_size);

    // Zero out the last signer in the list. Single measured call.
    if let Some(last) = signers.last() {
        client.set_signer(&admin, last, &0);
    }

    env.cost_estimate().resources().instructions
}

// ---------------------------------------------------------------------------
// remove_signer at varying list sizes
// ---------------------------------------------------------------------------
// Always hits the full-rewrite path:
//   1. read + deserialise SignerList                O(n)
//   2. rewrite loop (excluding the signer)          O(n)
//   3. instance().set(SignerList) on the new list   O(n)
//   4. instance().remove(Signer(addr))              O(1)

fn measure_remove_signer(list_size: u32) -> i64 {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    let signers = add_signers(&client, &admin, &env, list_size);

    if let Some(last) = signers.last() {
        client.remove_signer(&admin, last);
    }

    env.cost_estimate().resources().instructions
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn set_signer_add_instruction_cost_scales_with_list_size() {
    // list_size_before: number of non-admin signers already registered when
    // the measured call fires. Total list length = 1 (admin) + list_size_before.
    let sizes: &[u32] = &[0, 5, 10, 20, 50, 100];

    eprintln!("\n[signer_scaling] set_signer (add) — instructions for the single measured call");
    eprintln!("{:<30} {:>20}", "total_list_size_at_call", "instructions");
    eprintln!("{:-<51}", "");

    let mut prev = 0i64;
    for &n in sizes {
        let instructions = measure_set_signer_add(n);
        let total_size = n + 1; // +1 for admin
        let delta = if prev == 0 {
            "-".to_string()
        } else {
            format!("+{}", instructions - prev)
        };
        eprintln!("{:<30} {:>20}  delta: {}", total_size, instructions, delta);
        prev = instructions;

        // Sanity: the call must have executed (non-zero instruction count).
        assert!(
            instructions > 0,
            "set_signer (add) at total_list_size={} returned 0 instructions — call did not execute",
            total_size
        );
    }
}

#[test]
fn set_signer_zero_instruction_cost_scales_with_list_size() {
    let sizes: &[u32] = &[5, 10, 20, 50, 100];

    eprintln!(
        "\n[signer_scaling] set_signer (weight=0) — instructions for the single measured call"
    );
    eprintln!("{:<30} {:>20}", "total_list_size_at_call", "instructions");
    eprintln!("{:-<51}", "");

    let mut prev = 0i64;
    for &n in sizes {
        let instructions = measure_set_signer_zero(n);
        let total_size = n + 1;
        let delta = if prev == 0 {
            "-".to_string()
        } else {
            format!("+{}", instructions - prev)
        };
        eprintln!("{:<30} {:>20}  delta: {}", total_size, instructions, delta);
        prev = instructions;

        assert!(
            instructions > 0,
            "set_signer (zero) at total_list_size={} returned 0 instructions",
            total_size
        );
    }
}

#[test]
fn remove_signer_instruction_cost_scales_with_list_size() {
    let sizes: &[u32] = &[5, 10, 20, 50, 100];

    eprintln!("\n[signer_scaling] remove_signer — instructions for the single measured call");
    eprintln!("{:<30} {:>20}", "total_list_size_at_call", "instructions");
    eprintln!("{:-<51}", "");

    let mut prev = 0i64;
    for &n in sizes {
        let instructions = measure_remove_signer(n);
        let total_size = n + 1;
        let delta = if prev == 0 {
            "-".to_string()
        } else {
            format!("+{}", instructions - prev)
        };
        eprintln!("{:<30} {:>20}  delta: {}", total_size, instructions, delta);
        prev = instructions;

        assert!(
            instructions > 0,
            "remove_signer at total_list_size={} returned 0 instructions",
            total_size
        );
    }
}

// ---------------------------------------------------------------------------
// Liveness checks: confirm both operations remain functional at 100+ signers.
// ---------------------------------------------------------------------------

#[test]
fn set_signer_succeeds_at_100_signers() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    add_signers(&client, &admin, &env, 99); // admin + 99 = 100 total

    let all = client.get_all_signers();
    assert_eq!(
        all.len(),
        100,
        "expected 100 signers (admin + 99 registered)"
    );
}

#[test]
fn remove_signer_succeeds_at_100_signers() {
    let env = Env::default();
    env.mock_all_auths();

    let (client, admin) = setup(&env);
    let signers = add_signers(&client, &admin, &env, 99);

    client.remove_signer(&admin, &signers[98]);

    let all = client.get_all_signers();
    assert_eq!(
        all.len(),
        99,
        "expected 99 signers after removing one from a list of 100"
    );
}
