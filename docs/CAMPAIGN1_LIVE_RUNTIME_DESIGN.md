# Campaign 1 Incremental Reference Runtime

Date: 2026-09-29
Status: implemented in the engine-neutral reference runtime; supervisor integration is pending.

## Scope

The reference runtime now exposes a deterministic incremental traversal session over a validated `WorldArtifact`. The session owns the world snapshot used at initialization, so every step and restored snapshot remains bound to one immutable world identity.

This slice implements spatial traversal only. It does not claim to run the `GeneralGameSpec` ability/combat/objective state machine interactively. That contract currently accepts complete discrete traces through `run_general_replay`; adding a live gameplay step would require a separately specified input cadence, NPC/effect ordering, and terminal-state policy. Campaign 1 can therefore exercise a moving traversal session, while gameplay simulation remains batch replay until those semantics are specified.

## API

`TraversalSession::initialize(&world)` validates the complete artifact and starts at the declared player spawn at tick zero. `advance(expected_tick, destination_cell)` accepts one spatial input. Ticks must increase by exactly one; the destination must be collision-clear, orthogonally adjacent, and within the authored maximum grade. Encounter entry is triggered with the existing radius rule, and objective entry is recorded once. Invalid moves leave the session unchanged.

The session allows legal movement away from the precomputed navigation route. Completion is `Completed` only when the objective has been reached and every required encounter has been visited; otherwise `finish()` returns an `Incomplete` terminal result. A finished session rejects more steps and a second completion request.

`snapshot()` returns a closed typed record bound to world ID and digest, all traversal steps, visited encounters, objective state, and finish state. Its `state_sha256` seals the canonical JSON body. `restore(&world, &snapshot)` checks that digest and world identity, replays every stored step through the same movement rules, and compares the replayed state with the supplied snapshot. The history is capped at `MAX_TRAVERSAL_SESSION_STEPS` (257²).

`finish()` returns a `TraversalSessionCompletion` bound to the terminal snapshot digest. `validate_traversal_session_completion(world, snapshot, completion)` replays the finished snapshot and independently recomputes the terminal summary.

The existing `run_playthrough` now drives the same session transition helpers along the authored navigation route. It retains its established evidence schema, event ordering, and terminal-only objective event behavior. Interactive sessions emit a first-entry objective event. Both paths share cell-clearance, adjacency, slope, encounter, and state-digest machinery.

## Verification expectations

The focused integration suite covers deterministic replay and equivalence with batch route evidence, continuation after snapshot restore, stale and duplicate tick rejection without mutation, non-adjacent movement, collision and slope rejection, digest tampering, and replay rejection of a re-sealed divergent history.

The runtime API is ready for a Rust live supervisor to own its session and publish snapshots/tick digests. It does not provide a window, presentation acknowledgment, input queue, or a live connection to the standalone Tier A/Tier B evidence contract. Those remain integration work; this API supplies the deterministic spatial state that such a supervisor can bind to each frame.
