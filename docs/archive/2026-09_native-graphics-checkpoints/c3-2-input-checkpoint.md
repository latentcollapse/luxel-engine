# WGE C3.2 Input Seam — Pre-Reboot Checkpoint

Status: **checkpoint green and committed; deliberately parked for the reboot + TetLab integration sprint**
Date: 2026-10-02
Context: user directed a clean stopping point ("easily pick-upable checkpoint") before
rebooting the machine; TetCage/TetLab integration is the first post-reboot sprint, then
Grindstone (`docs/add-ons/grindstone-spec.md`), then the Demo A gauntlet.

## What is landed (all verified green)

**Rust — `native_graphics_contract/src/input_session.rs` (new module):**
- `InputSample` (`wge.input-sample/v1`): device-raw worker report — held GLFW keys
  (positive codes) plus reserved mouse codes `-2..-1`, normalized cursor while focused,
  joystick-1 axes/buttons. Fail-closed validation (finite, bounded, code domain).
- `InputFrame` (`wge.input-frame/v1`): semantic per-tick intent — camera-space
  `move_x/move_y`, look deltas (rad), sprint/roll/guard/light/heavy/interact.
- `ds3_map`: DS3-style Xbox layout. Keyboard: WASD move, Shift sprint, Space roll,
  Ctrl/RMB guard, LMB light, Shift+LMB heavy, E interact. Gamepad: left stick move
  (Y inverted, 0.15 deadzone with rescale), right stick look at 3.4 rad/s, B sprint,
  A roll, X/Y attack, LB guard, RB interact.
- `frame_from_sample`: keyboard + gamepad merge (active stick axes override; resting
  gamepad never cancels keyboard; buttons OR), cursor-displacement look approximation
  (documented; true cursor-lock is a later seam).
- Fixed-tick sim: `velocity_for` (camera-relative → world XZ, sprint 1.8×, unit-norm),
  `PhysicsWorld::step()` receipts at `REFERENCE_TICK_RATE_HZ`; rejected steps consume
  the tick and are recorded — the world rejects, the session adapts.
- `camera_intent`: third-person follow (5.0 m boom, 0.9 m shoulder, pitch clamped
  ±1.2 rad, floor clamp above body height).
- `InputDrivenSession<'world>`: owns sim state + trace over the borrowed validated
  world; `capture_and_promote` delegates through the unchanged authority path.
- Trace: `TraceStep` records + rolling chained digest (`wge.input-trace/v1`) —
  byte-comparable replays. **This is Grindstone's first input buffer.**
- Heading convention (right-handed, Y-up): `forward = (sin yaw, cos yaw)`,
  `right = up × forward`. Do not "simplify" back to cos/sin.

**Rust — `session.rs`:** `present_frames_with_input` adds `report_input:true` to the
worker request, parses typed `input_samples` back (missing/invalid → protocol error);
`present_frames` refactored onto the shared internal path, behavior-identical
(5/5 presented-session integration green).

**Julia — `LavaAdapter.jl`:** `_sample_window_input` (main-thread; FOCUSED-gated keys
+ cursor via `GetWindowAttrib/GetKey/GetMouseButton/GetCursorPos/GetWindowSize`;
`GLFW.JoystickPresent/GetJoystickAxes/GetJoystickButtons` via `GLFW.Joystick(0)`);
per-frame sampling after `PollEvents` when `report_input`; `render_window_frames!`
gained a `report_input::Bool=false` kwarg-position and returns `input_samples`.
**Julia — worker op:** `render_window` accepts `report_input` (Bool-validated) and
merges `input_samples` into the response only when requested.

**Gates at commit time:** lib 26/26 (incl. 7 new input_session tests) ·
`--test session` 5/5 · Julia `Pkg.test(test_args=["lava_adapter"])` exit 0
(FR-0014 test target still in place) · repo clean on `main`.

## Honest gaps (what is NOT done — three named follow-ups)

1. **The drive loop is not refactored yet.** `InputDrivenSession::sample_and_present`
   currently advances the sim from `InputSample`s the CALLER supplies and presents via
   plain `present_frames` (worker-side sampling not yet consumed end-to-end); it does
   not yet call `present_frames_with_input`, sample between its own batches, or replay
   the last sample for frames without a fresh one. Refactor it to loop:
   `present_frames_with_input` → apply one `InputFrame` per returned sample (worker
   samples are per-FRAME, so ticks == samples in steady state) → re-aim camera → next
   batch. Keep the `expected_tick` guard; the sim already rejects stale frames.
2. **Worker input-collection has no dedicated test** (existing suites stay green with
   the flag off). Add a worker-op test: `report_input:true` with `frame_count:N` →
   response carries `N` samples, each with `schema_version == "wge.input-sample/v1"`
   and frame-matched `timestamp_ms` monotonicity. Headless CI cannot press keys —
   assert on structure and count, not key state.
3. **No Rust integration test binds the loop over a real world.** Add
   `tests/input_session.rs`: session over Riverwatch → synthesize `InputSample`s →
   `apply_frame` advances position along terrain / gets rejected at obstacles →
   `capture_and_promote` still green with input active → trace digest is stable across
   a replayed sequence and differs when a frame differs.

Known cosmetic gap: a sample's input is applied at batch granularity (per-frame sim
ticks within a batch reuse that batch's frame mapping); acceptable for the checkpoint,
collapse when follow-up 1 lands.

## Next session order (frozen by the user)

1. **Reboot.** Grok is archiving `/tmp` into a developer archive on disk; nothing
   load-bearing lives in `/tmp` for this work (all evidence above is in-repo).
2. **tet-lab/TetCage integration sprint** (user-authorized; TetLab still quarantined at
   `graphics_lab/tet-lab/`). Entry point: `tetcage-fusion-headroom.md` (graph-fused
   skin ≈ 9–11 µs, ~2.9× instance capacity) and the A/B doctrine in
   `docs/archive/2026-09_roadmaps-and-audits/pre-demo-audit.md` §2 DEFERRED — ARENA BASELINE vs ARENA+TetCageRT, publish
   metrics, spend the savings on density.
3. Finish C3.2 via the three follow-ups above, then Demo A v1 gauntlet
   (`docs/archive/2026-09_roadmaps-and-audits/pre-demo-audit.md` §4 fix order), then Grindstone MVP
   (`docs/add-ons/grindstone-spec.md` §7).

## User requirements registered this session (all frozen in the audit doc)

DS3-style Xbox controller layout (mapping implemented) · Elden Ring automatic
powerstancing (gameplay-contract pairing rule; decide before weapon data composition) ·
Roundtable-style hub + training dummy + duel queue (Demo A v1) · battle royale 100-AI
as Phase 4 local-sim showcase · "frictionless: controller just connects when the game
window comes up" — the C3.2 sampling path is that seam: worker samples joystick-1
automatically every frame, no user configuration, mapping is engine-side · Grindstone
spec registered as the post-TetCage subsystem.
