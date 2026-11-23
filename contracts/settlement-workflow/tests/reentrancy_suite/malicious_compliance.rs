//! Malicious compliance test double for the settlement-workflow reentrancy
//! suite (see `settlement_workflow_test.rs`'s `reentrant_compliance_*` tests).
//!
//! `SettlementWorkflowContract::execute_with_compliance` performs a two-hop
//! call sequence: `Compliance::is_allowed(merchant)` first, and only if that
//! passes, `Treasury::execute_settlement(...)` second. Issue #118's
//! reentrancy suite (`contracts/treasury/tests/reentrancy_suite/`) only
//! exercises the token-transfer callback inside `execute_settlement` itself
//! — it says nothing about what happens if the *first* hop in this
//! contract's own orchestration (the compliance check) is the one that
//! turns out to be malicious or compromised.
//!
//! `MaliciousCompliance` models exactly that: its `is_allowed` — the same
//! entrypoint name and signature `compliance_client::ComplianceInterface`
//! expects, so it's a drop-in replacement for the real compliance contract's
//! address — attempts to call `Treasury::execute_settlement` directly,
//! *before* returning its allowed/denied verdict. That reentrant call races
//! `execute_with_compliance`'s own, legitimate `execute_settlement` call,
//! which only happens afterwards, once `is_allowed` has returned.
//!
//! This mirrors the `ReentrancyToken` pattern in
//! `contracts/treasury/tests/reentrancy_suite/malicious_token.rs`, adapted
//! for the compliance leg of settlement-workflow's call chain instead of the
//! token-transfer leg of treasury's.

use soroban_sdk::{contract, contractimpl, Address, Env};
use treasury::TreasuryContractClient;

#[contract]
pub struct MaliciousCompliance;

#[contractimpl]
impl MaliciousCompliance {
    /// Arms the reentrant callback and records everything `is_allowed` needs
    /// to call `Treasury::execute_settlement` on its own. Must be called
    /// before triggering `execute_with_compliance` in a test.
    ///
    /// `signer` is the address `execute_settlement` should be called with —
    /// in these tests, the settlement-workflow contract's own address, which
    /// is already registered as a treasury signer via `set_signer` the same
    /// way it is for the legitimate call.
    pub fn set_reentry_target(
        env: Env,
        treasury: Address,
        signer: Address,
        settlement_id: u64,
        token_contract: Address,
    ) {
        env.storage().instance().set(&("mc_treasury",), &treasury);
        env.storage().instance().set(&("mc_signer",), &signer);
        env.storage().instance().set(&("mc_sid",), &settlement_id);
        env.storage()
            .instance()
            .set(&("mc_token",), &token_contract);
        env.storage().instance().set(&("mc_armed",), &true);
    }

    /// Sets the allowed/denied verdict `is_allowed` returns after attempting
    /// its reentrant callback. Defaults to `true` (allowed) if never called.
    pub fn set_verdict(env: Env, allowed: bool) {
        env.storage().instance().set(&("mc_verdict",), &allowed);
    }

    /// Matches `compliance_client::ComplianceInterface::is_allowed`'s wire
    /// signature exactly, so this contract's address can be passed wherever
    /// a real compliance contract's address is expected.
    pub fn is_allowed(env: Env, _address: Address) -> bool {
        let armed: bool = env
            .storage()
            .instance()
            .get(&("mc_armed",))
            .unwrap_or(false);
        if armed {
            // Disarm before recursing so a future change that makes
            // execute_settlement call back into compliance can't turn this
            // into unbounded recursion.
            env.storage().instance().set(&("mc_armed",), &false);
            let treasury: Address = env.storage().instance().get(&("mc_treasury",)).unwrap();
            let signer: Address = env.storage().instance().get(&("mc_signer",)).unwrap();
            let sid: u64 = env.storage().instance().get(&("mc_sid",)).unwrap();
            let token: Address = env.storage().instance().get(&("mc_token",)).unwrap();
            let client = TreasuryContractClient::new(&env, &treasury);
            // Attempt to execute the settlement ourselves, from inside the
            // compliance check — before execute_with_compliance's own,
            // legitimate `treasury.execute_settlement` call ever runs.
            client.execute_settlement(&signer, &sid, &token);
        }
        env.storage()
            .instance()
            .get(&("mc_verdict",))
            .unwrap_or(true)
    }
}
