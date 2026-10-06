# Campaign 1 Incremental Gameplay State

## Purpose

Expose a bounded step interface for the existing v1 gameplay contract so a live
runtime can submit one player input per fixed tick and inspect each committed
result. The session uses the same movement, ability, NPC, objective, and outcome
rules as `run_replay`; it does not define another gameplay ruleset.

## Session contract

`GameplaySession::new` accepts only a `GameSnapshot` that passes
`validate_snapshot`. The session owns that immutable source snapshot, starts at
tick zero, and selects the same initial playable entity as v1 batch replay.

`GameplaySession::step(expected_tick, input)` accepts exactly one
`InputEvent`. The requested tick must be the state's next tick. A successful
step returns the existing typed `EventReceipt`, including the player
transition, NPC action, resulting state digest, and committed tick. The runtime
state, input history, and receipts are exposed through immutable references.

The session accepts no more than `MAX_REPLAY_EVENTS` inputs. Tick addition and
ability cooldown-ready ticks are checked for `u64` overflow. Each step stages
runtime state and cooldown changes in local copies, applies the player input,
executes the same NPC turn, resolves Win/Loss, and commits only after the full
transition succeeds. A failed tick leaves state, cooldowns, trace, and receipt
history unchanged.

## Shared v1 semantics

Batch replay and incremental sessions call one internal `step_runtime`
transition function. That function enforces:

- playable-entity selection and adjacent navigation movement;
- declared enemy targeting, navigation-step range, energy cost, cooldown, and
  damage/overkill accounting;
- objective location and all-NPCs-defeated prerequisites;
- deterministic NPC target choice, attack, and defeated-NPC behavior;
- objective victory, party defeat, and rejection of inputs after a terminal
  outcome.

`verify_batch_equivalence` reruns the session's accepted trace with
`run_replay` and compares every event receipt and the final state. The batch
receipt remains the v1 receipt format and authority surface.

## Checkpoint and restore

A checkpoint binds the source snapshot digest, v1 trace, inspectable runtime
state, state digest, and checkpoint digest. Cooldown state remains private and
is reconstructed by replaying the trace. Restore validates the source snapshot
and both schema versions, checks the source and content digests, replays every
input through the shared step path, then requires exact equality with the
stored state and checkpoint digest.

These SHA-256 digests detect accidental or stale edits and bind the checkpoint
contents to the source snapshot. They are integrity checks, not signatures:
they do not establish who created a checkpoint or prevent a party from
constructing a different internally consistent checkpoint.

## Explicit scope

This seam provides deterministic stepping for the existing single ability,
single objective, and v1 NPC behavior. It does not add multiplayer, physics,
continuous-time simulation, new abilities, autonomous NPC policies, engine
integration or rigging support. Gameplay semantics remain owned by the
gameplay contract crate; runtime adapters should transport typed inputs and
receipts without reimplementing them.

## Verification controls

The integration suite exercises snapshot validation, exact tick sequencing,
atomic failure, ability targeting/cost/cooldown/damage, NPC turns, objective
prerequisites and completion, both terminal outcomes, tampered/stale
checkpoints, deterministic restore, and exact step-versus-batch receipt
equivalence using the checked-in v1 vertical-slice fixture.
