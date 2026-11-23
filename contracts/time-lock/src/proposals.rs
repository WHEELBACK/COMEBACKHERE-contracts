//! Streamlined proposal deserialization for the time-lock contract (#943).
//!
//! A stored `TimelockProposal` is a sparse Soroban map with symbol keys. Most
//! callers only need to know whether the proposal is still active; decoding the
//! target address and three timestamps for a finalized proposal wastes host
//! CPU instructions and memory. Instead of reading the entry twice or adding a
//! second storage key (which would grow the instance footprint), this module
//! reads the raw `Val` **once**, unpacks the map keys in a single host call,
//! and inspects `status` first. Only active proposals pay the full conversion
//! cost — and those conversions are followed by the strict #945 invariant
//! check before the record is trusted.
//!
//! The keys must stay alphabetically sorted: that is the ordering the
//! `#[contracttype]` derive uses for `sparse_map_unpack_to_slice`.

use soroban_sdk::{Address, Env, EnvBase, MapObject, TryFromVal, Val};

use crate::{invariants, DataKey, TimeLockError, TimelockProposal, TimelockStatus};

const FIELD_KEYS: [&str; 5] = [
    "delay_secs",
    "executable_at",
    "proposed_at",
    "status",
    "target",
];
const STATUS_IDX: usize = 3;

/// Outcome of a lightweight proposal lookup.
pub(crate) enum ProposalLookup {
    /// No proposal is stored for the target.
    Missing,
    /// The proposal has a final status. Its remaining fields are deliberately
    /// not decoded (#943).
    Final,
    /// A well-formed, active proposal, fully verified against #945.
    Active(TimelockProposal),
}

/// Loads a proposal for a state transition or status query.
///
/// Reads the storage entry once. Finalized records short-circuit after the
/// status conversion; active records are fully reconstructed and verified.
pub(crate) fn load(env: &Env, target: &Address) -> Result<ProposalLookup, TimeLockError> {
    let raw: Val = match env
        .storage()
        .instance()
        .get::<_, Val>(&DataKey::TimelockProposal(target.clone()))
    {
        Some(val) => val,
        None => return Ok(ProposalLookup::Missing),
    };

    // One host call unpacks all five fields. This is the same call the
    // generated `TryFromVal<TimelockProposal>` impl performs.
    let map: MapObject = raw
        .try_into()
        .map_err(|_| TimeLockError::StateInconsistency)?;
    let mut fields = [Val::VOID.to_val(); FIELD_KEYS.len()];
    env.sparse_map_unpack_to_slice(map, &FIELD_KEYS, &mut fields)
        .map_err(|_| TimeLockError::StateInconsistency)?;

    let status = TimelockStatus::try_from_val(env, &fields[STATUS_IDX])
        .map_err(|_| TimeLockError::StateInconsistency)?;
    // Reject an undecodable/unknown status before treating the record as final.
    match status {
        TimelockStatus::Active | TimelockStatus::Executed | TimelockStatus::Cancelled => {}
    }

    if status != TimelockStatus::Active {
        return Ok(ProposalLookup::Final);
    }

    // Active records can authorize a delayed action, so pay the full
    // conversion cost and enforce every invariant (#945).
    let proposal = TimelockProposal {
        delay_secs: u64::try_from_val(env, &fields[0])
            .map_err(|_| TimeLockError::StateInconsistency)?,
        executable_at: u64::try_from_val(env, &fields[1])
            .map_err(|_| TimeLockError::StateInconsistency)?,
        proposed_at: u64::try_from_val(env, &fields[2])
            .map_err(|_| TimeLockError::StateInconsistency)?,
        status,
        target: Address::try_from_val(env, &fields[4])
            .map_err(|_| TimeLockError::StateInconsistency)?,
    };
    invariants::verify_proposal(&proposal, target)?;
    Ok(ProposalLookup::Active(proposal))
}
