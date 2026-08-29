use crate::{require_admin, require_not_paused, DataKey, TreasuryContract, TreasuryError};
#[allow(unused_imports)]
use crate::{TreasuryContractArgs, TreasuryContractClient};
use soroban_sdk::{contractimpl, token, Address, Env, Symbol, Vec};

#[contractimpl]
impl TreasuryContract {
    /// Deposits `amount` tokens from `from` into the treasury via `token_contract`.
    /// Errors: `ContractPaused`, `InvalidAmount`.
    /// Emits: `deposit`.
    pub fn deposit(
        env: Env,
        from: Address,
        token_contract: Address,
        amount: i128,
    ) -> Result<(), TreasuryError> {
        require_not_paused(&env);
        from.require_auth();
        deposit_one(&env, &from, &token_contract, amount)
    }

    /// Deposits multiple `(token_contract, amount)` pairs from `from` into the treasury.
    /// Errors: `ContractPaused`, `InvalidAmount`.
    /// Emits: `deposit` for each deposited token.
    pub fn batch_deposit(
        env: Env,
        from: Address,
        deposits: Vec<(Address, i128)>,
    ) -> Result<(), TreasuryError> {
        require_not_paused(&env);
        from.require_auth();
        for (token_contract, amount) in deposits.iter() {
            deposit_one(&env, &from, &token_contract, amount)?;
        }
        Ok(())
    }

    /// Withdraws `amount` tokens from the treasury to `to` via `token_contract`.
    /// Errors: `ContractPaused`, `InvalidAmount`, `InsufficientBalance`, `DestinationNotAllowed`.
    /// Emits: `withdraw`.
    pub fn withdraw(
        env: Env,
        to: Address,
        token_contract: Address,
        amount: i128,
    ) -> Result<(), TreasuryError> {
        require_not_paused(&env);
        to.require_auth();
        if amount <= 0 {
            return Err(TreasuryError::InvalidAmount);
        }
        // Check withdrawal destination allowlist: if non-empty, `to` must be present.
        let allowlist: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::WithdrawalAllowlist)
            .unwrap_or_else(|| Vec::new(&env));
        if !allowlist.is_empty() && !allowlist.contains(&to) {
            return Err(TreasuryError::DestinationNotAllowed);
        }
        let mut balance: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(to.clone(), token_contract.clone()))
            .unwrap_or(0);
        if balance < amount {
            return Err(TreasuryError::InsufficientBalance);
        }
        balance = balance
            .checked_sub(amount)
            .ok_or(TreasuryError::ArithmeticOverflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::Balance(to.clone(), token_contract.clone()), &balance);
        let treasury = env.current_contract_address();
        let token_client = token::Client::new(&env, &token_contract);
        token_client.transfer(&treasury, &to, &amount);
        env.events()
            .publish((Symbol::new(&env, "withdraw"), to), amount);
        Ok(())
    }

    /// Returns the recorded deposit balance for `address` under `token_contract`, or 0 if never
    /// deposited. Balances are segregated per token contract (#448); this never mixes holdings
    /// across different allowlisted tokens.
    /// Read-only, no authentication required.
    pub fn get_balance(env: Env, address: Address, token_contract: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(address, token_contract))
            .unwrap_or(0)
    }

    /// Drains the full token balance of the treasury to `recipient` (admin-only, paused-only emergency drain).
    /// Errors: `NotPaused`.
    /// Panics: `Unauthorized`.
    /// Emits: `treasury_drained`.
    pub fn withdraw_all(
        env: Env,
        admin: Address,
        token_contract: Address,
        recipient: Address,
    ) -> Result<(), TreasuryError> {
        require_admin(&env, &admin);
        let paused: bool = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if !paused {
            return Err(TreasuryError::NotPaused);
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
        Ok(())
    }
}

fn deposit_one(env: &Env, from: &Address, token_contract: &Address, amount: i128) -> Result<(), TreasuryError> {
    if amount <= 0 {
        return Err(TreasuryError::InvalidAmount);
    }
    let treasury = env.current_contract_address();
    let token_client = token::Client::new(env, token_contract);
    token_client.transfer(from, &treasury, &amount);
    let mut balance: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::Balance(from.clone(), token_contract.clone()))
        .unwrap_or(0);
    balance = balance
        .checked_add(amount)
        .ok_or(TreasuryError::ArithmeticOverflow)?;
    env.storage()
        .persistent()
        .set(&DataKey::Balance(from.clone()), &balance);
    env.events()
        .publish((Symbol::new(env, "deposit"), from.clone()), amount);
    Ok(())
}
