# Client integration guide

This guide is for anyone building an off-chain service against this protocol
(a merchant dashboard, a payment-processor integration) — not for contract
developers. It covers the external call surface and how to consume it.

## Recommended call sequence for a payment lifecycle

For a typical invoice-to-settlement flow, call the contracts in this order:

1. **`Invoice::create_invoice(merchant, amount, expires_in)`** — creates the
   invoice, status `Pending`.
2. Off-chain payment happens, then an admin calls
   **`Invoice::mark_paid(admin, id, payer)`** — status moves to `Paid`.
3. **`Treasury::propose_settlement(signer, merchant, amount)`** — a Treasury
   signer proposes the settlement.
4. Treasury signers call **`Treasury::approve_settlement(signer,
   settlement_id)`** until the configured signature threshold is met.
5. Execute the settlement **through `SettlementWorkflow`, not directly
   against `Treasury`**:
   `SettlementWorkflow::execute_with_compliance(settlement_id,
   token_contract, merchant)`. `SettlementWorkflow` checks
   `Compliance::is_allowed(merchant)` before calling
   `Treasury::execute_settlement` — Treasury itself does not consult
   Compliance, so calling `Treasury::execute_settlement` directly skips the
   compliance gate entirely. This is why `ARCHITECTURE.md` recommends
   `SettlementWorkflow` as the integration entrypoint for this step.
6. **`Invoice::release_escrow(admin, id)`** — releases escrow once
   settlement has executed, status moves to `Released`.

`SettlementWorkflow::execute_with_compliance_batch` is available for
executing several settlement IDs against one already-checked merchant in a
single call.

## Error handling: `ProtocolError`

`crates/protocol-errors` exposes `ProtocolError`, a single enum
(`ProtocolError::Invoice`, `::Treasury`, `::Compliance`) wrapping each
contract's own error type, with `From` conversions from each. A client that
wants to handle errors from any of the three contracts with one `match` can
depend on this crate instead of matching on each contract's error type
separately. Note this crate currently depends on the contract implementation
crates for their error types (see the related error-unification issues); once
that lands, `ProtocolError` should be resolvable from a deployed contract's
returned error code without depending on the implementation crate at all.

## Watching for state changes

Events are this protocol's primary mechanism for off-chain state
synchronization. Per contract:

- **Invoice** emits `invoice_created`, `invoice_paid`, `invoice_expired`,
  `invoice_cancelled`, `invoice_refund_requested`, and `refund_approved` with
  the full resulting `Invoice`; `escrow_released` carries the invoice ID,
  merchant, amount, and release timestamp.
- **Treasury** and **SettlementWorkflow** emit events on settlement
  proposal/approval/execution and pause/unpause state changes — see each
  contract's source for exact event names until per-contract READMEs land.
- **SettlementWorkflow** additionally emits `settlement_workflow_executed`
  (keyed by settlement ID) specifically so indexers can distinguish
  compliance-gated executions from a direct `Treasury::execute_settlement`
  call.

Consumers must process events in ledger/event order, checkpoint their
position, and deduplicate replayed events — the same requirement documented
for Invoice's audit trail in `ARCHITECTURE.md`. Current state can always be
reconciled by calling read entrypoints directly (e.g. `Invoice::get_invoice`,
`Treasury::get_settlement`) rather than trusting the event log alone.
