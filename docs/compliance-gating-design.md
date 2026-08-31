# Compliance gating design rationale

> Status: design rationale only. This document does not propose a behavior change; it explains why the current on-chain compliance gate is the intended model for this protocol and how it differs from plausible alternatives.

## 1. The protocol’s current model

The current compliance design is intentionally explicit and on-chain:

- `Compliance::is_allowed(address)` is the authoritative gate used by the settlement flow before execution proceeds.
- The underlying decision is stored directly in contract state under `Allowed(Address)`, `Blocked(Address)`, and time-bound variants such as `AllowedUntil(Address)` and `BlockedUntil(Address)`.
- A contract that needs compliance status does not ask an off-chain system or a separate oracle. It calls the compliance contract and receives a deterministic boolean result.
- This makes compliance part of the on-chain settlement decision, not just a policy document or an external operational step.

The resulting design is simple but materially important: every participant can observe the same rule, and every enforcement point evaluates the same state. In this protocol, the gate is not merely a notification channel. It is a hard condition enforced by the contract system itself.

## 2. Why the protocol chose this form

The design is built around four practical requirements:

1. Deterministic enforcement
   - Settlement and payment flows must behave the same way regardless of which off-chain operator is calling them.
   - On-chain state makes the result verifiable and reproducible by any observer or auditor.

2. Default-deny safety
   - If an address is not explicitly allowed, it is denied by default.
   - This makes the system conservative and easier to reason about under operational failure or incomplete data.

3. Auditability and operator accountability
   - A compliant address decision has a direct on-chain provenance trail: allow, block, clear, expiry, and tier metadata are all recorded in the compliance contract.
   - This is materially easier to inspect later than a model that depends on a private or external data source.

4. Emergency control without ambiguity
   - The design preserves a clear administrative authority model: admin mutations are explicit, while read-only checks remain permissionless.
   - The contract can still block addresses while paused for emergency remediation, while allowing reads to continue, which keeps the system responsive in crisis conditions without making the security model fuzzy.

This is not accidental. It is the actual operational philosophy encoded in the contract: an explicit, admin-managed set of allow/block decisions under protocol control.

## 3. Comparison to common alternatives

| Pattern | How it works | Strengths | Why it is not the chosen design here |
|---|---|---|---|
| Client-side or off-chain allowlist only | The app, backend, or integration layer checks a list before initiating a flow, but the chain itself does not enforce the result. | Fast to implement; simple operational workflow; low on-chain complexity. | It does not enforce the policy at the protocol boundary. If a client or integration layer is bypassed, the chain still accepts a transaction that should have been blocked. |
| Oracle-based external sanctions feed | A trusted external feed or oracle pushes sanctions updates into the protocol, often through a separate contract or signed data feed. | Can ingest real-world sanctions data at scale; more dynamic than manual admin updates; potentially better for external watchlists. | Increases trust assumptions about the feed source, provenance, and freshness. It also broadens the system boundary beyond the protocol’s own state machine. |
| Zero-knowledge attestation | A user proves compliance via a zk proof or attestations without exposing the underlying determination itself on-chain. | Strong privacy properties; useful where sensitive compliance data should remain private. | Requires a different trust and infrastructure model: proving keys, attestations, and verification logic. It changes the semantics from “the protocol stores the compliance status” to “the protocol verifies a proof about the status.” |
| This protocol’s current model | The admin explicitly sets allow/block state in contract storage; `is_allowed` reads that state and enforcement is automatic for all workflow participants. | Deterministic, auditable, easy to reason about, and enforceable by contract logic itself. | Requires operational discipline and admin attention; it does not magically ingest external sanctions data without an explicit on-chain update. |

## 4. Alternative 1: Off-chain allowlist only

A purely off-chain allowlist is the simplest alternative to imagine. A backend or client says, “only addresses on this list may proceed,” and then the application guards its own actions accordingly.

This pattern is useful for business logic, but it is not a protocol-enforced compliance gate. The critical limitation is that the chain does not know the decision. The enforcement point is outside the smart contract boundary, and any bypass path is outside the protocol’s security model.

That is not acceptable for a protocol component that is meant to mediate settlement and value movement. If the settlement workflow is intended to be the protocol’s authority on who can be paid, then the compliance outcome has to live in a place that the workflow itself cannot bypass.

The current design intentionally rejects that model. The compliance decision is not an advisory rule for one integration. It is a protocol-wide truth source that the protocol can enforce.

## 5. Alternative 2: Oracle-fed sanctions model

An oracle-fed model is a legitimate design for ingesting external sanctions data. It is especially attractive when the goal is to react to a live sanctions list maintained by an external authority. The issue discussed in the repository’s sanctions-list design notes is precisely this kind of problem: a real-world source of truth exists outside the protocol, and the protocol must decide how to incorporate it without making the trust model too complex or too brittle.

The oracle pattern has real strengths:

- It can react to changes in external lists without requiring a human admin to notice and update every address manually.
- It can scale better than a purely manual allow/block process for large, rapidly changing watchlists.
- It can keep a clean separation between “data source” and “protocol policy enforcement.”

However, it also introduces different trust assumptions:

- The protocol must trust the oracle or data source to provide accurate, timely, and correctly authenticated information.
- The protocol must define what happens when that source is stale, compromised, delayed, or false-positive-prone.
- The protocol must decide whether the feed is a single source of truth or a multisig/committee-based attestor set.

This is a separate design problem from the current compliance gate. The current repository is not set up as an oracle-rich environment. It is built around explicit on-chain admin-managed state that is straightforward to audit and review in place. The current model is therefore intentionally conservative: the protocol authorizes lawful admin-controlled state transitions and enforces them unambiguously on-chain, without depending on an external feed being available and trusted for every compliance decision.

In other words, the oracle model may be a valuable extension to the compliance system in a future sanctions-integration design, but it is not the protocol’s base primitive today. The base primitive is explicit protocol state.

## 6. Alternative 3: Zero-knowledge attestation

A zk-attestation design is conceptually elegant for privacy-sensitive compliance. A user may prove that they are compliant under some set of rules without revealing the underlying data or the reason for the determination.

This pattern is attractive where the primary requirement is privacy, such as a user wanting to prove KYC status or a sanctions check without broadcasting all underlying facts on-chain. It can protect sensitive personal data and minimize disclosure.

But it changes the architecture in a different way:

- The protocol is no longer simply checking an on-chain status. It is verifying a cryptographic proof about a status that may not be visible in the same way to all observers.
- The design requires a proving system, verification logic, and trusted setup assumptions that do not currently exist in this repo’s core payment architecture.
- It is a different design goal than this protocol’s current one: current compliance is a transparent, protocol-owned gate that can be reviewed and reasoned about directly from the chain state.

For a public protocol that must be externally auditable and whose enforcement must be understandable without specialized privacy infrastructure, this is not the chosen baseline. The current model favors transparency and explicitness over private proof-based compliance.

## 7. Why the current model remains the right fit here

The chosen pattern is best aligned with this protocol for several reasons:

- It is a public, on-chain system with no hidden compliance backend.
- The actual compliance determination is visible in storage and can be inspected by auditors and operators.
- The enforcement point is integrated directly with the protocol’s settlement flow, so there is no “soft” or separate compliance layer that could be skipped.
- The contract expresses a clear policy: allow and block are explicit state transitions, while the read path is deterministic and permissionless.
- The semantics are easy to understand: block wins over allow, expiry is handled in a defined order, and reads are stable and reproducible.

This is especially important for external auditors and maintainers. A system that stores the compliance result directly on-chain is easier to review, easier to explain, and easier to defend as a design choice when the system is audited or re-evaluated under stress.

## 8. Practical trade-off

The current design is not “the only possible compliance model.” It is the model that fits this protocol’s constraints and priorities.

Its primary trade-off is operational simplicity versus dynamic external data integration:

- It is very strong for protocol-enforced correctness and auditability.
- It is less automatic when real-world lists are changing outside the protocol.
- It depends on explicit admin actions to keep the allow/block state in sync with external events.

That trade-off is deliberate. The protocol prefers a direct, transparent, and enforceable on-chain state machine over a more complex model that relies on external trust assumptions or privacy-preserving proof infrastructure.

## 9. Conclusion

The current compliance gate is intentionally not a client-only filter and not a “hidden” compliance layer. It is an on-chain compliance decision enforced directly by the protocol.

Compared with:

- off-chain allowlist-only checks, which are not enforced on-chain,
- oracle-fed sanctions models, which add external trust assumptions,
- zero-knowledge attestation models, which optimize for privacy rather than transparent protocol governance,

this design is the most coherent fit for a public settlement protocol that values direct on-chain enforcement, explicit admin authority, and auditor-friendly state transitions.

The result is a design that is straightforward to audit, conservative by default, and easy to reason about as part of the protocol’s overall security model. That is the core rationale for the current compliance-gating choice.
