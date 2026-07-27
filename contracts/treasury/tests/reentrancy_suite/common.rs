//! Small helpers shared by the reentrancy suite's per-entrypoint modules.

use soroban_sdk::{Address, Env, Vec};
use treasury::{TreasuryContract, TreasuryContractClient};

/// Initialise a treasury with `threshold` and mock all auths.
///
/// Returns `(client, admin, treasury_id)`. `treasury_id` is needed so tests
/// can register it into the reentrancy token.
pub fn init_treasury<'a>(
    env: &'a Env,
    threshold: u32,
) -> (TreasuryContractClient<'a>, Address, Address) {
    env.mock_all_auths();
    let admin = Address::generate(env);
    let treasury_id = env.register_contract(None, TreasuryContract);
    let client = TreasuryContractClient::new(env, &treasury_id);
    client.initialize(&admin, &threshold, &Vec::new(env));
    (client, admin, treasury_id)
}

/// Build the standard `add_allowed_token` allowlist containing a single
/// `token_id`, so `execute_settlement` / `partially_execute_settlement`
/// pass the "non-empty allowlist must contain token" check.
pub fn allow_token_only<'a>(
    env: &Env,
    client: &TreasuryContractClient<'a>,
    admin: &Address,
    token_id: &Address,
) {
    client.add_allowed_token(admin, token_id);
}
