use crate::{require_admin, require_not_paused, DataKey, TreasuryContract};
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
            panic!("InvalidAmount");
        }
        // Check withdrawal destination allowlist: if non-empty, `to` must be present.
        let allowlist: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalAllowlist)
            .unwrap_or_else(|| Vec::new(&env));
        if !allowlist.is_empty() && !allowlist.contains(&to) {
            panic!("DestinationNotAllowed");
        }
        let mut balance: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(to.clone()))
            .unwrap_or(0);
        if balance < amount {
            panic!("InsufficientBalance");
        }
        balance -= amount;
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone()), &balance);
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        token_client.transfer(&treasury, &to, &amount);
        env.events()
            .publish((Symbol::new(&env, "withdraw"), to), amount);
    }

    /// Adds `address` to the withdrawal destination allowlist (admin-only).
    /// Emits: `withdrawal_address_added`.
    pub fn add_withdrawal_address(env: Env, admin: Address, address: Address) {
        require_admin(&env, &admin);
        let mut allowlist: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalAllowlist)
            .unwrap_or_else(|| Vec::new(&env));
        if !allowlist.contains(&address) {
            allowlist.push_back(address.clone());
            env.storage()
                .instance()
                .set(&DataKey::WithdrawalAllowlist, &allowlist);
            env.events()
                .publish((Symbol::new(&env, "withdrawal_address_added"),), address);
        }
    }

    /// Removes `address` from the withdrawal destination allowlist (admin-only).
    /// Emits: `withdrawal_address_removed`.
    pub fn remove_withdrawal_address(env: Env, admin: Address, address: Address) {
        require_admin(&env, &admin);
        let allowlist: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalAllowlist)
            .unwrap_or_else(|| Vec::new(&env));
        let mut updated = Vec::new(&env);
        for a in allowlist.iter() {
            if a != address {
                updated.push_back(a);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::WithdrawalAllowlist, &updated);
        env.events()
            .publish((Symbol::new(&env, "withdrawal_address_removed"),), address);
    }

    /// Returns the current list of allowed withdrawal destination addresses.
    pub fn get_withdrawal_allowlist(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::WithdrawalAllowlist)
            .unwrap_or_else(|| Vec::new(&env))
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
            panic!("NotPaused");
        }
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        let balance = token_client.balance(&treasury);
        if balance > 0 {
            token_client.transfer(&treasury, &recipient, &balance);
        }
        env.events()
            .publish((Symbol::new(&env, "treasury_drained"),), recipient);
    }
}

fn deposit_one(env: &Env, from: &Address, token_contract: &Address, amount: i128) {
    if amount <= 0 {
        panic!("InvalidAmount");
    }
    let treasury = env.current_contract_address();
    let token_client = token::Client::new(env, token_contract);
    token_client.transfer(from, &treasury, &amount);
    let mut balance: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::Balance(from.clone()))
        .unwrap_or(0);
    balance += amount;
    env.storage()
        .persistent()
        .set(&DataKey::Balance(from.clone()), &balance);
    env.events()
        .publish((Symbol::new(env, "deposit"), from.clone()), amount);
}
