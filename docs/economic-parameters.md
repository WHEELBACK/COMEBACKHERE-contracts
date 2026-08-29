# Economic Parameters

> **Glossary:** For term definitions, see [`GLOSSARY.md`](GLOSSARY.md). For contract
> data storage, see [`../ARCHITECTURE.md`](../ARCHITECTURE.md).

## Protocol fee on settlements

**Status: intentionally fee-free by design.** `Treasury::execute_settlement` pays out
the full, exact proposed amount with no deduction of any kind, and this is a
deliberate design choice, not an unaddressed gap.

### Rationale

- **Simplicity of the settlement path.** The treasury's settlement logic is a
  multisig-gated transfer of a pre-agreed amount (see `propose_settlement` /
  `approve_settlement` / `execute_settlement` in `contracts/treasury`). Any deduction
  would need to happen either at proposal time (changing what signers are actually
  approving) or at execution time (changing what the merchant actually receives
  relative to the invoice amount they were quoted) — both add a second amount that
  has to be reasoned about, audited, and kept consistent with `contracts/invoice`'s
  own `amount` field.
- **No fee-recipient trust boundary today.** A percentage or flat fee requires a
  fee-recipient address and a policy for who can change it. Introducing that address
  as a new privileged role (or overloading `Admin`) is a real trust/governance
  decision this protocol has not made, and doing it implicitly via this issue would
  bypass that decision rather than make it.
- **Funding model is out of scope for the on-chain contracts.** To the extent this
  protocol needs to sustain itself economically, that is presently assumed to be a
  business-layer concern (e.g. integration fees, subscription pricing, or a spread
  negotiated off-chain) rather than a per-settlement on-chain deduction. Nothing in
  `SECURITY.md` or `docs/audit-scope.md` requires an on-chain fee mechanism.

### When this should be revisited

This is a "no fee, for now" answer, not a permanent architectural constraint. It
should be explicitly revisited — via a follow-up issue, not a silent code change —
if any of the following become true:

- The protocol needs on-chain, enforceable fee collection (e.g. because off-chain
  fee collection proves unenforceable or is bypassed).
- A specific fee model (flat, percentage, or tiered by `Compliance::get_address_tier`,
  see [`../contracts/compliance/README.md`](../contracts/compliance/README.md)) is
  chosen and sponsored by an owner willing to define the fee-recipient trust model.

If a fee is added in the future, the recommended shape — sketched here for
discussion only, **not implemented** — is:

- A `FeeRecipient: Address` and `FeeBps: u32` (basis points) pair in treasury
  instance storage, admin-settable via a dedicated entrypoint (consistent with the
  existing `update_threshold` / `set_signer` admin-only pattern).
- Fee deducted at `execute_settlement` time, computed from the settlement's stored
  `amount`, with the remainder transferred to the merchant and the fee transferred to
  `FeeRecipient` in the same transfer step — so a signer approving a settlement is
  approving a fixed amount whose fee split is deterministic and visible before
  approval, not decided later.
- `FeeBps` bounded (e.g. capped well under 10_000 / 100%) to prevent a compromised or
  malicious admin from setting a confiscatory fee.

This sketch is intentionally not implemented in this change; implementing it is a
separate, scoped follow-up if and when a fee model is actually adopted.
