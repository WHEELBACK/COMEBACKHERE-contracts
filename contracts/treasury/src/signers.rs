use crate::{require_admin, DataKey, RotationStatus, SignerRotationProposal, TreasuryContract};
use multisig::{require_authorized_signer, signer_weight};
use soroban_sdk::{contractimpl, Address, Env, Symbol, Vec};

#[contractimpl]
impl TreasuryContract {
    /// Registers or updates the approval weight of `signer` (admin-only). Weight 0 deactivates the signer.
    /// Emits: `signer_weight_set`.
    pub fn set_signer(env: Env, admin: Address, signer: Address, weight: u32) {
        require_admin(&env, &admin);
        env.storage()
            .instance()
            .set(&DataKey::Signer(signer.clone()), &weight);
        let mut list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::SignerList)
            .unwrap_or_else(|| Vec::new(&env));
        if weight > 0 {
            if !list.contains(&signer) {
                list.push_back(signer.clone());
                env.storage().instance().set(&DataKey::SignerList, &list);
            }
        } else {
            let mut updated = Vec::new(&env);
            for s in list.iter() {
                if s != signer {
                    updated.push_back(s);
                }
            }
            env.storage().instance().set(&DataKey::SignerList, &updated);
        }
        env.events()
            .publish((Symbol::new(&env, "signer_weight_set"), signer), weight);
    }

    /// Returns the current approval weight for `signer`, or `0` if not registered.
    pub fn get_signer_weight(env: Env, signer: Address) -> u32 {
        signer_weight(&env, &signer)
    }

    /// Returns all registered signers and their current weights.
    pub fn get_all_signers(env: Env) -> Vec<(Address, u32)> {
        let list: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::SignerList)
            .unwrap_or_else(|| Vec::new(&env));
        let mut result = Vec::new(&env);
        for signer in list.iter() {
            let weight: u32 = env
                .storage()
                .instance()
                .get(&DataKey::Signer(signer.clone()))
                .unwrap_or(0);
            result.push_back((signer, weight));
        }
        result
    }

    /// Proposes replacing `old_signer` with `new_signer` in the authorised signer set.
    /// Emits: `rotation_proposed`.
    pub fn propose_signer_rotation(
        env: Env,
        proposer: Address,
        old_signer: Address,
        new_signer: Address,
    ) -> u64 {
        require_authorized_signer(&env, &proposer);
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::RotationCount)
            .unwrap_or(0);
        let id = count + 1;
        let weight = signer_weight(&env, &proposer);
        let mut approvals = Vec::new(&env);
        approvals.push_back(proposer);
        let proposal = SignerRotationProposal {
            id,
            old_signer,
            new_signer,
            approvals,
            approval_weight: weight,
            status: RotationStatus::Pending,
        };
        env.storage()
            .persistent()
            .set(&DataKey::SignerRotation(id), &proposal);
        env.storage().instance().set(&DataKey::RotationCount, &id);
        env.events()
            .publish((Symbol::new(&env, "rotation_proposed"), id), proposal);
        id
    }

    /// Approves a pending signer rotation; executes the swap when cumulative weight meets threshold.
    /// Panics: `UnauthorizedSigner`, `RotationNotFound`, `RotationAlreadyExecuted`.
    /// Emits: `rotation_approved`; additionally `rotation_executed` when threshold is met.
    pub fn approve_signer_rotation(
        env: Env,
        approver: Address,
        rotation_id: u64,
    ) -> SignerRotationProposal {
        require_authorized_signer(&env, &approver);
        let mut proposal: SignerRotationProposal = env
            .storage()
            .persistent()
            .get(&DataKey::SignerRotation(rotation_id))
            .unwrap_or_else(|| panic!("RotationNotFound"));
        if proposal.status != RotationStatus::Pending {
            panic!("RotationAlreadyExecuted");
        }
        if !proposal.approvals.contains(&approver) {
            proposal.approval_weight += signer_weight(&env, &approver);
            proposal.approvals.push_back(approver);
        }
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Threshold)
            .unwrap_or(1);
        if proposal.approval_weight >= threshold {
            let old_weight = signer_weight(&env, &proposal.old_signer);
            env.storage()
                .instance()
                .set(&DataKey::Signer(proposal.new_signer.clone()), &old_weight);
            env.storage()
                .instance()
                .set(&DataKey::Signer(proposal.old_signer.clone()), &0u32);
            proposal.status = RotationStatus::Executed;
            env.events().publish(
                (Symbol::new(&env, "rotation_executed"), rotation_id),
                proposal.clone(),
            );
        }
        env.storage()
            .persistent()
            .set(&DataKey::SignerRotation(rotation_id), &proposal);
        env.events().publish(
            (Symbol::new(&env, "rotation_approved"), rotation_id),
            proposal.clone(),
        );
        proposal
    }

    /// Cancels a pending signer rotation proposal (admin-only).
    /// Panics: `Unauthorized`, `RotationNotFound`, `RotationAlreadyExecuted`.
    /// Emits: `rotation_cancelled`.
    pub fn cancel_rotation(env: Env, admin: Address, rotation_id: u64) -> SignerRotationProposal {
        require_admin(&env, &admin);
        let mut proposal: SignerRotationProposal = env
            .storage()
            .persistent()
            .get(&DataKey::SignerRotation(rotation_id))
            .unwrap_or_else(|| panic!("RotationNotFound"));
        if proposal.status != RotationStatus::Pending {
            panic!("RotationAlreadyExecuted");
        }
        proposal.status = RotationStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::SignerRotation(rotation_id), &proposal);
        env.events().publish(
            (Symbol::new(&env, "rotation_cancelled"), rotation_id),
            proposal.clone(),
        );
        proposal
    }
}
