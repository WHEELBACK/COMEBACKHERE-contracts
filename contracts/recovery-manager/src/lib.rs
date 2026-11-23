#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum RecoveryError {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
    InvalidAmount = 4,
    InsufficientBalance = 5,
    ArithmeticUnderflow = 6,
    ArithmeticOverflow = 7,
    InvalidSettlement = 8,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Admin,
    Balance(Address),
    TotalRecovered,
}

#[contract]
pub struct RecoveryManager;

#[contractimpl]
impl RecoveryManager {
    /// Initialize the contract with an admin.
    pub fn initialize(env: Env, admin: Address) -> Result<(), RecoveryError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(RecoveryError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TotalRecovered, &0i128);
        Ok(())
    }

    /// Deposit funds for an account.
    pub fn deposit(env: Env, from: Address, amount: i128) -> Result<i128, RecoveryError> {
        from.require_auth();
        if amount <= 0 {
            return Err(RecoveryError::InvalidAmount);
        }
        let key = DataKey::Balance(from.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let updated = current
            .checked_add(amount)
            .ok_or(RecoveryError::ArithmeticOverflow)?;
        env.storage().persistent().set(&key, &updated);
        Ok(updated)
    }

    /// Get the balance of an account.
    pub fn balance(env: Env, account: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(account))
            .unwrap_or(0)
    }

    /// Get the total amount recovered across all settlements.
    pub fn total_recovered(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalRecovered)
            .unwrap_or(0)
    }

    /// Perform a partial settlement: recover `amount` from `account`'s balance.
    ///
    /// Uses checked arithmetic throughout so that any underflow/overflow is
    /// surfaced as a typed error rather than panicking or wrapping.
    pub fn partial_settle(
        env: Env,
        account: Address,
        amount: i128,
    ) -> Result<i128, RecoveryError> {
        account.require_auth();

        // Validate inputs before any computation.
        if amount <= 0 {
            return Err(RecoveryError::InvalidAmount);
        }

        let key = DataKey::Balance(account.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);

        // Defensive: ensure the requested settlement does not exceed balance.
        if amount > current {
            return Err(RecoveryError::InsufficientBalance);
        }

        // Use checked_sub to prevent arithmetic underflow.
        let remaining = current
            .checked_sub(amount)
            .ok_or(RecoveryError::ArithmeticUnderflow)?;

        // Update total recovered using checked_add.
        let total: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalRecovered)
            .unwrap_or(0);
        let new_total = total
            .checked_add(amount)
            .ok_or(RecoveryError::ArithmeticOverflow)?;

        env.storage().persistent().set(&key, &remaining);
        env.storage()
            .instance()
            .set(&DataKey::TotalRecovered, &new_total);

        Ok(remaining)
    }

    /// Compute the remaining balance after a partial settlement without
    /// mutating state. Useful for previews and off-chain callers.
    pub fn preview_partial_settle(
        env: Env,
        account: Address,
        amount: i128,
    ) -> Result<i128, RecoveryError> {
        if amount <= 0 {
            return Err(RecoveryError::InvalidAmount);
        }
        let current: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(account))
            .unwrap_or(0);
        if amount > current {
            return Err(RecoveryError::InsufficientBalance);
        }
        current
            .checked_sub(amount)
            .ok_or(RecoveryError::ArithmeticUnderflow)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Env;

    fn setup() -> (Env, RecoveryManagerClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, RecoveryManager);
        let client = RecoveryManagerClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin)
    }

    #[test]
    fn test_deposit_and_balance() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        let bal = client.deposit(&user, &1000);
        assert_eq!(bal, 1000);
        assert_eq!(client.balance(&user), 1000);
    }

    #[test]
    fn test_deposit_invalid_amount() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        let res = client.try_deposit(&user, &0);
        assert_eq!(res, Err(Ok(RecoveryError::InvalidAmount)));
        let res = client.try_deposit(&user, &-5);
        assert_eq!(res, Err(Ok(RecoveryError::InvalidAmount)));
    }

    #[test]
    fn test_partial_settle_success() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        client.deposit(&user, &1000);
        let remaining = client.partial_settle(&user, &400);
        assert_eq!(remaining, 600);
        assert_eq!(client.balance(&user), 600);
        assert_eq!(client.total_recovered(), 400);
    }

    #[test]
    fn test_partial_settle_full() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        client.deposit(&user, &500);
        let remaining = client.partial_settle(&user, &500);
        assert_eq!(remaining, 0);
        assert_eq!(client.balance(&user), 0);
        assert_eq!(client.total_recovered(), 500);
    }

    #[test]
    fn test_partial_settle_insufficient_balance() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        client.deposit(&user, &100);
        let res = client.try_partial_settle(&user, &200);
        assert_eq!(res, Err(Ok(RecoveryError::InsufficientBalance)));
        // State unchanged.
        assert_eq!(client.balance(&user), 100);
        assert_eq!(client.total_recovered(), 0);
    }

    #[test]
    fn test_partial_settle_invalid_amount() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        client.deposit(&user, &100);
        let res = client.try_partial_settle(&user, &0);
        assert_eq!(res, Err(Ok(RecoveryError::InvalidAmount)));
        let res = client.try_partial_settle(&user, &-1);
        assert_eq!(res, Err(Ok(RecoveryError::InvalidAmount)));
    }

    #[test]
    fn test_partial_settle_no_balance() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        let res = client.try_partial_settle(&user, &1);
        assert_eq!(res, Err(Ok(RecoveryError::InsufficientBalance)));
    }

    #[test]
    fn test_preview_partial_settle() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        client.deposit(&user, &1000);
        let preview = client.preview_partial_settle(&user, &300);
        assert_eq!(preview, 700);
        // State unchanged by preview.
        assert_eq!(client.balance(&user), 1000);
    }

    #[test]
    fn test_preview_insufficient() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        client.deposit(&user, &50);
        let res = client.try_preview_partial_settle(&user, &100);
        assert_eq!(res, Err(Ok(RecoveryError::InsufficientBalance)));
    }

    #[test]
    fn test_deposit_overflow() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        client.deposit(&user, &i128::MAX);
        let res = client.try_deposit(&user, &1);
        assert_eq!(res, Err(Ok(RecoveryError::ArithmeticOverflow)));
    }

    #[test]
    fn test_total_recovered_overflow() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        // Set up a balance large enough to attempt overflow on total.
        client.deposit(&user, &i128::MAX);
        // First settle a large amount.
        client.partial_settle(&user, &(i128::MAX - 1));
        // Deposit more and try to settle again to overflow total.
        client.deposit(&user, &100);
        let res = client.try_partial_settle(&user, &100);
        assert_eq!(res, Err(Ok(RecoveryError::ArithmeticOverflow)));
    }

    #[test]
    fn test_initialize_twice_fails() {
        let (env, client, admin) = setup();
        let res = client.try_initialize(&admin);
        assert_eq!(res, Err(Ok(RecoveryError::AlreadyInitialized)));
        let _ = env;
    }

    #[test]
    fn test_fuzz_partial_settle_never_underflows() {
        let (env, client, _admin) = setup();
        let user = Address::generate(&env);
        client.deposit(&user, &10_000);

        // Deterministic pseudo-fuzz sweep over a range of amounts.
        let mut amount: i128 = -50;
        while amount <= 10_050 {
            let res = client.try_partial_settle(&user, &amount);
            if amount <= 0 {
                assert_eq!(res, Err(Ok(RecoveryError::InvalidAmount)));
            } else if amount > client.balance(&user) {
                assert_eq!(res, Err(Ok(RecoveryError::InsufficientBalance)));
            } else {
                assert!(res.is_ok());
            }
            amount += 7;
        }
    }
}