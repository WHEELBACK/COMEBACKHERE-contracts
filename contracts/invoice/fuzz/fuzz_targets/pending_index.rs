#![no_main]

// Fuzz harness (cargo-fuzz) complementing the invoice unit tests. The pending
// index backs `get_pending_ids` and `batch_expire`, and must always contain
// exactly the invoices that are currently pending. Unit tests cover specific
// paths, but index bookkeeping bugs tend to surface only after unusual
// sequences of operations. This harness performs random sequences of create,
// pay, cancel, expire and refund, and after each step compares the contract's
// `get_pending_ids` against a simple in-memory model set of currently-pending
// invoices. It follows the layout of the `amount_precision` target so it runs
// with the same tooling.

use arbitrary::Arbitrary;
use invoice::{InvoiceContract, InvoiceContractClient, MaybeAddress, MaybeBytes};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{testutils::Address as _, Address, Env, Vec};

#[derive(Debug, Arbitrary)]
struct Op {
    kind: u8,
    amount_usdc: i128,
    gross_usdc: i128,
    expires_in_seconds: u64,
    merchant_nonce: u64,
}

#[derive(Debug, Arbitrary)]
struct Input {
    ops: Vec<Op>,
}

fuzz_target!(|input: Input| {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let merchant = Address::generate(&env);
    let payer = Address::generate(&env);
    let contract_id = env.register_contract(None, InvoiceContract);
    let client = InvoiceContractClient::new(&env, &contract_id);
    client.initialize(&admin);

    // Model of the currently-pending invoice ids. Kept in sync with the
    // contract's pending index as operations succeed.
    let mut model: Vec<u64> = Vec::new(&env);
    let mut next_nonce: u64 = 0;

    for op in input.ops.iter() {
        match op.kind % 5 {
            // create
            0 => {
                let nonce = next_nonce;
                next_nonce = next_nonce.wrapping_add(1);
                let res = client.try_create_invoice(
                    &merchant,
                    &op.amount_usdc,
                    &op.gross_usdc,
                    &op.expires_in_seconds,
                    &MaybeBytes::None,
                    &MaybeBytes::None,
                    &nonce,
                    &MaybeAddress::None,
                );
                if let Ok(Ok(id)) = res {
                    model.push_back(id);
                }
            }
            // pay
            1 => {
                if let Some(id) = pick(&env, &model, op.merchant_nonce) {
                    let res = client.try_pay_invoice(&payer, &id, &MaybeBytes::None);
                    if matches!(res, Ok(Ok(_))) {
                        remove(&mut model, id);
                    }
                }
            }
            // cancel
            2 => {
                if let Some(id) = pick(&env, &model, op.merchant_nonce) {
                    let res = client.try_cancel_invoice(&merchant, &id);
                    if matches!(res, Ok(Ok(_))) {
                        remove(&mut model, id);
                    }
                }
            }
            // expire
            3 => {
                if let Some(id) = pick(&env, &model, op.merchant_nonce) {
                    let res = client.try_expire_invoice(&id);
                    if matches!(res, Ok(Ok(_))) {
                        remove(&mut model, id);
                    }
                }
            }
            // refund
            _ => {
                if let Some(id) = pick(&env, &model, op.merchant_nonce) {
                    let res = client.try_refund_invoice(&merchant, &id);
                    if matches!(res, Ok(Ok(_))) {
                        remove(&mut model, id);
                    }
                }
            }
        }

        // The pending index must contain exactly the model's pending ids.
        let pending = client.get_pending_ids();
        assert_eq!(pending, model, "pending index diverged from model");
    }
});

fn pick(env: &Env, model: &Vec<u64>, seed: u64) -> Option<u64> {
    if model.is_empty() {
        return None;
    }
    let idx = (seed % model.len() as u64) as u32;
    model.get(idx)
}

fn remove(model: &mut Vec<u64>, id: u64) {
    if let Some(pos) = model.iter().position(|x| x == id) {
        model.remove(pos as u32);
    }
}
