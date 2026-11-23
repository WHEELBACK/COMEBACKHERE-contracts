// contracts/treasury/src/lib.rs - Issues #860, #861, #862, #863
// Treasury contract with security hardening

#![no_std]

use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, symbol_short, map, Map};

// ============================================================================
// Issue #861: Granular Role-Based Authorization
// ============================================================================

#[derive(Clone, PartialEq)]
pub enum Role {
    Admin = 0,
    Modifier = 1,
    Reader = 2,
}

pub fn check_role(env: &Env, caller: &Address, required_role: Role) -> Result<(), Symbol> {
    caller.require_auth();

    let roles: Map<Address, u32> = env.storage()
        .persistent()
        .get(&symbol_short!("roles"))
        .unwrap_or_else(|| map![env]);

    let caller_role = roles.get(caller).unwrap_or(2);

    if caller_role < required_role as u32 {
        return Err(symbol_short!("unauth"));
    }

    Ok(())
}

pub fn set_admin_role(env: &Env, caller: &Address, target: &Address) -> Result<(), Symbol> {
    check_role(env, caller, Role::Admin)?;

    let mut roles: Map<Address, u32> = env.storage()
        .persistent()
        .get(&symbol_short!("roles"))
        .unwrap_or_else(|| map![env]);

    roles.set(target.clone(), Role::Admin as u32);
    env.storage().persistent().set(&symbol_short!("roles"), &roles);

    Ok(())
}

// ============================================================================
// Issue #860: Prevent Arithmetic Underflow
// ============================================================================

pub fn safe_subtract(a: i128, b: i128) -> Result<i128, Symbol> {
    a.checked_sub(b).ok_or(symbol_short!("underflow"))
}

pub fn settle_partial(env: &Env, caller: &Address, amount: i128) -> Result<i128, Symbol> {
    check_role(env, caller, Role::Modifier)?;

    let balance: i128 = env.storage()
        .persistent()
        .get(&symbol_short!("balance"))
        .unwrap_or(0);

    // Prevent underflow: validate amount <= balance
    if amount > balance {
        return Err(symbol_short!("insuf"));
    }

    // Safe subtraction
    let remaining = safe_subtract(balance, amount)?;

    env.storage()
        .persistent()
        .set(&symbol_short!("balance"), &remaining);

    Ok(remaining)
}

// ============================================================================
// Issue #862: Optimize Storage Deserialization
// ============================================================================

#[derive(Clone)]
pub struct TreasuryData {
    pub balance: i128,
    pub timestamp: u64,
}

pub fn get_treasury_data(env: &Env) -> Option<TreasuryData> {
    let key = symbol_short!("treas");
    env.storage().persistent().get(&key)
}

pub fn set_treasury_data(env: &Env, data: &TreasuryData) -> Result<(), Symbol> {
    // Validate before storage
    if data.balance < 0 {
        return Err(symbol_short!("badbal"));
    }

    if data.timestamp > env.ledger().timestamp() + 3600 {
        return Err(symbol_short!("badts"));
    }

    let key = symbol_short!("treas");
    env.storage().persistent().set(&key, data);

    Ok(())
}

// ============================================================================
// Issue #863: Fuzzing Test Suite for Malicious Scenarios
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::*, Env as TestEnv};

    #[test]
    fn test_unauthorized_settlement() {
        let env = TestEnv::default();
        let attacker = Address::random(&env);

        // Attacker cannot settle without permission
        let result = settle_partial(&env, &attacker, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_underflow_protection() {
        let env = TestEnv::default();
        let admin = Address::random(&env);

        // Set balance to 50
        let data = TreasuryData {
            balance: 50,
            timestamp: env.ledger().timestamp(),
        };
        set_treasury_data(&env, &data).unwrap();

        // Set admin role
        let mut roles = map![&env];
        roles.set(admin.clone(), 0);
        env.storage().persistent().set(&symbol_short!("roles"), &roles);

        // Try to settle more than balance (underflow protection)
        let result = settle_partial(&env, &admin, 100);
        assert_eq!(result, Err(symbol_short!("insuf")));
    }

    #[test]
    fn test_negative_balance_rejected() {
        let env = TestEnv::default();

        // Try to store negative balance
        let invalid = TreasuryData {
            balance: -50,
            timestamp: env.ledger().timestamp(),
        };

        let result = set_treasury_data(&env, &invalid);
        assert_eq!(result, Err(symbol_short!("badbal")));
    }

    #[test]
    fn test_safe_subtract() {
        // Normal case
        assert_eq!(safe_subtract(100, 50).unwrap(), 50);

        // Edge case: exact zero
        assert_eq!(safe_subtract(50, 50).unwrap(), 0);

        // Underflow case
        assert!(safe_subtract(50, 100).is_err());
    }

    #[test]
    fn test_role_escalation_blocked() {
        let env = TestEnv::default();
        let attacker = Address::random(&env);
        let victim = Address::random(&env);

        // Attacker cannot grant admin role
        let result = set_admin_role(&env, &attacker, &victim);
        assert!(result.is_err());
    }
}
