#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec,
};

/// Persistent storage keys used by the event-emitter contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    /// Tracks the balance associated with an address.
    Balance(Address),
    /// Tracks the total number of events emitted.
    EventCount,
    /// Tracks whether an address is an authorized emitter.
    Emitter(Address),
    /// Stores the list of registered emitters.
    Emitters,
}

/// TTL threshold (in ledgers) below which persistent entries are bumped.
pub const TTL_THRESHOLD: u32 = 100;
/// TTL extension target (in ledgers) applied when bumping persistent entries.
pub const TTL_EXTEND_TO: u32 = 1_000;

/// Typed errors returned by the contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// A zero balance was encountered where a positive balance is required.
    ZeroBalance = 1,
    /// Arithmetic overflow or underflow occurred.
    ArithmeticError = 2,
    /// The requested address is not a registered emitter.
    NotAuthorized = 3,
    /// The balance is insufficient for the requested operation.
    InsufficientBalance = 4,
    /// The value could not be found in storage.
    NotFound = 5,
}

#[contract]
pub struct EventEmitterContract;

#[contractimpl]
impl EventEmitterContract {
    /// Bump the TTL of a persistent storage key to avoid ledger eviction.
    fn bump_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }

    /// Read a persistent balance, bumping its TTL on access.
    fn read_balance(env: &Env, addr: &Address) -> i128 {
        let key = DataKey::Balance(addr.clone());
        let balance: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        Self::bump_ttl(env, &key);
        balance
    }

    /// Write a persistent balance, bumping its TTL on access.
    fn write_balance(env: &Env, addr: &Address, balance: i128) {
        let key = DataKey::Balance(addr.clone());
        env.storage().persistent().set(&key, &balance);
        Self::bump_ttl(env, &key);
    }

    /// Register a new emitter address.
    pub fn register_emitter(env: Env, emitter: Address) -> Result<(), Error> {
        emitter.require_auth();

        let key = DataKey::Emitter(emitter.clone());
        if env.storage().persistent().has(&key) {
            Self::bump_ttl(&env, &key);
            return Ok(());
        }

        env.storage().persistent().set(&key, &true);
        Self::bump_ttl(&env, &key);

        let mut emitters: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Emitters)
            .unwrap_or_else(|| Vec::new(&env));
        emitters.push_back(emitter);
        env.storage().persistent().set(&DataKey::Emitters, &emitters);
        Self::bump_ttl(&env, &DataKey::Emitters);

        Ok(())
    }

    /// Deposit a positive amount into an address's balance.
    pub fn deposit(env: Env, addr: Address, amount: i128) -> Result<i128, Error> {
        addr.require_auth();

        if amount <= 0 {
            return Err(Error::ZeroBalance);
        }

        let current = Self::read_balance(&env, &addr);
        let updated = current.checked_add(amount).ok_or(Error::ArithmeticError)?;
        Self::write_balance(&env, &addr, updated);

        Ok(updated)
    }

    /// Withdraw a positive amount from an address's balance.
    pub fn withdraw(env: Env, addr: Address, amount: i128) -> Result<i128, Error> {
        addr.require_auth();

        if amount <= 0 {
            return Err(Error::ZeroBalance);
        }

        let current = Self::read_balance(&env, &addr);
        if current == 0 {
            return Err(Error::ZeroBalance);
        }

        let updated = current.checked_sub(amount).ok_or(Error::ArithmeticError)?;
        if updated < 0 {
            return Err(Error::InsufficientBalance);
        }

        Self::write_balance(&env, &addr, updated);

        Ok(updated)
    }

    /// Emit an event from an authorized emitter.
    pub fn emit(env: Env, emitter: Address, topic: Symbol, value: i128) -> Result<(), Error> {
        emitter.require_auth();

        let emitter_key = DataKey::Emitter(emitter.clone());
        let authorized: bool = env
            .storage()
            .persistent()
            .get(&emitter_key)
            .unwrap_or(false);
        Self::bump_ttl(&env, &emitter_key);

        if !authorized {
            return Err(Error::NotAuthorized);
        }

        let count_key = DataKey::EventCount;
        let count: u32 = env.storage().persistent().get(&count_key).unwrap_or(0);
        let new_count = count.checked_add(1).ok_or(Error::ArithmeticError)?;
        env.storage().persistent().set(&count_key, &new_count);
        Self::bump_ttl(&env, &count_key);

        env.events().publish((symbol_short!("emit"), topic), value);

        Ok(())
    }

    /// Return the current balance for an address.
    pub fn balance(env: Env, addr: Address) -> i128 {
        Self::read_balance(&env, &addr)
    }

    /// Return the total number of events emitted.
    pub fn event_count(env: Env) -> u32 {
        let key = DataKey::EventCount;
        let count: u32 = env.storage().persistent().get(&key).unwrap_or(0);
        Self::bump_ttl(&env, &key);
        count
    }

    /// Return whether an address is a registered emitter.
    pub fn is_emitter(env: Env, addr: Address) -> bool {
        let key = DataKey::Emitter(addr);
        let authorized: bool = env.storage().persistent().get(&key).unwrap_or(false);
        Self::bump_ttl(&env, &key);
        authorized
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Env;

    #[test]
    fn test_deposit_and_withdraw() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EventEmitterContract, ());
        let client = EventEmitterContractClient::new(&env, &contract_id);

        let user = Address::generate(&env);

        assert_eq!(client.deposit(&user, &100), 100);
        assert_eq!(client.balance(&user), 100);

        assert_eq!(client.withdraw(&user, &40), 60);
        assert_eq!(client.balance(&user), 60);
    }

    #[test]
    fn test_zero_balance_guards() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EventEmitterContract, ());
        let client = EventEmitterContractClient::new(&env, &contract_id);

        let user = Address::generate(&env);

        // Depositing zero returns a typed error instead of panicking.
        assert_eq!(client.try_deposit(&user, &0), Err(Ok(Error::ZeroBalance)));

        // Withdrawing from a zero balance returns a typed error.
        assert_eq!(
            client.try_withdraw(&user, &10),
            Err(Ok(Error::ZeroBalance))
        );

        // Withdrawing zero returns a typed error.
        assert_eq!(client.try_withdraw(&user, &0), Err(Ok(Error::ZeroBalance)));
    }

    #[test]
    fn test_insufficient_balance() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EventEmitterContract, ());
        let client = EventEmitterContractClient::new(&env, &contract_id);

        let user = Address::generate(&env);

        client.deposit(&user, &50);
        assert_eq!(
            client.try_withdraw(&user, &100),
            Err(Ok(Error::InsufficientBalance))
        );
    }

    #[test]
    fn test_overflow_guards() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EventEmitterContract, ());
        let client = EventEmitterContractClient::new(&env, &contract_id);

        let user = Address::generate(&env);

        client.deposit(&user, &i128::MAX);
        assert_eq!(
            client.try_deposit(&user, &1),
            Err(Ok(Error::ArithmeticError))
        );
    }

    #[test]
    fn test_emit_requires_authorization() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EventEmitterContract, ());
        let client = EventEmitterContractClient::new(&env, &contract_id);

        let emitter = Address::generate(&env);

        assert_eq!(
            client.try_emit(&emitter, &symbol_short!("test"), &1),
            Err(Ok(Error::NotAuthorized))
        );

        client.register_emitter(&emitter);
        assert!(client.is_emitter(&emitter));

        client.emit(&emitter, &symbol_short!("test"), &1);
        assert_eq!(client.event_count(), 1);
    }

    #[test]
    fn test_ttl_bump_on_access() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(EventEmitterContract, ());
        let client = EventEmitterContractClient::new(&env, &contract_id);

        let user = Address::generate(&env);
        client.deposit(&user, &10);

        // Accessing the balance should bump the TTL without panicking.
        assert_eq!(client.balance(&user), 10);
    }
}
```

```toml
[package]
name = "event-emitter"
version = "0.1.0"
edition = "2021"
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
soroban-sdk = { workspace = true }

[dev-dependencies]
soroban-sdk = { workspace = true, features = ["testutils"] }