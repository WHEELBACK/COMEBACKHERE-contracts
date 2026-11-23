use crate::types::TimeLock;
use soroban_sdk::{symbol_short, Env};

pub fn lock_created(env: &Env, id: u64, lock: &TimeLock) {
    env.events()
        .publish((symbol_short!("LOCKED"), id), lock.clone());
}

pub fn lock_released(env: &Env, id: u64, lock: &TimeLock) {
    env.events()
        .publish((symbol_short!("RELEASED"), id), lock.clone());
}

pub fn lock_cancelled(env: &Env, id: u64, lock: &TimeLock) {
    env.events()
        .publish((symbol_short!("CANCELLED"), id), lock.clone());
}

pub fn contract_paused(env: &Env) {
    env.events().publish((symbol_short!("PAUSED"),), ());
}

pub fn contract_unpaused(env: &Env) {
    env.events().publish((symbol_short!("UNPAUSED"),), ());
}
