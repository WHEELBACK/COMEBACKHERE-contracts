use crate::{require_admin, require_not_paused, DataKey, TreasuryContract, TreasuryError};
#[allow(unused_imports)]
use crate::{TreasuryContractArgs, TreasuryContractClient};
use soroban_sdk::{contractimpl, token, Address, Env, Symbol, Vec};

#[contractimpl]
impl TreasuryContract {
    /// Deposits `amount` tokens from `from` into the treasury via `token_contract`.
    /// Panics: `ContractPaused`, `InvalidAmount`.
    /// Emits: `deposit`.
    pub fn deposit(env: Env, from: Address, token_contract: Address, amount: i128) {
        require_not_paused(&env);
        from.require_auth();
        deposit_one(&env, &from, &token_contract, amount);
    }

    /// Deposits multiple `(token_contract, amount)` pairs from `from` into the treasury.
    /// Panics: `ContractPaused`, `InvalidAmount`.
    /// Emits: `deposit` for each deposited token.
    pub fn batch_deposit(env: Env, from: Address, deposits: Vec<(Address, i128)>) {
        require_not_paused(&env);
        from.require_auth();
        for (token_contract, amount) in deposits.iter() {
            deposit_one(&env, &from, &token_contract, amount);
        }
    }

    /// Withdraws `amount` tokens from the treasury to `to` via `token_contract`.
    /// Panics: `ContractPaused`, `InvalidAmount`, `InsufficientBalance`, `DestinationNotAllowed`.
    /// Emits: `withdraw`.
    pub fn withdraw(env: Env, to: Address, token_contract: Address, amount: i128) {
        require_not_paused(&env);
        to.require_auth();
        if amount <= 0 {
            soroban_sdk::panic_with_error!(env, TreasuryError::InvalidAmount);
        }
        // Check withdrawal destination allowlist: if non-empty, `to` must be present.
        let allowlist: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalAllowlist)
            .unwrap_or_else(|| Vec::new(&env));
        if !allowlist.is_empty() && !allowlist.contains(&to) {
            soroban_sdk::panic_with_error!(env, TreasuryError::DestinationNotAllowed);
        }
        let mut balance: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(to.clone()))
            .unwrap_or(0);
        if balance < amount {
            soroban_sdk::panic_with_error!(env, TreasuryError::InsufficientBalance);
        }
        enforce_withdrawal_limit(&env, &to, amount);
        balance = balance.checked_sub(amount).unwrap_or_else(|| {
            soroban_sdk::panic_with_error!(env, TreasuryError::ArithmeticOverflow)
        });
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &balance);
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        token_client.transfer(&treasury, &to, &amount);
        env.events()
            .publish((Symbol::new(&env, "withdraw"), to), amount);
    }

    /// Returns the recorded deposit balance for `address`, or 0 if never deposited.
    /// Read-only, no authentication required.
    pub fn get_balance(env: Env, address: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(address))
            .unwrap_or(0)
    }

    /// Drains the full token balance of the treasury to `recipient` (admin-only, paused-only emergency drain).
    /// Panics: `Unauthorized`, `NotPaused`.
    /// Emits: `treasury_drained`.
    pub fn withdraw_all(env: Env, admin: Address, token_contract: Address, recipient: Address) {
        require_admin(&env, &admin);
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if !paused {
            soroban_sdk::panic_with_error!(env, TreasuryError::NotPaused);
        }
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        let balance = token_client.balance(&treasury);
        if balance > 0 {
            enforce_withdrawal_limit(&env, &recipient, balance);
            token_client.transfer(&treasury, &recipient, &balance);
        }
        env.events()
            .publish((Symbol::new(&env, "treasury_drained"),), recipient);
    }
}

/// Enforces the admin-configured withdrawal-per-window cap (see
/// `TreasuryContract::set_withdrawal_limit`) against `amount` for `tracked`, rolling the
/// window over once it has elapsed. A `limit <= 0` (the default) leaves withdrawals uncapped.
/// Panics: `WithdrawalLimitExceeded`, `ArithmeticOverflow`.
fn enforce_withdrawal_limit(env: &Env, tracked: &Address, amount: i128) {
    let limit: i128 = env
        .storage()
        .instance()
        .get(&DataKey::WithdrawalLimitPerWindow)
        .unwrap_or(0);
    if limit <= 0 {
        return;
    }
    let window_secs: u64 = env
        .storage()
        .instance()
        .get(&DataKey::WithdrawalWindowSecs)
        .unwrap_or(0);
    let now = env.ledger().timestamp();
    let window_start: u64 = env
        .storage()
        .persistent()
        .get(&DataKey::WithdrawalWindowStart(tracked.clone()))
        .unwrap_or(0);
    let window_elapsed = window_secs == 0 || now >= window_start.saturating_add(window_secs);
    let withdrawn_so_far: i128 = if window_elapsed {
        env.storage()
            .persistent()
            .set(&DataKey::WithdrawalWindowStart(tracked.clone()), &now);
        0
    } else {
        env.storage()
            .persistent()
            .get(&DataKey::WithdrawnInWindow(tracked.clone()))
            .unwrap_or(0)
    };
    let new_total = withdrawn_so_far.checked_add(amount).unwrap_or_else(|| {
        soroban_sdk::panic_with_error!(env, TreasuryError::ArithmeticOverflow)
    });
    if new_total > limit {
        soroban_sdk::panic_with_error!(env, TreasuryError::WithdrawalLimitExceeded);
    }
    env.storage()
        .persistent()
        .set(&DataKey::WithdrawnInWindow(tracked.clone()), &new_total);
}

fn deposit_one(env: &Env, from: &Address, token_contract: &Address, amount: i128) {
    if amount <= 0 {
        soroban_sdk::panic_with_error!(env, TreasuryError::InvalidAmount);
    }
    let treasury = env.current_contract_address();
    let token_client = token::Client::new(env, token_contract);
    token_client.transfer(from, &treasury, &amount);
    let mut balance: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::Balance(from.clone()))
        .unwrap_or(0);
    balance = balance
        .checked_add(amount)
        .unwrap_or_else(|| soroban_sdk::panic_with_error!(env, TreasuryError::ArithmeticOverflow));
    env.storage()
        .persistent()
        .set(&DataKey::Balance(from.clone()), &balance);
    env.events()
        .publish((Symbol::new(env, "deposit"), from.clone()), amount);
}
