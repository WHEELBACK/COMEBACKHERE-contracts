//! Shared malicious-token harness for the treasury reentrancy suite (issue #118).
//!
//! `ReentrancyToken` is a SEP-41–shaped contract whose `transfer` re-enters a
//! configurable treasury entrypoint exactly once per invocation. A depth counter
//! in instance storage bounds recursion so the test never trips Soroban's
//! cross-contract call-depth limit.
//!
//! Each test calls one of the `set_*` helpers to wire up the callback and any
//! parameters the target entrypoint needs (signer, settlement ID, partial
//! amount, etc.) before triggering the original treasury call.

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};
use treasury::TreasuryContractClient;

/// Which treasury entrypoint `ReentrancyToken::transfer` should re-enter on
/// its outer invocation. Selectable per-test via `set_callback_target`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CallbackTarget {
    None,
    Deposit,
    Withdraw,
    WithdrawAll,
    ExecuteSettlement,
    PartiallyExecuteSettlement,
}

/// Reusable SEP-41 mock that re-enters the treasury during `transfer`.
#[contract]
pub struct ReentrancyToken;

#[contractimpl]
impl ReentrancyToken {
    pub fn mint(env: Env, to: Address, amount: i128) {
        let k = ("bal", to.clone());
        let b: i128 = env.storage().persistent().get(&k).unwrap_or(0);
        env.storage().persistent().set(&k, &(b + amount));
    }

    pub fn balance(env: Env, of: Address) -> i128 {
        let k = ("bal", of);
        env.storage().persistent().get(&k).unwrap_or(0)
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();

        // Depth counter — incremented on every `transfer` entry. The outer
        // call (depth=0) performs the configured callback; recursive reentries
        // (depth>=1) skip the callback so the test never infinite-recurses.
        let depth: u32 = env
            .storage()
            .instance()
            .get(&("rt_depth",))
            .unwrap_or(0);
        env.storage().instance().set(&("rt_depth",), &(depth + 1));

        let target: CallbackTarget = env
            .storage()
            .instance()
            .get(&("rt_target",))
            .unwrap_or(CallbackTarget::None);

        if depth == 0 && target != CallbackTarget::None {
            let treasury_addr: Address = env
                .storage()
                .instance()
                .get(&("rt_treasury",))
                .unwrap();
            let client = TreasuryContractClient::new(&env, &treasury_addr);

            match target {
                CallbackTarget::Deposit => {
                    let depositor: Address = env
                        .storage()
                        .instance()
                        .get(&("rt_depositor",))
                        .unwrap();
                    client.deposit(&depositor, &env.current_contract_address(), &amount);
                }
                CallbackTarget::Withdraw => {
                    // Balance was already decremented by the outer `withdraw`,
                    // so this re-entry should panic with `InsufficientBalance`
                    // when called with the same amount — proving the CEI
                    // ordering protects `withdraw`.
                    client.withdraw(&to, &env.current_contract_address(), &amount);
                }
                CallbackTarget::WithdrawAll => {
                    let admin: Address = env
                        .storage()
                        .instance()
                        .get(&("rt_admin",))
                        .unwrap();
                    let recipient: Address = env
                        .storage()
                        .instance()
                        .get(&("rt_recipient",))
                        .unwrap();
                    client.withdraw_all(&admin, &env.current_contract_address(), &recipient);
                }
                CallbackTarget::ExecuteSettlement => {
                    let signer: Address = env
                        .storage()
                        .instance()
                        .get(&("rt_signer",))
                        .unwrap();
                    let sid: u64 = env
                        .storage()
                        .instance()
                        .get(&("rt_sid",))
                        .unwrap();
                    client.execute_settlement(&signer, &sid, &env.current_contract_address());
                }
                CallbackTarget::PartiallyExecuteSettlement => {
                    let signer: Address = env
                        .storage()
                        .instance()
                        .get(&("rt_signer",))
                        .unwrap();
                    let sid: u64 = env
                        .storage()
                        .instance()
                        .get(&("rt_sid",))
                        .unwrap();
                    let partial: i128 = env
                        .storage()
                        .instance()
                        .get(&("rt_partial",))
                        .unwrap();
                    client.partially_execute_settlement(
                        &signer,
                        &sid,
                        &partial,
                        &env.current_contract_address(),
                    );
                }
                CallbackTarget::None => {}
            }
        }

        // Actual token bookkeeping — does NOT check the from-balance, which
        // simulates an attacker-controlled token. The depth counter guarantees
        // this branch always runs exactly twice (outer + inner reentry).
        let from_key = ("bal", from.clone());
        let to_key = ("bal", to.clone());
        let from_bal: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
        let to_bal: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&from_key, &(from_bal - amount));
        env.storage().persistent().set(&to_key, &(to_bal + amount));
    }

    // -------- configuration setters --------

    pub fn set_callback_target(env: Env, target: CallbackTarget, treasury: Address) {
        env.storage().instance().set(&("rt_target",), &target);
        env.storage().instance().set(&("rt_treasury",), &treasury);
        // Reset depth so a subsequent `transfer` is treated as the outer call.
        env.storage().instance().set(&("rt_depth",), &0u32);
    }

    pub fn set_depositor(env: Env, depositor: Address) {
        env.storage().instance().set(&("rt_depositor",), &depositor);
    }

    pub fn set_withdraw_all_params(env: Env, admin: Address, recipient: Address) {
        env.storage().instance().set(&("rt_admin",), &admin);
        env.storage().instance().set(&("rt_recipient",), &recipient);
    }

    pub fn set_execute_settlement_params(env: Env, signer: Address, sid: u64) {
        env.storage().instance().set(&("rt_signer",), &signer);
        env.storage().instance().set(&("rt_sid",), &sid);
    }

    pub fn set_partial_execute_params(env: Env, signer: Address, sid: u64, partial: i128) {
        env.storage().instance().set(&("rt_signer",), &signer);
        env.storage().instance().set(&("rt_sid",), &sid);
        env.storage().instance().set(&("rt_partial",), &partial);
    }
}
