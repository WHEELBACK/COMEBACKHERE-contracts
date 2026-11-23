#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env, Symbol,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    Unauthorized = 3,
    InvalidAmount = 4,
    InvalidState = 5,
    InsufficientBalance = 6,
    Reentrancy = 7,
    InvalidRecipient = 8,
    SameAddress = 9,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VaultState {
    Uninitialized,
    Active,
    Locked,
    Released,
    Refunded,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Token,
    Depositor,
    Beneficiary,
    Amount,
    State,
    Locked,
}

const MAX_AMOUNT: i128 = i128::MAX / 2;

#[contract]
pub struct EscrowVault;

#[contractimpl]
impl EscrowVault {
    /// Initialize the escrow vault. Can only be called once.
    pub fn initialize(
        env: Env,
        admin: Address,
        token: Address,
        depositor: Address,
        beneficiary: Address,
        amount: i128,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::State) {
            return Err(Error::AlreadyInitialized);
        }
        if amount <= 0 || amount > MAX_AMOUNT {
            return Err(Error::InvalidAmount);
        }
        if depositor == beneficiary {
            return Err(Error::SameAddress);
        }

        // Admin must authorize initialization.
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Depositor, &depositor);
        env.storage()
            .instance()
            .set(&DataKey::Beneficiary, &beneficiary);
        env.storage().instance().set(&DataKey::Amount, &amount);
        env.storage()
            .instance()
            .set(&DataKey::State, &VaultState::Active);
        env.storage().instance().set(&DataKey::Locked, &false);

        Ok(())
    }

    /// Deposit funds into the vault. Only the depositor may fund.
    pub fn deposit(env: Env) -> Result<(), Error> {
        let state = Self::read_state(&env)?;
        if state != VaultState::Active {
            return Err(Error::InvalidState);
        }
        Self::guard_reentrancy(&env)?;

        let depositor: Address = env
            .storage()
            .instance()
            .get(&DataKey::Depositor)
            .ok_or(Error::NotInitialized)?;
        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)?;
        let amount: i128 = env
            .storage()
            .instance()
            .get(&DataKey::Amount)
            .ok_or(Error::NotInitialized)?;

        depositor.require_auth();

        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&depositor, &env.current_contract_address(), &amount);

        env.storage().instance().set(&DataKey::Locked, &false);
        Ok(())
    }

    /// Release funds to the beneficiary. Only admin may release.
    pub fn release(env: Env) -> Result<(), Error> {
        let state = Self::read_state(&env)?;
        if state != VaultState::Active {
            return Err(Error::InvalidState);
        }
        Self::guard_reentrancy(&env)?;

        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let beneficiary: Address = env
            .storage()
            .instance()
            .get(&DataKey::Beneficiary)
            .ok_or(Error::NotInitialized)?;
        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)?;
        let amount: i128 = env
            .storage()
            .instance()
            .get(&DataKey::Amount)
            .ok_or(Error::NotInitialized)?;

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        // Mark released before external call to prevent reentrancy.
        env.storage()
            .instance()
            .set(&DataKey::State, &VaultState::Released);

        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&env.current_contract_address(), &beneficiary, &amount);

        env.storage().instance().set(&DataKey::Locked, &false);
        Ok(())
    }

    /// Refund funds to the depositor. Only admin may refund.
    pub fn refund(env: Env) -> Result<(), Error> {
        let state = Self::read_state(&env)?;
        if state != VaultState::Active {
            return Err(Error::InvalidState);
        }
        Self::guard_reentrancy(&env)?;

        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let depositor: Address = env
            .storage()
            .instance()
            .get(&DataKey::Depositor)
            .ok_or(Error::NotInitialized)?;
        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::Token)
            .ok_or(Error::NotInitialized)?;
        let amount: i128 = env
            .storage()
            .instance()
            .get(&DataKey::Amount)
            .ok_or(Error::NotInitialized)?;

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        env.storage()
            .instance()
            .set(&DataKey::State, &VaultState::Refunded);

        let token_client = token::Client::new(&env, &token_addr);
        token_client.transfer(&env.current_contract_address(), &depositor, &amount);

        env.storage().instance().set(&DataKey::Locked, &false);
        Ok(())
    }

    /// Read-only view of the current vault state.
    pub fn get_state(env: Env) -> Result<VaultState, Error> {
        Self::read_state(&env)
    }

    /// Read-only view of the configured amount.
    pub fn get_amount(env: Env) -> Result<i128, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Amount)
            .ok_or(Error::NotInitialized)
    }

    /// Read-only view of the beneficiary.
    pub fn get_beneficiary(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Beneficiary)
            .ok_or(Error::NotInitialized)
    }

    fn read_state(env: &Env) -> Result<VaultState, Error> {
        env.storage()
            .instance()
            .get(&DataKey::State)
            .ok_or(Error::NotInitialized)
    }

    fn guard_reentrancy(env: &Env) -> Result<(), Error> {
        let locked: bool = env
            .storage()
            .instance()
            .get(&DataKey::Locked)
            .unwrap_or(false);
        if locked {
            return Err(Error::Reentrancy);
        }
        env.storage().instance().set(&DataKey::Locked, &true);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, MockAuth, MockAuthInvoke},
        token::{Client as TokenClient, StellarAssetClient},
        Address, Env, IntoVal,
    };

    fn setup() -> (
        Env,
        EscrowVaultClient<'static>,
        Address,
        Address,
        Address,
        Address,
        Address,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let depositor = Address::generate(&env);
        let beneficiary = Address::generate(&env);

        let token_admin = Address::generate(&env);
        let sac = env.register_stellar_asset_contract_v2(token_admin.clone());
        let token_addr = sac.address();
        let asset_client = StellarAssetClient::new(&env, &token_addr);
        asset_client.mint(&depositor, &1_000_000);

        let contract_id = env.register_contract(None, EscrowVault);
        let client = EscrowVaultClient::new(&env, &contract_id);

        (env, client, admin, token_addr, depositor, beneficiary, contract_id)
    }

    #[test]
    fn test_initialize_ok() {
        let (env, client, admin, token, depositor, beneficiary, _) = setup();
        let _ = env;
        client.initialize(&admin, &token, &depositor, &beneficiary, &1000);
        assert_eq!(client.get_state(), VaultState::Active);
        assert_eq!(client.get_amount(), 1000);
    }

    #[test]
    fn test_initialize_twice_fails() {
        let (_env, client, admin, token, depositor, beneficiary, _) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &1000);
        let res = client.try_initialize(&admin, &token, &depositor, &beneficiary, &1000);
        assert_eq!(res, Err(Ok(Error::AlreadyInitialized)));
    }

    #[test]
    fn test_initialize_invalid_amount() {
        let (_env, client, admin, token, depositor, beneficiary, _) = setup();
        let res = client.try_initialize(&admin, &token, &depositor, &beneficiary, &0);
        assert_eq!(res, Err(Ok(Error::InvalidAmount)));
        let res = client.try_initialize(&admin, &token, &depositor, &beneficiary, &-5);
        assert_eq!(res, Err(Ok(Error::InvalidAmount)));
    }

    #[test]
    fn test_initialize_same_address() {
        let (_env, client, admin, token, depositor, _, _) = setup();
        let res = client.try_initialize(&admin, &token, &depositor, &depositor, &1000);
        assert_eq!(res, Err(Ok(Error::SameAddress)));
    }

    #[test]
    fn test_initialize_requires_admin_auth() {
        let (env, client, admin, token, depositor, beneficiary, _) = setup();
        env.set_auths(&[]);
        let res = client.try_initialize(&admin, &token, &depositor, &beneficiary, &1000);
        assert!(res.is_err());
    }

    #[test]
    fn test_deposit_and_release() {
        let (env, client, admin, token, depositor, beneficiary, contract_id) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &1000);
        client.deposit();

        let token_client = TokenClient::new(&env, &token);
        assert_eq!(token_client.balance(&contract_id), 1000);

        client.release();
        assert_eq!(client.get_state(), VaultState::Released);
        assert_eq!(token_client.balance(&beneficiary), 1000);
        assert_eq!(token_client.balance(&contract_id), 0);
    }

    #[test]
    fn test_deposit_requires_depositor_auth() {
        let (env, client, admin, token, depositor, beneficiary, _) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &1000);
        env.set_auths(&[]);
        let res = client.try_deposit();
        assert!(res.is_err());
    }

    #[test]
    fn test_release_requires_admin_auth() {
        let (env, client, admin, token, depositor, beneficiary, _) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &1000);
        client.deposit();
        env.set_auths(&[]);
        let res = client.try_release();
        assert!(res.is_err());
    }

    #[test]
    fn test_release_twice_fails() {
        let (_env, client, admin, token, depositor, beneficiary, _) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &1000);
        client.deposit();
        client.release();
        let res = client.try_release();
        assert_eq!(res, Err(Ok(Error::InvalidState)));
    }

    #[test]
    fn test_refund_flow() {
        let (env, client, admin, token, depositor, beneficiary, contract_id) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &1000);
        client.deposit();
        client.refund();
        assert_eq!(client.get_state(), VaultState::Refunded);
        let token_client = TokenClient::new(&env, &token);
        assert_eq!(token_client.balance(&contract_id), 0);
    }

    #[test]
    fn test_refund_after_release_fails() {
        let (_env, client, admin, token, depositor, beneficiary, _) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &1000);
        client.deposit();
        client.release();
        let res = client.try_refund();
        assert_eq!(res, Err(Ok(Error::InvalidState)));
    }

    #[test]
    fn test_uninitialized_calls_fail() {
        let (_env, client, _, _, _, _, _) = setup();
        assert_eq!(client.try_deposit(), Err(Ok(Error::NotInitialized)));
        assert_eq!(client.try_release(), Err(Ok(Error::NotInitialized)));
        assert_eq!(client.try_refund(), Err(Ok(Error::NotInitialized)));
        assert_eq!(client.try_get_state(), Err(Ok(Error::NotInitialized)));
    }

    // ---- Fuzz-style tests for malicious caller scenarios ----

    #[test]
    fn fuzz_initialize_amounts() {
        let (_env, client, admin, token, depositor, beneficiary, _) = setup();
        let cases: [i128; 8] = [
            0,
            -1,
            i128::MIN,
            i128::MAX,
            MAX_AMOUNT + 1,
            MAX_AMOUNT,
            1,
            42,
        ];
        for amt in cases {
            let res = client.try_initialize(&admin, &token, &depositor, &beneficiary, &amt);
            if amt <= 0 || amt > MAX_AMOUNT {
                assert_eq!(res, Err(Ok(Error::InvalidAmount)));
            } else {
                assert!(res.is_ok());
                // reset by re-registering is not possible; break after success
                break;
            }
        }
    }

    #[test]
    fn fuzz_unauthorized_callers() {
        let (env, client, admin, token, depositor, beneficiary, _) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &1000);
        client.deposit();

        // Spoofed auth: no auths at all.
        env.set_auths(&[]);
        assert!(client.try_release().is_err());
        assert!(client.try_refund().is_err());

        // Auth for wrong address.
        let attacker = Address::generate(&env);
        env.mock_auths(&[MockAuth {
            address: &attacker,
            invoke: &MockAuthInvoke {
                contract: &client.address,
                fn_name: "release",
                args: ().into_val(&env),
                sub_invokes: &[],
            },
        }]);
        assert!(client.try_release().is_err());
    }

    #[test]
    fn fuzz_deposit_without_balance() {
        let (env, client, admin, token, depositor, beneficiary, _) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &1_000_000_000);
        // depositor has only 1_000_000 minted
        let res = client.try_deposit();
        assert!(res.is_err());
        let _ = env;
    }

    #[test]
    fn fuzz_state_transitions() {
        let (_env, client, admin, token, depositor, beneficiary, _) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &1000);
        client.deposit();
        client.release();
        // After release, deposit/refund/release must all fail.
        assert_eq!(client.try_deposit(), Err(Ok(Error::InvalidState)));
        assert_eq!(client.try_refund(), Err(Ok(Error::InvalidState)));
        assert_eq!(client.try_release(), Err(Ok(Error::InvalidState)));
    }

    #[test]
    fn fuzz_reentrancy_guard() {
        let (env, client, admin, token, depositor, beneficiary, contract_id) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &1000);
        client.deposit();

        // Simulate a stuck lock: set Locked=true directly via storage.
        env.as_contract(&contract_id, || {
            env.storage().instance().set(&DataKey::Locked, &true);
        });
        assert_eq!(client.try_release(), Err(Ok(Error::Reentrancy)));
        assert_eq!(client.try_refund(), Err(Ok(Error::Reentrancy)));
    }

    #[test]
    fn fuzz_getters_after_init() {
        let (_env, client, admin, token, depositor, beneficiary, _) = setup();
        client.initialize(&admin, &token, &depositor, &beneficiary, &777);
        assert_eq!(client.get_amount(), 777);
        assert_eq!(client.get_beneficiary(), beneficiary);
        assert_eq!(client.get_state(), VaultState::Active);
    }
}