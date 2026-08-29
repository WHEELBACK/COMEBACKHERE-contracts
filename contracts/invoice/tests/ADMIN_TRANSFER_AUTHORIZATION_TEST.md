# Invoice admin-transfer authorization test coverage

## What was implemented

`contracts/invoice/tests/admin_transfer_authorization_test.rs` adds functional
test coverage for invoice's two-step admin transfer flow
(`transfer_admin` / `accept_admin`, defined in
`contracts/invoice/src/entrypoints/admin.rs:46-72`).

## Why

`#55` added a test confirming "only PendingAdmin can call accept_admin", but
before this change, `contracts/invoice/tests/invoice_test.rs` only referenced
`accept_admin`/`transfer_admin` as string literals inside a
`expected_entrypoints` `HashSet` membership check (verifying the entrypoint
exists in the WASM export list) — there was no test that actually invoked
`transfer_admin`/`accept_admin` and asserted on authorization behavior.
Compliance's equivalent two-step flow already had this kind of coverage;
invoice's did not. Since the entire point of a two-step transfer is to
prevent a transfer to an unreachable or mistyped address from silently
taking effect, a gap here left exactly that failure mode untested.

## What the new test file covers

- `accept_admin` fails with `NoPendingAdmin` when no transfer was ever
  initiated.
- Only the currently-nominated pending admin can successfully call
  `accept_admin`; any other caller (even with a valid signature of their
  own) is rejected with `Unauthorized`, and admin rights do not move as a
  side effect of the rejected call.
- A successful `accept_admin` call grants the new admin admin-gated
  capabilities and revokes them from the old admin.
- `PendingAdmin` is cleared on acceptance, so a stale/second `accept_admin`
  call fails with `NoPendingAdmin` rather than silently re-succeeding.
- `transfer_admin` can be called again before the first nominee accepts,
  changing who the pending admin is; the superseded nominee can no longer
  accept.
- `transfer_admin` itself requires the *current* admin's authorization
  (rejects a non-admin caller).
- `accept_admin` requires the new admin's own authorization (rejects a call
  made without the new admin's signature present in the auth set).

## Result

No duplicate coverage existed prior to this change (confirmed by reading
`contracts/invoice/tests/` before writing new tests, per the parent issue's
instructions) — this file closes a genuine gap rather than adding redundant
tests.
