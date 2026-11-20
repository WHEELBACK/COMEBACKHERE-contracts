#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Symbol,
};

/// Ledger threshold (in ledgers) below which persistent entries are bumped.
const TTL_THRESHOLD: u32 = 100;
/// Ledger target (in ledgers) to extend persistent entries to.
const TTL_EXTEND_TO: u32 = 10_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// Arithmetic overflow occurred.
    Overflow = 1,
    /// Arithmetic underflow occurred.
    Underflow = 2,
    /// Escrow not found.
    NotFound = 3,
    /// Escrow already exists.
    AlreadyExists = 4,
    /// Escrow is not in a state that permits the operation.
    InvalidState = 5,
    /// Amount must be strictly positive.
    InvalidAmount = 6,
    /// Settlement amount exceeds remaining balance.
    InsufficientFunds = 7,
    /// Invariant verification failed.
    InvariantViolation = 8,
    /// Unauthorized caller.
    Unauthorized = 9,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EscrowStatus {
    Active,
    Settled,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Escrow {
    pub depositor: Address,
    pub beneficiary: Address,
    pub total: i128,
    pub settled: i128,
    pub status: EscrowStatus,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Escrow(Symbol),
}

#[contract]
pub struct EscrowVault;

#[contractimpl]
impl EscrowVault {
    /// Create a new escrow record.
    pub fn create_escrow(
        env: Env,
        id: Symbol,
        depositor: Address,
        beneficiary: Address,
        total: i128,
    ) -> Result<(), Error> {
        depositor.require_auth();

        if total <= 0 {
            return Err(Error::InvalidAmount);
        }

        let key = DataKey::Escrow(id.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyExists);
        }

        let escrow = Escrow {
            depositor,
            beneficiary,
            total,
            settled: 0,
            status: EscrowStatus::Active,
        };

        Self::verify_invariants(&escrow)?;
        Self::write_escrow(&env, &key, &escrow);
        Ok(())
    }

    /// Perform a partial settlement of the escrow.
    pub fn settle(env: Env, id: Symbol, amount: i128) -> Result<i128, Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let key = DataKey::Escrow(id.clone());
        let mut escrow = Self::read_escrow(&env, &key)?;

        if escrow.status != EscrowStatus::Active {
            return Err(Error::InvalidState);
        }

        escrow.beneficiary.require_auth();

        // Compute remaining balance using checked arithmetic to prevent underflow.
        let remaining = escrow
            .total
            .checked_sub(escrow.settled)
            .ok_or(Error::Underflow)?;

        if amount > remaining {
            return Err(Error::InsufficientFunds);
        }

        // Compute new settled amount using checked addition to prevent overflow.
        let new_settled = escrow
            .settled
            .checked_add(amount)
            .ok_or(Error::Overflow)?;

        escrow.settled = new_settled;

        if escrow.settled == escrow.total {
            escrow.status = EscrowStatus::Settled;
        }

        Self::verify_invariants(&escrow)?;
        Self::write_escrow(&env, &key, &escrow);

        Ok(escrow.settled)
    }

    /// Read an escrow record, bumping its TTL.
    pub fn get_escrow(env: Env, id: Symbol) -> Result<Escrow, Error> {
        let key = DataKey::Escrow(id);
        Self::read_escrow(&env, &key)
    }

    // ------------------------------------------------------------------
    // Internal helpers
    // ------------------------------------------------------------------

    /// Read an escrow from persistent storage, bumping its TTL.
    fn read_escrow(env: &Env, key: &DataKey) -> Result<Escrow, Error> {
        let escrow: Escrow = env
            .storage()
            .persistent()
            .get(key)
            .ok_or(Error::NotFound)?;
        Self::bump_ttl(env, key);
        Ok(escrow)
    }

    /// Write an escrow to persistent storage, bumping its TTL.
    fn write_escrow(env: &Env, key: &DataKey, escrow: &Escrow) {
        env.storage().persistent().set(key, escrow);
        Self::bump_ttl(env, key);
    }

    /// Extend the TTL of a persistent storage key to avoid ledger eviction.
    fn bump_ttl(env: &Env, key: &DataKey) {
        env.storage()
            .persistent()
            .extend_ttl(key, TTL_THRESHOLD, TTL_EXTEND_TO);
    }

    /// Verify all escrow invariants hold.
    fn verify_invariants(escrow: &Escrow) -> Result<(), Error> {
        // Total must be strictly positive.
        if escrow.total <= 0 {
            return Err(Error::InvariantViolation);
        }

        // Settled amount must be non-negative.
        if escrow.settled < 0 {
            return Err(Error::InvariantViolation);
        }

        // Settled amount must never exceed the total.
        if escrow.settled > escrow.total {
            return Err(Error::InvariantViolation);
        }

        // Status must be consistent with the settled amount.
        match escrow.status {
            EscrowStatus::Active => {
                if escrow.settled >= escrow.total {
                    return Err(Error::InvariantViolation);
                }
            }
            EscrowStatus::Settled => {
                if escrow.settled != escrow.total {
                    return Err(Error::InvariantViolation);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::Env;

    fn setup() -> (Env, EscrowVaultClient<'static>, Address, Address, Symbol) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, EscrowVault);
        let client = EscrowVaultClient::new(&env, &contract_id);
        let depositor = Address::generate(&env);
        let beneficiary = Address::generate(&env);
        let id = symbol_short!("esc1");
        (env, client, depositor, beneficiary, id)
    }

    #[test]
    fn test_create_and_settle_full() {
        let (_env, client, depositor, beneficiary, id) = setup();
        client.create_escrow(&id, &depositor, &beneficiary, &1000);
        let settled = client.settle(&id, &1000);
        assert_eq!(settled, 1000);
        let escrow = client.get_escrow(&id);
        assert_eq!(escrow.status, EscrowStatus::Settled);
        assert_eq!(escrow.settled, escrow.total);
    }

    #[test]
    fn test_partial_settlements() {
        let (_env, client, depositor, beneficiary, id) = setup();
        client.create_escrow(&id, &depositor, &beneficiary, &1000);
        assert_eq!(client.settle(&id, &250), 250);
        assert_eq!(client.settle(&id, &250), 500);
        assert_eq!(client.settle(&id, &500), 1000);
        let escrow = client.get_escrow(&id);
        assert_eq!(escrow.status, EscrowStatus::Settled);
    }

    #[test]
    fn test_settle_exceeds_remaining() {
        let (_env, client, depositor, beneficiary, id) = setup();
        client.create_escrow(&id, &depositor, &beneficiary, &1000);
        client.settle(&id, &600);
        let res = client.try_settle(&id, &500);
        assert_eq!(res, Err(Ok(Error::InsufficientFunds)));
    }

    #[test]
    fn test_settle_zero_amount() {
        let (_env, client, depositor, beneficiary, id) = setup();
        client.create_escrow(&id, &depositor, &beneficiary, &1000);
        let res = client.try_settle(&id, &0);
        assert_eq!(res, Err(Ok(Error::InvalidAmount)));
    }

    #[test]
    fn test_settle_negative_amount() {
        let (_env, client, depositor, beneficiary, id) = setup();
        client.create_escrow(&id, &depositor, &beneficiary, &1000);
        let res = client.try_settle(&id, &-1);
        assert_eq!(res, Err(Ok(Error::InvalidAmount)));
    }

    #[test]
    fn test_create_invalid_total() {
        let (_env, client, depositor, beneficiary, id) = setup();
        let res = client.try_create_escrow(&id, &depositor, &beneficiary, &0);
        assert_eq!(res, Err(Ok(Error::InvalidAmount)));
    }

    #[test]
    fn test_create_duplicate() {
        let (_env, client, depositor, beneficiary, id) = setup();
        client.create_escrow(&id, &depositor, &beneficiary, &1000);
        let res = client.try_create_escrow(&id, &depositor, &beneficiary, &1000);
        assert_eq!(res, Err(Ok(Error::AlreadyExists)));
    }

    #[test]
    fn test_settle_after_settled() {
        let (_env, client, depositor, beneficiary, id) = setup();
        client.create_escrow(&id, &depositor, &beneficiary, &1000);
        client.settle(&id, &1000);
        let res = client.try_settle(&id, &1);
        assert_eq!(res, Err(Ok(Error::InvalidState)));
    }

    #[test]
    fn test_get_missing() {
        let (_env, client, _depositor, _beneficiary, id) = setup();
        let res = client.try_get_escrow(&id);
        assert_eq!(res, Err(Ok(Error::NotFound)));
    }

    #[test]
    fn test_underflow_boundary() {
        // Settling exactly the remaining amount must succeed; one more must fail.
        let (_env, client, depositor, beneficiary, id) = setup();
        client.create_escrow(&id, &depositor, &beneficiary, &100);
        client.settle(&id, &99);
        let res = client.try_settle(&id, &2);
        assert_eq!(res, Err(Ok(Error::InsufficientFunds)));
        assert_eq!(client.settle(&id, &1), 100);
    }

    #[test]
    fn test_invariants_hold_after_sequence() {
        let (_env, client, depositor, beneficiary, id) = setup();
        client.create_escrow(&id, &depositor, &beneficiary, &1000);
        for amt in [100i128, 200, 300, 400] {
            client.settle(&id, &amt);
            let escrow = client.get_escrow(&id);
            assert!(escrow.settled >= 0);
            assert!(escrow.settled <= escrow.total);
        }
        let escrow = client.get_escrow(&id);
        assert_eq!(escrow.settled, escrow.total);
        assert_eq!(escrow.status, EscrowStatus::Settled);
    }

    #[test]
    fn test_ttl_extension() {
        let (env, client, depositor, beneficiary, id) = setup();
        client.create_escrow(&id, &depositor, &beneficiary, &1000);
        let key = DataKey::Escrow(id.clone());
        let contract_id = client.address.clone();
        let ttl = env.as_contract(&contract_id, || {
            env.storage().persistent().get_ttl(&key)
        });
        assert!(ttl >= TTL_THRESHOLD);
    }

    #[test]
    fn test_invariant_verifier_rejects_bad_state() {
        let env = Env::default();
        let addr = Address::generate(&env);
        let bad = Escrow {
            depositor: addr.clone(),
            beneficiary: addr.clone(),
            total: 100,
            settled: 200,
            status: EscrowStatus::Active,
        };
        assert_eq!(
            EscrowVault::verify_invariants(&bad),
            Err(Error::InvariantViolation)
        );

        let bad_status = Escrow {
            depositor: addr.clone(),
            beneficiary: addr.clone(),
            total: 100,
            settled: 100,
            status: EscrowStatus::Active,
        };
        assert_eq!(
            EscrowVault::verify_invariants(&bad_status),
            Err(Error::InvariantViolation)
        );

        let bad_negative = Escrow {
            depositor: addr.clone(),
            beneficiary: addr.clone(),
            total: 100,
            settled: -1,
            status: EscrowStatus::Active,
        };
        assert_eq!(
            EscrowVault::verify_invariants(&bad_negative),
            Err(Error::InvariantViolation)
        );
    }

    #[test]
    fn test_fuzz_partial_settlement_amounts() {
        // Deterministic pseudo-fuzz over many partial settlement amounts.
        let (_env, client, depositor, beneficiary, id) = setup();
        let total: i128 = 1_000_000;
        client.create_escrow(&id, &depositor, &beneficiary, &total);

        let mut seed: u64 = 0xDEAD_BEEF;
        let mut expected_settled: i128 = 0;
        for _ in 0..200 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let remaining = total - expected_settled;
            if remaining == 0 {
                break;
            }
            let amount = ((seed >> 33) as i128 % remaining) + 1;
            let new_settled = client.settle(&id, &amount);
            expected_settled += amount;
            assert_eq!(new_settled, expected_settled);
            let escrow = client.get_escrow(&id);
            assert_eq!(escrow.settled, expected_settled);
            assert!(escrow.settled <= escrow.total);
        }
        // Drain the remainder.
        let remaining = total - expected_settled;
        if remaining > 0 {
            client.settle(&id, &remaining);
        }
        let escrow = client.get_escrow(&id);
        assert_eq!(escrow.settled, escrow.total);
        assert_eq!(escrow.status, EscrowStatus::Settled);
    }

    #[test]
    fn test_fuzz_random_operation_sequences() {
        // Deterministic pseudo-fuzz over random operation sequences.
        let (_env, client, depositor, beneficiary, id) = setup();
        let total: i128 = 500_000;
        client.create_escrow(&id, &depositor, &beneficiary, &total);

        let mut seed: u64 = 0x1234_5678_9ABC_DEF0;
        let mut expected_settled: i128 = 0;
        let mut settled_status = false;

        for _ in 0..300 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let op = (seed >> 60) & 0x1;

            if op == 0 {
                // Attempt a settlement with a random amount.
                let amount = ((seed >> 20) as i128 % (total + 1)) + 1;
                let remaining = total - expected_settled;
                let res = client.try_settle(&id, &amount);
                if settled_status {
                    assert_eq!(res, Err(Ok(Error::InvalidState)));
                } else if amount > remaining {
                    assert_eq!(res, Err(Ok(Error::InsufficientFunds)));
                } else {
                    let new_settled = res.expect("settle should succeed");
                    expected_settled += amount;
                    assert_eq!(new_settled, expected_settled);
                    if expected_settled == total {
                        settled_status = true;
                    }
                }
            } else {
                // Read and verify invariants.
                let escrow = client.get_escrow(&id);
                assert!(escrow.settled >= 0);
                assert!(escrow.settled <= escrow.total);
                assert_eq!(escrow.settled, expected_settled);
                if settled_status {
                    assert_eq!(escrow.status, EscrowStatus::Settled);
                } else {
                    assert_eq!(escrow.status, EscrowStatus::Active);
                }
            }
        }
    }
}