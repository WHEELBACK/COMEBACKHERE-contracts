# Grace-window clock behavior test — what was implemented

Issue: `mark_paid`'s grace-window boundary logic (#12) was property-tested
against a monotonically increasing ledger timestamp. Confirm the logic's
actual behavior when the ledger timestamp jumps backward between
transactions, or stays flat across multiple transactions, using
soroban-sdk's `Env` testutils to set `env.ledger().timestamp()` directly.

## File added

`contracts/invoice/tests/grace_window_clock_test.rs`

## What it does

Four tests, all setting the ledger timestamp explicitly via
`env.ledger().with_mut(|l| l.timestamp = ...)` rather than relying on
elapsed wall-clock time:

- `mark_paid_succeeds_when_ledger_timestamp_is_earlier_than_at_creation` —
  invoice created at t=1000, `mark_paid` called at t=500 (earlier than
  creation time but still before `expires_at`). Succeeds, since the check
  is a pure function of the current timestamp, not of elapsed time since
  creation.
- `mark_paid_rejected_then_backward_clock_jump_makes_it_payable_again` —
  an invoice correctly rejected as `Expired` at t=1200 becomes payable
  again once the ledger timestamp moves backward to t=900. Documents the
  "extending an effective payment window" scenario named in the issue as
  observed behavior (not a bug assertion — the check has no monotonicity
  guard and real Stellar timestamps aren't expected to move backward).
- `mark_paid_succeeds_for_multiple_invoices_at_an_identical_flat_timestamp`
  — five invoices created and paid at the exact same timestamp, with no
  advance between any transaction, all succeed independently.
- `mark_paid_rejects_multiple_invoices_at_an_identical_flat_expiry_boundary`
  — five invoices, all evaluated at the exact expiry boundary with a flat
  timestamp, are all rejected identically and leave state as `Pending`.

## Finding

`mark_paid`'s grace-window check (`timestamp >= expires_at + grace_window`
in `contracts/invoice/src/entrypoints/lifecycle.rs`) reads
`env.ledger().timestamp()` fresh on every call and makes no monotonicity
assumption. A flat timestamp behaves exactly like the boundary tests in
`invoice_grace_window_test.rs` predict. A backward-jumping timestamp is not
rejected or specially handled — an already-expired invoice can become
payable again if the ledger clock subsequently reports an earlier time.
This is a documented property, not a fix; flagged here since Stellar
ledger timestamps are not expected to regress in production, so no
production impact is asserted.

## Not run

Per the task instructions, this change was written without running
`cargo test` / `cargo clippy` / `cargo fmt` locally. Those checks are listed
as outstanding follow-up in the PR description.
