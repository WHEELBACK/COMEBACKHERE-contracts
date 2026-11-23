//! Role-manager contract — admin-managed role assignment and permission
//! enforcement for the COMEBACKHERE protocol.
//!
//! # Overview
//!
//! This contract maintains a registry of `(Address, Role)` pairs and exposes
//! helpers that other contracts can call cross-contractually to gate sensitive
//! operations.  Three issues drive the implementation:
//!
//! * **#932** — Prevent arithmetic underflow during partial settlement
//!   computations.  Every counter that could underflow (`role_count`,
//!   quota arithmetic) uses `checked_sub` and returns
//!   [`RoleManagerError::ArithmeticUnderflow`] instead of wrapping.
//!
//! * **#934** — Reduce host memory and CPU instruction allocation by
//!   streamlining storage deserialization.  Role lookups are single-key
//!   persistent reads; the signer-list scan used in treasury-style contracts is
//!   avoided.  Where iteration is unavoidable (e.g. `list_role_members`) it is
//!   bounded by `MAX_ROLE_MEMBERS`.
//!
//! * **#935** — Fuzzing test suite covering malicious caller scenarios.  The
//!   tests in `tests/` cover: unauthorized role assignment, role revocation by
//!   wrong caller, quota underflow attempts, re-grant idempotency, and
//!   cross-role boundary violations.
//!
//! # Storage layout (footprint-optimised per #934)
//!
//! | Key                           | Kind       | Description                        |
//! |-------------------------------|------------|------------------------------------|
//! | `Admin`                       | Instance   | Contract administrator             |
//! | `Paused`                      | Instance   | Circuit-breaker flag               |
//! | `RoleCount`                   | Instance   | Live member count                  |
//! | `RoleMemberList`              | Instance   | Bounded index of tracked addresses |
//! | `Role(Address)`               | Persistent | Role assigned to an address        |
//! | `RoleGrantedAt(Address)`      | Persistent | Timestamp of latest grant          |
//! | `PartialQuota(Address)`       | Persistent | Remaining partial-settlement quota |
//!
//! Deserialization is kept minimal: a role check reads **one** key
//! (`Role(address)`) and nothing else.  Counter updates touch `RoleCount`
//! in instance storage (single read-modify-write) and `RoleMemberList` only
//! when a genuinely new member is added or removed.

#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env, Symbol, Vec};

// ── Error type ────────────────────────────────────────────────────────────────

/// Primary error type for the role-manager contract.
///
/// Variants are append-only to preserve on-chain backwards compatibility.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RoleManagerError {
    /// Contract was already initialized.
    AlreadyInitialized = 1,
    /// Caller is not the stored administrator.
    Unauthorized = 2,
    /// Contract is paused; state-mutating calls are rejected.
    ContractPaused = 3,
    /// Target address does not hold any role.
    AddressHasNoRole = 4,
    /// Role member cap (`MAX_ROLE_MEMBERS`) would be exceeded.
    RoleLimitReached = 5,
    /// An arithmetic addition would overflow.
    ArithmeticOverflow = 6,
    /// An arithmetic subtraction would produce a negative result.
    ///
    /// Returned by any counter decrement that would wrap around — the core
    /// protection for issue #932.
    ArithmeticUnderflow = 7,
    /// The requested partial quota reduction exceeds the stored quota.
    QuotaExceeded = 8,
    /// An invalid (zero or negative) amount was supplied.
    InvalidAmount = 9,
}

// ── Storage key type ──────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Paused,
    /// Live count of addresses that currently hold a role.  Single instance
    /// storage key — cheap read in `has_role` does not touch this at all.
    RoleCount,
    /// Bounded list of addresses that currently hold any role.  Consulted only
    /// for `list_role_members`; not read in hot-path `has_role` / `get_role`.
    RoleMemberList,
    /// The [`Role`] held by `address`.  Single-key persistent read — the
    /// most-accessed path in the contract (issue #934).
    Role(Address),
    /// Ledger timestamp at which the current role grant was recorded.
    RoleGrantedAt(Address),
    /// Per-address partial-settlement quota.  Decremented by `consume_quota`;
    /// protected against underflow via `checked_sub` (issue #932).
    PartialQuota(Address),
}

// ── Domain types ─────────────────────────────────────────────────────────────

/// The set of roles an address can hold.
///
/// Variants are append-only to preserve on-chain backwards compatibility.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Role {
    /// Can propose and approve settlements but not execute them.
    Proposer,
    /// Can approve and execute settlements.
    Executor,
    /// Read-only observer; not authorized to mutate state.
    Observer,
    /// Super-role: can reassign roles but not execute settlements directly.
    RoleAdmin,
}

/// Maximum number of tracked role members.  Caps `RoleMemberList` growth to
/// bound storage-rent costs (issue #934).
pub const MAX_ROLE_MEMBERS: u32 = 1_000;

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct RoleManagerContract;

#[contractimpl]
impl RoleManagerContract {
    // ── Lifecycle ─────────────────────────────────────────────────────────────

    /// Initialise the contract with `admin` as the owner.
    ///
    /// # Errors
    /// [`RoleManagerError::AlreadyInitialized`] — called a second time.
    ///
    /// Emits: `role_manager_initialized`.
    pub fn initialize(env: Env, admin: Address) -> Result<(), RoleManagerError> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(RoleManagerError::AlreadyInitialized);
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::RoleCount, &0u32);
        env.storage()
            .instance()
            .set(&DataKey::RoleMemberList, &Vec::<Address>::new(&env));
        env.events()
            .publish((Symbol::new(&env, "role_manager_initialized"),), admin);
        Ok(())
    }

    /// Pause the contract (admin-only).
    ///
    /// Emits: `role_manager_paused`.
    pub fn pause(env: Env, admin: Address) -> Result<(), RoleManagerError> {
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events()
            .publish((Symbol::new(&env, "role_manager_paused"),), admin);
        Ok(())
    }

    /// Resume normal operations (admin-only).
    ///
    /// Emits: `role_manager_unpaused`.
    pub fn unpause(env: Env, admin: Address) -> Result<(), RoleManagerError> {
        require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events()
            .publish((Symbol::new(&env, "role_manager_unpaused"),), admin);
        Ok(())
    }

    // ── Role management ───────────────────────────────────────────────────────

    /// Assign `role` to `target` (admin-only).
    ///
    /// Idempotent: re-granting the same role to an address that already holds
    /// it updates `RoleGrantedAt` but does not increment `RoleCount` again.
    ///
    /// # Errors
    /// [`RoleManagerError::RoleLimitReached`],
    /// [`RoleManagerError::ArithmeticOverflow`].
    ///
    /// Emits: `role_granted`.
    pub fn grant_role(
        env: Env,
        admin: Address,
        target: Address,
        role: Role,
    ) -> Result<(), RoleManagerError> {
        require_admin(&env, &admin)?;
        require_not_paused(&env)?;

        let already_has_role: bool = env
            .storage()
            .persistent()
            .get::<DataKey, Role>(&DataKey::Role(target.clone()))
            .is_some();

        if !already_has_role {
            // Check capacity before inserting (issue #934: bounded list).
            let count: u32 = env
                .storage()
                .instance()
                .get(&DataKey::RoleCount)
                .unwrap_or(0);
            if count >= MAX_ROLE_MEMBERS {
                return Err(RoleManagerError::RoleLimitReached);
            }
            let new_count = count
                .checked_add(1)
                .ok_or(RoleManagerError::ArithmeticOverflow)?;
            env.storage()
                .instance()
                .set(&DataKey::RoleCount, &new_count);

            // Append to the member list.
            let mut list: Vec<Address> = env
                .storage()
                .instance()
                .get(&DataKey::RoleMemberList)
                .unwrap_or_else(|| Vec::new(&env));
            list.push_back(target.clone());
            env.storage()
                .instance()
                .set(&DataKey::RoleMemberList, &list);
        }

        // Persist the role and grant timestamp with a single write each
        // (minimises deserialization overhead on the read path, issue #934).
        env.storage()
            .persistent()
            .set(&DataKey::Role(target.clone()), &role);
        env.storage().persistent().set(
            &DataKey::RoleGrantedAt(target.clone()),
            &env.ledger().timestamp(),
        );

        env.events()
            .publish((Symbol::new(&env, "role_granted"), target), role);
        Ok(())
    }

    /// Revoke the role of `target` (admin-only).
    ///
    /// # Errors
    /// [`RoleManagerError::AddressHasNoRole`],
    /// [`RoleManagerError::ArithmeticUnderflow`].
    ///
    /// Emits: `role_revoked`.
    pub fn revoke_role(env: Env, admin: Address, target: Address) -> Result<(), RoleManagerError> {
        require_admin(&env, &admin)?;
        require_not_paused(&env)?;

        let existing_role: Option<Role> = env
            .storage()
            .persistent()
            .get(&DataKey::Role(target.clone()));

        if existing_role.is_none() {
            return Err(RoleManagerError::AddressHasNoRole);
        }

        // Remove the role and its metadata.
        env.storage()
            .persistent()
            .remove(&DataKey::Role(target.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::RoleGrantedAt(target.clone()));
        env.storage()
            .persistent()
            .remove(&DataKey::PartialQuota(target.clone()));

        // ── Underflow guard (issue #932) ───────────────────────────────────
        //
        // `RoleCount` must never go negative. Use `checked_sub` so a corrupted
        // or adversarially crafted storage value (count == 0) does not wrap
        // around to u32::MAX.
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::RoleCount)
            .unwrap_or(0);
        let new_count = count
            .checked_sub(1)
            .ok_or(RoleManagerError::ArithmeticUnderflow)?;
        env.storage()
            .instance()
            .set(&DataKey::RoleCount, &new_count);

        // Remove from the member list.
        let list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::RoleMemberList)
            .unwrap_or_else(|| Vec::new(&env));
        let mut updated = Vec::new(&env);
        for addr in list.iter() {
            if addr != target {
                updated.push_back(addr);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::RoleMemberList, &updated);

        env.events()
            .publish((Symbol::new(&env, "role_revoked"),), target);
        Ok(())
    }

    // ── Quota management (partial settlement, issue #932) ─────────────────────

    /// Set the partial-settlement quota for `target` (admin-only).
    ///
    /// A quota of `0` effectively disallows any partial settlement by that address
    /// because `consume_quota` requires `amount > 0` and returns `QuotaExceeded`
    /// if `amount > quota`.
    ///
    /// # Errors
    /// [`RoleManagerError::AddressHasNoRole`], [`RoleManagerError::InvalidAmount`].
    ///
    /// Emits: `quota_set`.
    pub fn set_quota(
        env: Env,
        admin: Address,
        target: Address,
        quota: i128,
    ) -> Result<(), RoleManagerError> {
        require_admin(&env, &admin)?;
        require_not_paused(&env)?;
        if quota < 0 {
            return Err(RoleManagerError::InvalidAmount);
        }
        // Only addresses with a role may hold a quota.
        let _role: Role = env
            .storage()
            .persistent()
            .get(&DataKey::Role(target.clone()))
            .ok_or(RoleManagerError::AddressHasNoRole)?;

        env.storage()
            .persistent()
            .set(&DataKey::PartialQuota(target.clone()), &quota);

        env.events()
            .publish((Symbol::new(&env, "quota_set"), target), quota);
        Ok(())
    }

    /// Consume `amount` from `target`'s partial-settlement quota.
    ///
    /// This is the hot-path guard called by settlement workflows to enforce
    /// per-address partial-settlement limits.  The underflow risk (issue #932)
    /// arises when `amount > quota`: without `checked_sub` the remaining quota
    /// would silently wrap to a large positive number, allowing unlimited
    /// settlements.  The fix: return [`RoleManagerError::QuotaExceeded`] instead.
    ///
    /// # Errors
    /// [`RoleManagerError::ContractPaused`],
    /// [`RoleManagerError::AddressHasNoRole`],
    /// [`RoleManagerError::InvalidAmount`],
    /// [`RoleManagerError::QuotaExceeded`],
    /// [`RoleManagerError::ArithmeticUnderflow`].
    ///
    /// Emits: `quota_consumed`.
    pub fn consume_quota(
        env: Env,
        caller: Address,
        amount: i128,
    ) -> Result<i128, RoleManagerError> {
        require_not_paused(&env)?;
        caller.require_auth();

        if amount <= 0 {
            return Err(RoleManagerError::InvalidAmount);
        }

        // Caller must hold a role.
        let _role: Role = env
            .storage()
            .persistent()
            .get(&DataKey::Role(caller.clone()))
            .ok_or(RoleManagerError::AddressHasNoRole)?;

        let quota: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::PartialQuota(caller.clone()))
            .unwrap_or(0);

        if amount > quota {
            return Err(RoleManagerError::QuotaExceeded);
        }

        // ── Underflow guard (issue #932) ───────────────────────────────────
        //
        // `checked_sub` ensures that even if `quota` and `amount` are
        // inconsistent (e.g. storage corruption), we never produce a negative
        // remaining value that wraps into a large positive quota.
        let new_quota = quota
            .checked_sub(amount)
            .ok_or(RoleManagerError::ArithmeticUnderflow)?;

        env.storage()
            .persistent()
            .set(&DataKey::PartialQuota(caller.clone()), &new_quota);

        env.events().publish(
            (Symbol::new(&env, "quota_consumed"), caller),
            (amount, new_quota),
        );

        Ok(new_quota)
    }

    // ── View helpers (deserialization-streamlined, issue #934) ───────────────

    /// Return `true` if `address` currently holds any role.
    ///
    /// Single persistent key read — no iteration, no auxiliary counters.
    pub fn has_role(env: Env, address: Address) -> bool {
        env.storage().persistent().has(&DataKey::Role(address))
    }

    /// Return the [`Role`] held by `address`, or `None`.
    ///
    /// Single persistent key read (issue #934).
    pub fn get_role(env: Env, address: Address) -> Option<Role> {
        env.storage().persistent().get(&DataKey::Role(address))
    }

    /// Return `true` if `address` holds the specific `role`.
    ///
    /// Two-step: one persistent read for the role, one equality check.
    pub fn has_specific_role(env: Env, address: Address, role: Role) -> bool {
        env.storage()
            .persistent()
            .get::<DataKey, Role>(&DataKey::Role(address))
            .map(|r| r == role)
            .unwrap_or(false)
    }

    /// Return the timestamp at which `address`'s current role was granted,
    /// or `None` if the address holds no role.
    pub fn role_granted_at(env: Env, address: Address) -> Option<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::RoleGrantedAt(address))
    }

    /// Return the remaining partial-settlement quota for `address`.
    pub fn get_quota(env: Env, address: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::PartialQuota(address))
            .unwrap_or(0)
    }

    /// Return the total number of addresses currently holding a role.
    pub fn role_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::RoleCount)
            .unwrap_or(0)
    }

    /// Return all addresses currently holding a role.
    ///
    /// Bounded by `MAX_ROLE_MEMBERS` (issue #934).
    pub fn list_role_members(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::RoleMemberList)
            .unwrap_or_else(|| Vec::new(&env))
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn require_admin(env: &Env, admin: &Address) -> Result<(), RoleManagerError> {
    admin.require_auth();
    let stored: Address = env
        .storage()
        .instance()
        .get(&DataKey::Admin)
        .ok_or(RoleManagerError::Unauthorized)?;
    if stored != *admin {
        return Err(RoleManagerError::Unauthorized);
    }
    Ok(())
}

fn require_not_paused(env: &Env) -> Result<(), RoleManagerError> {
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false);
    if paused {
        return Err(RoleManagerError::ContractPaused);
    }
    Ok(())
}

#[cfg(test)]
extern crate std;
