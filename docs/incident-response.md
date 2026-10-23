# Incident response runbook — emergency pause (settlement-workflow)

This runbook covers the emergency fan-out pause orchestrated by
`contracts/settlement-workflow` (`initialize_emergency_pause`,
`emergency_pause_all`, `resume_all`, `get_emergency_paused_at`).
For deployment ordering and key custody, see
[`docs/deployment-runbook.md`](deployment-runbook.md);
for event names, see [`docs/event-schema.md`](event-schema.md).

Placeholders below follow the deployment runbook:
`$WORKFLOW` = settlement-workflow contract ID,
`$ADMIN` = emergency-pause admin address,
`$NETWORK` = network alias, `$TARGET_*` = paused contract IDs.

## 0. Preconditions — target configuration

Wire the orchestrator once before any incident:

```sh
stellar contract invoke \
  --id $WORKFLOW \
  --source $ADMIN \
  --network $NETWORK \
  -- initialize_emergency_pause \
  --admin $ADMIN \
  --targets '["'$COMPLIANCE_ID'","'$TREASURY_ID'","'$INVOICE_ID'"]'
```

Rules enforced by `validate_pause_targets` (`src/lib.rs`):

- Non-empty, at most `MAX_PAUSE_TARGETS` (8).
- No duplicates, and never the workflow contract itself (`SelfPauseTarget`).
- This contract must already be registered as `admin` on each target,
  because the fan-out calls `pause($WORKFLOW)` on every target.
- Reconfiguration while `get_emergency_paused_at` is `Some` fails with
  `EmergencyPauseActive` — the paused set must match the resumed set.

Verify wiring:

```sh
stellar contract invoke --id $WORKFLOW --network $NETWORK \
  -- get_emergency_pause_targets
stellar contract invoke --id $WORKFLOW --network $NETWORK \
  -- get_emergency_pause_admin
stellar contract invoke --id $WORKFLOW --network $NETWORK \
  -- get_emergency_paused_at
# -> None means no incident is open.
```

Emits `emergency_pause_configured`.

## 1. Trigger — `emergency_pause_all`

One key stops the whole protocol. The sweep is all-or-nothing:
the first target that refuses to pause aborts with `PauseTargetFailed`
and the host reverts the targets already paused.

```sh
stellar contract invoke \
  --id $WORKFLOW \
  --source $ADMIN \
  --network $NETWORK \
  -- emergency_pause_all \
  --admin $ADMIN
```

- Requires `admin.require_auth()` + `require_pause_admin` (`Unauthorized`
  otherwise) and prior configuration (`NotConfigured` otherwise).
- Targets already paused independently are recorded in
  `DataKey::AlreadyPaused` and left paused on resume.
- Emits `emergency_pause_completed` with `(admin, paused_targets)`.

## 2. Verify — the protocol is stopped

`EmergencyPausedAt` is written only after the last target confirms,
so `Some(ts)` means every target is paused and `None` means this
orchestrator paused nothing:

```sh
stellar contract invoke --id $WORKFLOW --network $NETWORK \
  -- get_emergency_paused_at
# -> Some(<ledger timestamp>) while the incident is open.

# Per-target confirmation (repeat for each target):
stellar contract invoke --id $COMPLIANCE_ID --network $NETWORK -- is_paused
stellar contract invoke --id $TREASURY_ID --network $NETWORK -- is_paused
stellar contract invoke --id $INVOICE_ID --network $NETWORK -- is_paused
stellar contract invoke --id $WORKFLOW --network $NETWORK -- is_paused
```

Alert on `emergency_pause_completed`; page on any unexpected per-contract
`*_paused` event without a matching orchestrator sweep (see
[`docs/alerting-guide.md`](alerting-guide.md)).

## 3. Resume — `resume_all` with cooling-off delay (#780)

Resume fans out in reverse order (outside-in) with the same all-or-nothing
discipline: a target that refuses to unpause aborts with
`UnpauseTargetFailed` and already-resumed targets roll back.
Targets in `AlreadyPaused` are skipped — the orchestrator only reverses
its own sweep and can never unpause a protocol somebody else paused
through their own admin key (`NotEmergencyPaused` otherwise).

`min_delay_secs` enforces a minimum cooling-off period measured from
`get_emergency_paused_at`. `0` means no delay (backward compatible):

```sh
# Wait at least $DELAY seconds after the pause timestamp, then:
stellar contract invoke \
  --id $WORKFLOW \
  --source $ADMIN \
  --network $NETWORK \
  -- resume_all \
  --admin $ADMIN \
  --min_delay_secs $DELAY
```

- Calling before `paused_at + min_delay_secs` returns
  `WorkflowError::ResumeTooEarly`; retry after the window elapses.
- Passing `0` proceeds immediately — use only for a confirmed false alarm.
- Emits `emergency_pause_resumed`; `get_emergency_paused_at` returns to
  `None` only on success.

Example: pause landed at `t=1000`, policy requires 15 minutes:

```sh
# paused_at = 1000 (from get_emergency_paused_at)
# earliest resume = 1000 + 900 = 1900 (ledger time)
stellar contract invoke --id $WORKFLOW --source $ADMIN --network $NETWORK \
  -- resume_all --admin $ADMIN --min_delay_secs 900
```

## 4. Failure modes

| Situation | Signal |
|---|---|
| Not wired | `NotConfigured` from any fan-out getter/entrypoint |
| Wrong caller | `Unauthorized` (must match `EmergencyPauseAdmin`) |
| Bad target set | `NoPauseTargets` / `TooManyPauseTargets` / `DuplicatePauseTarget` / `SelfPauseTarget` |
| Target refuses pause | `PauseTargetFailed`, `get_emergency_paused_at` stays `None` |
| Target refuses resume | `UnpauseTargetFailed`, `get_emergency_paused_at` stays `Some` |
| Resume too early | `ResumeTooEarly`, retry after `paused_at + min_delay_secs` |
| Resume with nothing open | `NotEmergencyPaused` |
| Reconfigure mid-incident | `EmergencyPauseActive` |
