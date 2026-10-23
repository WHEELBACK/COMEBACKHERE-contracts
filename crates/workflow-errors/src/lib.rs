#![no_std]

use error_macros::declare_contract_error;

declare_contract_error! {
    /// Error codes for the settlement-workflow contract.
    ///
    /// Variants are append-only and must not be renumbered; discriminants are
    /// part of the on-chain ABI and are matched by callers and off-chain systems.
    /// New variants must be added at the end with an explicit discriminant one
    /// higher than the current maximum; see `scripts/check-enum-ordering.sh`.
    pub enum WorkflowError {
        /// The caller is not the address registered as the emergency-pause admin.
        Unauthorized = 1,
        /// `initialize_emergency_pause` has not been called, so there is no target
        /// set to coordinate.
        NotConfigured = 2,
        /// A target refused to pause, or the invocation itself failed. The whole
        /// fan-out is rolled back, so no target is left paused by a partial sweep.
        PauseTargetFailed = 3,
        /// A target refused to unpause. The whole fan-out is rolled back, so the
        /// protocol is never left partially resumed.
        UnpauseTargetFailed = 4,
        /// An empty target set was supplied; there would be nothing to coordinate.
        NoPauseTargets = 5,
        /// More targets than `MAX_PAUSE_TARGETS`.
        TooManyPauseTargets = 6,
        /// The same address appears twice, which would pause it twice and make the
        /// second call the one that decides whether the fan-out succeeds.
        DuplicatePauseTarget = 7,
        /// The workflow contract itself was supplied as a target. Pausing itself
        /// part-way through the fan-out would make every remaining call fail,
        /// turning a recoverable incident into a workflow that cannot be resumed.
        SelfPauseTarget = 8,
        /// The target set cannot be reconfigured while an emergency pause is active:
        /// the new set would not match the state that is actually paused.
        EmergencyPauseActive = 9,
        /// `resume_all` was called while no emergency pause is active. Distinct from
        /// `NotConfigured` so an operator can tell "never set up" from "nothing to
        /// undo", and so the orchestrator cannot be used to unpause a protocol that
        /// somebody else paused through their own admin key.
        NotEmergencyPaused = 10,
    }
}
