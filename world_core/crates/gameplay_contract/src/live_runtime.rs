//! Incremental v1 gameplay sessions backed by the batch replay transition path.
//!
//! A session owns a validated snapshot, its input trace, runtime state, and
//! derived cooldown state. Each accepted input is one atomic fixed tick.

use serde::{Deserialize, Serialize};

use crate::runtime::{self, CooldownReadyAt};
use crate::{
    EventReceipt, FailureCode, GAMEPLAY_SESSION_SNAPSHOT_SCHEMA, GAMEPLAY_TRACE_SCHEMA,
    GameFailure, GameSnapshot, GameplayReceipt, InputEvent, MAX_REPLAY_EVENTS, ReplayTrace,
    RuntimeState, run_replay, validate_snapshot,
};

/// A mutable deterministic play session over one validated v1 game snapshot.
#[derive(Clone, Debug)]
pub struct GameplaySession {
    game_snapshot: GameSnapshot,
    game_snapshot_sha256: String,
    trace: ReplayTrace,
    state: RuntimeState,
    cooldown_ready_at: CooldownReadyAt,
    receipts: Vec<EventReceipt>,
}

impl GameplaySession {
    /// Start from a snapshot that passes the existing v1 semantic validator.
    pub fn new(game_snapshot: &GameSnapshot) -> Result<Self, GameFailure> {
        validate_snapshot(game_snapshot)?;
        Ok(Self {
            game_snapshot: game_snapshot.clone(),
            game_snapshot_sha256: runtime::sha256_json(game_snapshot)?,
            trace: ReplayTrace {
                schema_version: GAMEPLAY_TRACE_SCHEMA.to_owned(),
                events: Vec::new(),
            },
            state: runtime::initial_state(game_snapshot)?,
            cooldown_ready_at: CooldownReadyAt::new(),
            receipts: Vec::new(),
        })
    }

    /// Current immutable runtime state. Mutations are available only through
    /// `step`, which checks the next tick and commits only complete transitions.
    pub fn state(&self) -> &RuntimeState {
        &self.state
    }

    /// Accepted v1 input history in replay order.
    pub fn trace(&self) -> &ReplayTrace {
        &self.trace
    }

    /// Receipts emitted for accepted inputs in replay order.
    pub fn receipts(&self) -> &[EventReceipt] {
        &self.receipts
    }

    /// Return the only tick that can currently be accepted.
    pub fn next_tick(&self) -> Result<u64, GameFailure> {
        self.state.tick.checked_add(1).ok_or_else(|| {
            GameFailure::new(FailureCode::TickOverflow, "runtime tick counter overflowed")
        })
    }

    /// Apply one input at its exact next tick using v1 movement, ability,
    /// objective, NPC response, and terminal-outcome rules.
    pub fn step(
        &mut self,
        expected_tick: u64,
        input: InputEvent,
    ) -> Result<EventReceipt, GameFailure> {
        let event_index = self.trace.events.len();
        if event_index >= MAX_REPLAY_EVENTS {
            return Err(GameFailure::new(
                FailureCode::TraceTooLong,
                format!("session has reached the maximum of {MAX_REPLAY_EVENTS} accepted inputs"),
            )
            .at_event(event_index));
        }

        let (next_state, next_cooldowns, receipt) = runtime::step_runtime(
            &self.game_snapshot,
            &self.state,
            &self.cooldown_ready_at,
            &input,
            expected_tick,
            event_index,
        )?;

        // Commit only after the full shared runtime transition has succeeded.
        self.trace.events.push(input);
        self.receipts.push(receipt.clone());
        self.state = next_state;
        self.cooldown_ready_at = next_cooldowns;
        Ok(receipt)
    }

    /// Create a canonical checkpoint. Runtime state is included for inspection,
    /// then independently reconstructed from the trace during restoration.
    pub fn checkpoint(&self) -> Result<GameplaySessionSnapshot, GameFailure> {
        let body = GameplaySessionSnapshotBody {
            schema_version: GAMEPLAY_SESSION_SNAPSHOT_SCHEMA.to_owned(),
            game_snapshot_sha256: self.game_snapshot_sha256.clone(),
            trace: self.trace.clone(),
            state_sha256: runtime::sha256_json(&self.state)?,
            state: self.state.clone(),
        };
        let snapshot_sha256 = runtime::sha256_json(&body)?;
        Ok(GameplaySessionSnapshot {
            body,
            snapshot_sha256,
        })
    }

    /// Restore by checking checkpoint and source-snapshot bindings, replaying
    /// every accepted input through the shared v1 transition function, and
    /// comparing the reconstructed state and canonical checkpoint digest.
    pub fn restore(
        game_snapshot: &GameSnapshot,
        checkpoint: &GameplaySessionSnapshot,
    ) -> Result<Self, GameFailure> {
        validate_snapshot(game_snapshot)?;
        if checkpoint.body.schema_version != GAMEPLAY_SESSION_SNAPSHOT_SCHEMA {
            return Err(GameFailure::new(
                FailureCode::UnsupportedSchema,
                format!(
                    "session snapshot schema {:?} is not supported; expected {:?}",
                    checkpoint.body.schema_version, GAMEPLAY_SESSION_SNAPSHOT_SCHEMA
                ),
            ));
        }
        if checkpoint.body.trace.schema_version != GAMEPLAY_TRACE_SCHEMA {
            return Err(GameFailure::new(
                FailureCode::UnsupportedSchema,
                format!(
                    "checkpoint trace schema {:?} is not supported; expected {:?}",
                    checkpoint.body.trace.schema_version, GAMEPLAY_TRACE_SCHEMA
                ),
            ));
        }
        if checkpoint.body.trace.events.len() > MAX_REPLAY_EVENTS {
            return Err(GameFailure::new(
                FailureCode::TraceTooLong,
                format!(
                    "checkpoint has {} inputs; maximum is {MAX_REPLAY_EVENTS}",
                    checkpoint.body.trace.events.len()
                ),
            ));
        }

        let actual_game_snapshot_sha256 = runtime::sha256_json(game_snapshot)?;
        if checkpoint.body.game_snapshot_sha256 != actual_game_snapshot_sha256 {
            return Err(GameFailure::subject(
                FailureCode::ReplayDiverged,
                "GameSnapshotSha256",
                "checkpoint was created from a different game snapshot",
            ));
        }
        let actual_state_sha256 = runtime::sha256_json(&checkpoint.body.state)?;
        if checkpoint.body.state_sha256 != actual_state_sha256 {
            return Err(GameFailure::subject(
                FailureCode::ReplayDiverged,
                "SessionStateSha256",
                "checkpoint state does not match its declared state digest",
            ));
        }
        let actual_checkpoint_sha256 = runtime::sha256_json(&checkpoint.body)?;
        if checkpoint.snapshot_sha256 != actual_checkpoint_sha256 {
            return Err(GameFailure::subject(
                FailureCode::ReplayDiverged,
                "SessionSnapshotSha256",
                "checkpoint body does not match its canonical digest",
            ));
        }

        let mut restored = Self::new(game_snapshot)?;
        for (event_index, input) in checkpoint.body.trace.events.iter().enumerate() {
            let next_tick = restored
                .next_tick()
                .map_err(|failure| failure.at_event(event_index))?;
            restored.step(next_tick, input.clone())?;
        }

        let reconstructed = restored.checkpoint()?;
        if reconstructed.body != checkpoint.body
            || reconstructed.snapshot_sha256 != checkpoint.snapshot_sha256
        {
            return Err(GameFailure::subject(
                FailureCode::ReplayDiverged,
                "ReconstructedSession",
                "deterministic replay did not reproduce the checkpoint state and digest",
            ));
        }
        Ok(restored)
    }

    /// Re-run the accepted trace through `run_replay` and verify the stepped
    /// receipts and final state match the established batch contract exactly.
    pub fn verify_batch_equivalence(&self) -> Result<GameplayReceipt, GameFailure> {
        let batch = run_replay(&self.game_snapshot, &self.trace)?;
        if batch.body.events != self.receipts || batch.body.final_state != self.state {
            return Err(GameFailure::subject(
                FailureCode::ReplayDiverged,
                "StepVsBatch",
                "incremental session differs from the v1 batch replay result",
            ));
        }
        Ok(batch)
    }
}

/// Digest-bound, inspectable state saved at an incremental gameplay boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameplaySessionSnapshot {
    pub body: GameplaySessionSnapshotBody,
    pub snapshot_sha256: String,
}

impl GameplaySessionSnapshot {
    /// Stable JSON serialization for storage, comparison, and transport.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, GameFailure> {
        serde_json::to_vec(self).map_err(|error| {
            GameFailure::new(
                FailureCode::ReplayDiverged,
                format!("session snapshot serialization failed: {error}"),
            )
        })
    }
}

/// Canonical fields covered by [`GameplaySessionSnapshot::snapshot_sha256`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameplaySessionSnapshotBody {
    pub schema_version: String,
    pub game_snapshot_sha256: String,
    pub trace: ReplayTrace,
    pub state_sha256: String,
    pub state: RuntimeState,
}
