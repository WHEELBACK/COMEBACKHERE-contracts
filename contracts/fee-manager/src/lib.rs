#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, token, Address, Env, IntoVal,
    Symbol, Vec,
};

/// Storage keys for the fee-manager contract.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Address of the admin that can configure the contract.
    Admin,
    /// Default fee in basis points applied when no specific rule exists.
    DefaultFeeBps,
    /// Per-asset fee configuration.
    AssetFee(Address),
    /// Per-(asset, payer) accumulated fees.
    Accrued(Address, Address),
    /// Treasury address where collected fees are sent.
    Treasury,
}

/// Errors returned by the fee-manager contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// Contract has not been initialized.
    NotInitialized = 1,
    /// Caller is not authorized for this operation.
    Unauthorized = 2,
    /// Fee basis points exceed the maximum allowed (10000 = 100%).
    InvalidFeeBps = 3,
    /// Amount must be strictly positive.
    InvalidAmount = 4,
    /// Arithmetic overflow occurred while computing fees.
    Overflow = 5,
    /// The provided asset address is invalid or unsupported.
    InvalidAsset = 6,
    /// Cross-contract call to the token failed.
    TokenCallFailed = 7,
}

/// Maximum fee in basis points (100%).
const MAX_FEE_BPS: u32 = 10_000;
/// Basis points denominator.
const BPS_DENOMINATOR: u32 = 10_000;

/// Event topics.
const TOPIC_INIT: Symbol = symbol_short!("init");
const TOPIC_FEE_SET: Symbol = symbol_short!("fee_set");
const TOPIC_COLLECTED: Symbol = symbol_short!("collected");
const TOPIC_WITHDRAWN: Symbol = symbol_short!("withdrawn");

#[contract]
pub struct FeeManager;

#[contractimpl]
impl FeeManager {
    /// Initialize the fee-manager with an admin, treasury, and default fee.
    pub fn initialize(
        env: Env,
        admin: Address,
        treasury: Address,
        default_fee_bps: u32,
    ) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::Unauthorized);
        }
        if default_fee_bps > MAX_FEE_BPS {
            return Err(Error::InvalidFeeBps);
        }
        admin.require_auth();

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        env.storage()
            .instance()
            .set(&DataKey::DefaultFeeBps, &default_fee_bps);

        env.events()
            .publish((TOPIC_INIT, admin.clone()), (treasury, default_fee_bps));
        Ok(())
    }

    /// Set the fee (in bps) for a specific asset. Admin only.
    pub fn set_asset_fee(env: Env, asset: Address, fee_bps: u32) -> Result<(), Error> {
        let admin = Self::admin(&env)?;
        admin.require_auth();
        if fee_bps > MAX_FEE_BPS {
            return Err(Error::InvalidFeeBps);
        }
        env.storage()
            .persistent()
            .set(&DataKey::AssetFee(asset.clone()), &fee_bps);
        env.events().publish((TOPIC_FEE_SET, asset), fee_bps);
        Ok(())
    }

    /// Update the default fee (in bps). Admin only.
    pub fn set_default_fee(env: Env, fee_bps: u32) -> Result<(), Error> {
        let admin = Self::admin(&env)?;
        admin.require_auth();
        if fee_bps > MAX_FEE_BPS {
            return Err(Error::InvalidFeeBps);
        }
        env.storage()
            .instance()
            .set(&DataKey::DefaultFeeBps, &fee_bps);
        Ok(())
    }

    /// Update the treasury address. Admin only.
    pub fn set_treasury(env: Env, treasury: Address) -> Result<(), Error> {
        let admin = Self::admin(&env)?;
        admin.require_auth();
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        Ok(())
    }

    /// Compute the fee for a given asset and amount without transferring.
    pub fn quote_fee(env: Env, asset: Address, amount: i128) -> Result<i128, Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        let fee_bps = Self::fee_bps_for(&env, &asset);
        Self::compute_fee(amount, fee_bps)
    }

    /// Collect a fee from `payer` for `asset` and `amount`.
    ///
    /// Performs a cross-contract invocation of the token contract to move
    /// `fee` from `payer` to the treasury, and records the accrued amount.
    pub fn collect_fee(
        env: Env,
        payer: Address,
        asset: Address,
        amount: i128,
    ) -> Result<i128, Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        payer.require_auth();

        let fee_bps = Self::fee_bps_for(&env, &asset);
        let fee = Self::compute_fee(amount, fee_bps)?;

        if fee > 0 {
            let treasury = Self::treasury(&env)?;
            let token_client = token::Client::new(&env, &asset);
            token_client.transfer(&payer, &treasury, &fee);

            let key = DataKey::Accrued(asset.clone(), payer.clone());
            let prev: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            let next = prev.checked_add(fee).ok_or(Error::Overflow)?;
            env.storage().persistent().set(&key, &next);

            env.events()
                .publish((TOPIC_COLLECTED, payer, asset), (fee, next));
        }

        Ok(fee)
    }

    /// Withdraw accrued fees for a (asset, payer) pair to the treasury.
    ///
    /// Performs a cross-contract invocation of the token contract to move
    /// the accrued amount from the contract to the treasury.
    pub fn withdraw_accrued(env: Env, asset: Address, payer: Address) -> Result<i128, Error> {
        let admin = Self::admin(&env)?;
        admin.require_auth();

        let key = DataKey::Accrued(asset.clone(), payer.clone());
        let accrued: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if accrued <= 0 {
            return Ok(0);
        }

        let treasury = Self::treasury(&env)?;
        let token_client = token::Client::new(&env, &asset);
        token_client.transfer(&env.current_contract_address(), &treasury, &accrued);

        env.storage().persistent().set(&key, &0i128);
        env.events()
            .publish((TOPIC_WITHDRAWN, payer, asset), accrued);
        Ok(accrued)
    }

    /// Read the accrued fee for a (asset, payer) pair.
    pub fn accrued(env: Env, asset: Address, payer: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Accrued(asset, payer))
            .unwrap_or(0)
    }

    /// Read the effective fee (in bps) for an asset.
    pub fn fee_bps(env: Env, asset: Address) -> u32 {
        Self::fee_bps_for(&env, &asset)
    }

    // ----- internal helpers -----

    fn admin(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    fn treasury(env: &Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Treasury)
            .ok_or(Error::NotInitialized)
    }

    fn fee_bps_for(env: &Env, asset: &Address) -> u32 {
        if let Some(bps) = env
            .storage()
            .persistent()
            .get::<DataKey, u32>(&DataKey::AssetFee(asset.clone()))
        {
            return bps;
        }
        env.storage()
            .instance()
            .get(&DataKey::DefaultFeeBps)
            .unwrap_or(0)
    }

    fn compute_fee(amount: i128, fee_bps: u32) -> Result<i128, Error> {
        let bps = fee_bps as i128;
        let numerator = amount.checked_mul(bps).ok_or(Error::Overflow)?;
        let fee = numerator / (BPS_DENOMINATOR as i128);
        Ok(fee)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Events, Ledger};
    use soroban_sdk::{
        contract, contractimpl, contracttype, token::StellarAssetClient, Address, Env, IntoVal,
        Symbol, TryFromVal, Val, Vec,
    };

    // -----------------------------------------------------------------------
    // Mock token contract used to exercise cross-contract invocations.
    // -----------------------------------------------------------------------

    #[contracttype]
    #[derive(Clone)]
    enum MockTokenKey {
        Balance(Address),
        Allowance(Address, Address),
        Total,
    }

    #[contract]
    pub struct MockToken;

    #[contractimpl]
    impl MockToken {
        pub fn mint(env: Env, to: Address, amount: i128) {
            let key = MockTokenKey::Balance(to.clone());
            let prev: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            env.storage().persistent().set(&key, &(prev + amount));
            let total: i128 = env
                .storage()
                .persistent()
                .get(&MockTokenKey::Total)
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&MockTokenKey::Total, &(total + amount));
        }

        pub fn balance(env: Env, id: Address) -> i128 {
            env.storage()
                .persistent()
                .get(&MockTokenKey::Balance(id))
                .unwrap_or(0)
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            assert!(amount >= 0, "negative transfer");
            let from_key = MockTokenKey::Balance(from.clone());
            let from_bal: i128 = env.storage().persistent().get(&from_key).unwrap_or(0);
            assert!(from_bal >= amount, "insufficient balance");
            env.storage()
                .persistent()
                .set(&from_key, &(from_bal - amount));

            let to_key = MockTokenKey::Balance(to.clone());
            let to_bal: i128 = env.storage().persistent().get(&to_key).unwrap_or(0);
            env.storage()
                .persistent()
                .set(&to_key, &(to_bal + amount));
        }
    }

    // -----------------------------------------------------------------------
    // Mock compliance contract used to verify cross-contract reads.
    // -----------------------------------------------------------------------

    #[contracttype]
    #[derive(Clone)]
    enum ComplianceKey {
        Approved(Address),
    }

    #[contract]
    pub struct MockCompliance;

    #[contractimpl]
    impl MockCompliance {
        pub fn approve(env: Env, who: Address) {
            env.storage()
                .persistent()
                .set(&ComplianceKey::Approved(who), &true);
        }

        pub fn is_approved(env: Env, who: Address) -> bool {
            env.storage()
                .persistent()
                .get(&ComplianceKey::Approved(who))
                .unwrap_or(false)
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    struct Harness {
        env: Env,
        admin: Address,
        treasury: Address,
        payer: Address,
        asset: Address,
        fee_manager: Address,
    }

    fn setup() -> Harness {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let treasury = Address::generate(&env);
        let payer = Address::generate(&env);

        let fee_manager = env.register_contract(None, FeeManager);
        let asset = env.register_contract(None, MockToken);

        // Mint initial balance to the payer.
        let token_client = MockTokenClient::new(&env, &asset);
        token_client.mint(&payer, &1_000_000i128);

        // Initialize the fee-manager with a 100 bps (1%) default fee.
        let fm_client = FeeManagerClient::new(&env, &fee_manager);
        fm_client.initialize(&admin, &treasury, &100u32);

        Harness {
            env,
            admin,
            treasury,
            payer,
            asset,
            fee_manager,
        }
    }

    fn find_event(env: &Env, topic: Symbol) -> bool {
        let events = env.events().all();
        let mut i = 0;
        while i < events.len() {
            let (_contract, topics, _data) = events.get(i).unwrap();
            if topics.len() >= 1 {
                if let Ok(sym) = Symbol::try_from_val(env, &topics.get(0).unwrap()) {
                    if sym == topic {
                        return true;
                    }
                }
            }
            i += 1;
        }
        false
    }

    // -----------------------------------------------------------------------
    // Tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_initialize_emits_event() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let treasury = Address::generate(&env);
        let fee_manager = env.register_contract(None, FeeManager);
        let client = FeeManagerClient::new(&env, &fee_manager);

        client.initialize(&admin, &treasury, &250u32);
        assert_eq!(client.fee_bps(&treasury), 250u32);
        assert!(find_event(&env, TOPIC_INIT));
    }

    #[test]
    fn test_initialize_rejects_invalid_fee() {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let treasury = Address::generate(&env);
        let fee_manager = env.register_contract(None, FeeManager);
        let client = FeeManagerClient::new(&env, &fee_manager);

        let res = client.try_initialize(&admin, &treasury, &20_000u32);
        assert_eq!(res, Err(Ok(Error::InvalidFeeBps)));
    }

    #[test]
    fn test_quote_fee_uses_default() {
        let h = setup();
        let client = FeeManagerClient::new(&h.env, &h.fee_manager);
        // 1% of 10_000 = 100
        let fee = client.quote_fee(&h.asset, &10_000i128);
        assert_eq!(fee, 100i128);
    }

    #[test]
    fn test_quote_fee_uses_asset_override() {
        let h = setup();
        let client = FeeManagerClient::new(&h.env, &h.fee_manager);
        client.set_asset_fee(&h.asset, &500u32);
        // 5% of 10_000 = 500
        let fee = client.quote_fee(&h.asset, &10_000i128);
        assert_eq!(fee, 500i128);
    }

    #[test]
    fn test_quote_fee_rejects_zero_amount() {
        let h = setup();
        let client = FeeManagerClient::new(&h.env, &h.fee_manager);
        let res = client.try_quote_fee(&h.asset, &0i128);
        assert_eq!(res, Err(Ok(Error::InvalidAmount)));
    }

    #[test]
    fn test_collect_fee_cross_contract_transfer() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);
        let token = MockTokenClient::new(&h.env, &h.asset);

        let payer_before = token.balance(&h.payer);
        let treasury_before = token.balance(&h.treasury);

        // 1% of 100_000 = 1_000
        let fee = fm.collect_fee(&h.payer, &h.asset, &100_000i128);
        assert_eq!(fee, 1_000i128);

        assert_eq!(token.balance(&h.payer), payer_before - 1_000);
        assert_eq!(token.balance(&h.treasury), treasury_before + 1_000);
        assert_eq!(fm.accrued(&h.asset, &h.payer), 1_000i128);
        assert!(find_event(&h.env, TOPIC_COLLECTED));
    }

    #[test]
    fn test_collect_fee_accumulates() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);

        fm.collect_fee(&h.payer, &h.asset, &100_000i128);
        fm.collect_fee(&h.payer, &h.asset, &50_000i128);

        // 1% of 100_000 + 1% of 50_000 = 1_000 + 500 = 1_500
        assert_eq!(fm.accrued(&h.asset, &h.payer), 1_500i128);
    }

    #[test]
    fn test_collect_fee_zero_fee_no_transfer() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);
        let token = MockTokenClient::new(&h.env, &h.asset);

        fm.set_asset_fee(&h.asset, &0u32);
        let payer_before = token.balance(&h.payer);
        let treasury_before = token.balance(&h.treasury);

        let fee = fm.collect_fee(&h.payer, &h.asset, &100_000i128);
        assert_eq!(fee, 0i128);
        assert_eq!(token.balance(&h.payer), payer_before);
        assert_eq!(token.balance(&h.treasury), treasury_before);
        assert_eq!(fm.accrued(&h.asset, &h.payer), 0i128);
    }

    #[test]
    fn test_collect_fee_rejects_invalid_amount() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);
        let res = fm.try_collect_fee(&h.payer, &h.asset, &0i128);
        assert_eq!(res, Err(Ok(Error::InvalidAmount)));
    }

    #[test]
    fn test_withdraw_accrued_cross_contract_transfer() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);
        let token = MockTokenClient::new(&h.env, &h.asset);

        // Accrue 1_000 in fees.
        fm.collect_fee(&h.payer, &h.asset, &100_000i128);
        assert_eq!(fm.accrued(&h.asset, &h.payer), 1_000i128);

        let treasury_before = token.balance(&h.treasury);
        let withdrawn = fm.withdraw_accrued(&h.asset, &h.payer);
        assert_eq!(withdrawn, 1_000i128);
        assert_eq!(token.balance(&h.treasury), treasury_before + 1_000);
        assert_eq!(fm.accrued(&h.asset, &h.payer), 0i128);
        assert!(find_event(&h.env, TOPIC_WITHDRAWN));
    }

    #[test]
    fn test_withdraw_accrued_noop_when_zero() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);
        let withdrawn = fm.withdraw_accrued(&h.asset, &h.payer);
        assert_eq!(withdrawn, 0i128);
    }

    #[test]
    fn test_set_asset_fee_emits_event() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);
        fm.set_asset_fee(&h.asset, &300u32);
        assert_eq!(fm.fee_bps(&h.asset), 300u32);
        assert!(find_event(&h.env, TOPIC_FEE_SET));
    }

    #[test]
    fn test_set_asset_fee_rejects_invalid() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);
        let res = fm.try_set_asset_fee(&h.asset, &20_000u32);
        assert_eq!(res, Err(Ok(Error::InvalidFeeBps)));
    }

    #[test]
    fn test_set_default_fee() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);
        fm.set_default_fee(&200u32);
        // No asset override, so default applies.
        let fee = fm.quote_fee(&h.asset, &10_000i128);
        assert_eq!(fee, 200i128);
    }

    #[test]
    fn test_set_treasury() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);
        let new_treasury = Address::generate(&h.env);
        fm.set_treasury(&new_treasury);

        let token = MockTokenClient::new(&h.env, &h.asset);
        let before = token.balance(&new_treasury);
        fm.collect_fee(&h.payer, &h.asset, &100_000i128);
        assert_eq!(token.balance(&new_treasury), before + 1_000);
    }

    #[test]
    fn test_compliance_mock_cross_contract_read() {
        // Demonstrates a cross-contract read against a second mock contract.
        let env = Env::default();
        env.mock_all_auths();
        let compliance = env.register_contract(None, MockCompliance);
        let client = MockComplianceClient::new(&env, &compliance);
        let who = Address::generate(&env);

        assert!(!client.is_approved(&who));
        client.approve(&who);
        assert!(client.is_approved(&who));
    }

    #[test]
    fn test_uninitialized_returns_error() {
        let env = Env::default();
        env.mock_all_auths();
        let fee_manager = env.register_contract(None, FeeManager);
        let client = FeeManagerClient::new(&env, &fee_manager);
        let asset = Address::generate(&env);
        let res = client.try_quote_fee(&asset, &1_000i128);
        // fee_bps_for falls back to 0 when uninitialized, so quote returns 0.
        assert_eq!(res, Ok(Ok(0i128)));
    }

    #[test]
    fn test_overflow_guard() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);
        // Set 100% fee and try to quote with i128::MAX.
        fm.set_asset_fee(&h.asset, &10_000u32);
        let res = fm.try_quote_fee(&h.asset, &i128::MAX);
        assert_eq!(res, Err(Ok(Error::Overflow)));
    }

    #[test]
    fn test_collect_fee_insufficient_balance_panics() {
        let h = setup();
        let fm = FeeManagerClient::new(&h.env, &h.fee_manager);
        // Payer has 1_000_000; 1% of 1_000_000_000 = 10_000_000 > balance.
        let res = fm.try_collect_fee(&h.payer, &h.asset, &1_000_000_000i128);
        assert!(res.is_err());
    }
}