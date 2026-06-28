# Treasury Contract

The Treasury contract manages funds and settlements using a multi-signature approval process. It supports settlement proposals, partial settlements, disputes, and signer rotations.

## Entrypoints

| Function | Auth Required | Parameters | Returns | Errors |
|----------|---------------|------------|---------|--------|
| `initialize` | `admin` | `admin: Address, threshold: u32` | `Result<(), TreasuryError>` | `AlreadyInitialized`, `ZeroThreshold` |
| `set_signer` | `admin` | `admin: Address, signer: Address, weight: u32` | `()` | `Unauthorized` |
| `propose_settlement` | `signer` | `signer: Address, merchant_address: Address, amount: i128` | `u64` | `ContractPaused`, `UnauthorizedSigner`, `InvalidAmount` |
| `propose_partial_settlement` | `signer` | `signer: Address, merchant_address: Address, amount: i128` | `u64` | `ContractPaused`, `UnauthorizedSigner`, `InvalidAmount` |
| `approve_settlement` | `signer` | `signer: Address, settlement_id: u64` | `Settlement` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted` |
| `approve_partial_settlement` | `signer` | `signer: Address, settlement_id: u64, partial_amount: i128` | `Settlement` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`, `InvalidAmount` |
| `execute_settlement` | `signer` | `signer: Address, settlement_id: u64, token_contract: Address` | `()` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `SettlementOnHold`, `AlreadyExecuted`, `ThresholdNotConfigured`, `ThresholdNotMet`, `InvalidTokenContract`, `TokenNotAllowed` |
| `partially_execute_settlement` | `signer` | `signer: Address, settlement_id: u64, partial_amount: i128, token_contract: Address` | `()` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `AlreadyExecuted`, `ThresholdNotConfigured`, `ThresholdNotMet`, `InvalidTokenContract`, `InvalidAmount` |
| `cancel_settlement` | `signer` | `signer: Address, settlement_id: u64` | `()` | `ContractPaused`, `UnauthorizedSigner`, `SettlementNotFound`, `SettlementNotCancellable` |
| `get_pending_settlements` | None | None | `Vec<Settlement>` | None |
| `get_pending_settlements_page` | None | `start: u64, limit: u64` | `Vec<Settlement>` | None |
| `get_settlement` | None | `settlement_id: u64` | `Settlement` | `SettlementNotFound` |
| `update_threshold` | `admin` | `admin: Address, new_threshold: u32` | `Result<(), TreasuryError>` | `Unauthorized`, `ZeroThreshold` |
| `pause` | `admin` | `admin: Address` | `()` | `Unauthorized` |
| `unpause` | `admin` | `admin: Address` | `()` | `Unauthorized` |
| `raise_dispute` | `claimant` | `claimant: Address, settlement_id: u64, counterparty: Address, amount: i128` | `u64` | `ContractPaused`, `Unauthorized`, `InvalidAmount` |
| `resolve_dispute` | `admin` | `admin: Address, dispute_id: u64, in_favor_of_claimant: bool` | `()` | `Unauthorized`, `ContractPaused`, `DisputeNotFound`, `DisputeAlreadyResolved` |
| `vote_dispute_resolution` | `signer` | `signer: Address, dispute_id: u64, in_favor_of_claimant: bool` | `()` | `ContractPaused`, `UnauthorizedSigner`, `DisputeNotFound`, `DisputeAlreadyResolved`, `ResolutionDirectionMismatch` |
| `deposit` | `from` | `from: Address, token_contract: Address, amount: i128` | `()` | `ContractPaused`, `Unauthorized`, `InvalidAmount` |
| `withdraw` | `to` | `to: Address, token_contract: Address, amount: i128` | `()` | `ContractPaused`, `Unauthorized`, `InvalidAmount`, `InsufficientBalance` |
| `add_allowed_token` | `admin` | `admin: Address, token: Address` | `()` | `Unauthorized` |
| `remove_allowed_token` | `admin` | `admin: Address, token: Address` | `()` | `Unauthorized` |
| `get_allowed_tokens` | None | None | `Vec<Address>` | None |
| `propose_signer_rotation` | `proposer` | `proposer: Address, old_signer: Address, new_signer: Address` | `u64` | `UnauthorizedSigner` |
| `approve_signer_rotation` | `approver` | `approver: Address, rotation_id: u64` | `SignerRotationProposal` | `UnauthorizedSigner`, `RotationNotFound`, `RotationAlreadyExecuted` |
| `update_merchant_payout_address` | `merchant` | `merchant: Address, new_payout_address: Address` | `()` | `ContractPaused`, `Unauthorized` |
| `get_merchant_payout_address` | None | `merchant: Address` | `Option<Address>` | None |
| `hold_settlement` | `admin` | `admin: Address, settlement_id: u64, reason: SettlementHoldReason` | `()` | `Unauthorized`, `SettlementNotFound`, `AlreadyExecuted` |
| `release_hold` | `admin` | `admin: Address, settlement_id: u64` | `()` | `Unauthorized`, `SettlementNotFound`, `NotOnHold` |
