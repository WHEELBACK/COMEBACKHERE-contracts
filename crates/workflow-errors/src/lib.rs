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
        /// `resume_all` was called with a non-zero `min_delay_secs` before that
        /// many seconds had elapsed since the emergency pause was recorded (#780).
        ResumeTooEarly = 11,
        /// The contract's stored state violates one of the workflow invariants
        /// (#873): e.g. the pinned compliance and treasury instances are the same
        /// contract or the workflow itself, a required key is missing, or an
        /// emergency pause is recorded without a valid target set.
        InvariantViolation = 12,
        /// Treasury reported a settlement as executed, but the settlement it
        /// returns afterwards is not in the `Executed` state (#873). The whole
        /// invocation is rolled back rather than emitting a misleading event.
        SettlementNotExecuted = 13,
    }
}

impl WorkflowError {
    /// Every variant, in discriminant order. Kept next to the enum so a new
    /// variant cannot be added without also giving it a message (#874).
    pub const ALL: [WorkflowError; 13] = [
        WorkflowError::Unauthorized,
        WorkflowError::NotConfigured,
        WorkflowError::PauseTargetFailed,
        WorkflowError::UnpauseTargetFailed,
        WorkflowError::NoPauseTargets,
        WorkflowError::TooManyPauseTargets,
        WorkflowError::DuplicatePauseTarget,
        WorkflowError::SelfPauseTarget,
        WorkflowError::EmergencyPauseActive,
        WorkflowError::NotEmergencyPaused,
        WorkflowError::ResumeTooEarly,
        WorkflowError::InvariantViolation,
        WorkflowError::SettlementNotExecuted,
    ];

    /// The stable on-chain `u32` code for this error.
    pub const fn code(self) -> u32 {
        self as u32
    }

    /// Maps an on-chain error code back to its variant, so clients that only
    /// see the raw `Error(Contract, #n)` can recover the typed error (#874).
    pub fn from_code(code: u32) -> Option<WorkflowError> {
        WorkflowError::ALL.iter().copied().find(|e| e.code() == code)
    }

    /// Stable, human-readable name of the variant (#874).
    pub const fn name(self) -> &'static str {
        match self {
            WorkflowError::Unauthorized => "Unauthorized",
            WorkflowError::NotConfigured => "NotConfigured",
            WorkflowError::PauseTargetFailed => "PauseTargetFailed",
            WorkflowError::UnpauseTargetFailed => "UnpauseTargetFailed",
            WorkflowError::NoPauseTargets => "NoPauseTargets",
            WorkflowError::TooManyPauseTargets => "TooManyPauseTargets",
            WorkflowError::DuplicatePauseTarget => "DuplicatePauseTarget",
            WorkflowError::SelfPauseTarget => "SelfPauseTarget",
            WorkflowError::EmergencyPauseActive => "EmergencyPauseActive",
            WorkflowError::NotEmergencyPaused => "NotEmergencyPaused",
            WorkflowError::ResumeTooEarly => "ResumeTooEarly",
            WorkflowError::InvariantViolation => "InvariantViolation",
            WorkflowError::SettlementNotExecuted => "SettlementNotExecuted",
        }
    }

    /// Custom, client-facing explanation of the error and how to recover from
    /// it (#874). Exposed on-chain through the workflow's `get_error_message`.
    pub const fn message(self) -> &'static str {
        match self {
            WorkflowError::Unauthorized => {
                "caller is not the configured administrator for this operation"
            }
            WorkflowError::NotConfigured => {
                "required configuration is missing; initialize the workflow or emergency pause first"
            }
            WorkflowError::PauseTargetFailed => {
                "a target refused to pause; the whole sweep was rolled back"
            }
            WorkflowError::UnpauseTargetFailed => {
                "a target refused to unpause; the whole resume was rolled back"
            }
            WorkflowError::NoPauseTargets => "emergency pause target list must not be empty",
            WorkflowError::TooManyPauseTargets => {
                "emergency pause target list exceeds MAX_PAUSE_TARGETS"
            }
            WorkflowError::DuplicatePauseTarget => {
                "emergency pause target list contains a duplicate address"
            }
            WorkflowError::SelfPauseTarget => {
                "the workflow contract cannot be its own emergency pause target"
            }
            WorkflowError::EmergencyPauseActive => {
                "an emergency pause is active; resume_all before retrying"
            }
            WorkflowError::NotEmergencyPaused => "no emergency pause is active to resume",
            WorkflowError::ResumeTooEarly => {
                "min_delay_secs has not elapsed since the emergency pause; retry later"
            }
            WorkflowError::InvariantViolation => {
                "stored workflow state is inconsistent; call verify_invariants for details"
            }
            WorkflowError::SettlementNotExecuted => {
                "treasury did not leave the settlement in the Executed state; invocation reverted"
            }
        }
    }
}
