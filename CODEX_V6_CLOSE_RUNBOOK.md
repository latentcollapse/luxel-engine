# Adapter-v6 Audit Close Runbook

Status: read-only companion to the adapter-v6 adversarial audit; written by a second review
session, 2026-09-29
Scope: independent review findings on the uncommitted v6 diff, plus the ordered steps to close
the audit. This document modifies nothing and runs nothing. The owning session (Codex) verifies,
dismisses, or acts on each item.

## Why this exists

A second read-only session reviewed the full v6 diff while the owning session was running the
decisive integration pass. Read-only means: no commands, no tracked-file edits, no concurrent
cargo/Julia/GPU work — the integration suite owns the Vulkan device and the cargo target
directory while it runs. Findings below are anchors and reasoning, not patches. Every finding is
phrased as verify-or-dismiss because the focused suites (Julia 26/26, Rust 16/16, fmt, clippy)
were already green at review time; several items may be fully covered by the 34 new
projection/overlay assertions.

## Independent review findings

Findings the review believes are **correct** (verified by reading, stated so the owning session
does not re-derive them):

- **Sphere-vs-frustum culling math.** `_sphere_visible`'s perspective branch is the standard
  sphere-to-plane test: the boundary `horizontal − forward·tanθ ≤ r·sqrt(1+tan²θ)` is exact for
  a sphere against a half-angle plane, and the orthographic branch is the correct padded
  half-extent test. This assumes `frame.projection[1]/[2]` carry tan(half-angle), consistent
  with both `_project_world` and `_project_clip`.
- **Cache keys cannot go stale on camera.** `MeshResources`/shadow caches key on
  `packet.content_sha256`, and the camera is part of the packet body — so per-packet caching is
  sound, including the camera-cut profiles (they are separate packets with separate digests).
- **Dense-benchmark authorization is exact.** `validate_authorized_world_projection` requires
  byte-equality with the canonical dense derivation, so a forged packet that merely matches the
  reference instance count plus arbitrary additions fails the `any(candidate == packet)` check.
- **Telemetry balancing is tight.** Per-class visible/culled totals are reconciled against the
  packet's importance histogram and against total visibility; terrain vertex count is
  recomputed from resolution. Producer telemetry cannot drift decoratively.
- **Fail-closed dispatch.** Unknown importance types throw `unsupported_importance`; the
  deterministic `_stable_tangent` fallback preserves byte-determinism on degenerate UV
  determinants instead of skipping.

Findings requiring owning-session disposition:

- **F1 — first-frame timeout margin (verify).** `DEFAULT_WORKER_RESPONSE_TIMEOUT` is 120 s;
  the historical cold CLI sample was 116.17 s and the cold promoted frame paid 14.2 s of lazy
  device/pipeline work inside a frame. Confirm which deadline governs the first frame after a
  restart (startup handshake budget vs frame response deadline), and whether the ready handshake
  pre-warms device/pipeline state. On a slower host, cold-init-inside-first-frame could trip a
  120 s frame deadline and produce a `worker_timeout` that looks like a stall rather than slow
  startup. If the handshake budget does not cover lazy init, either pre-warm in the handshake or
  document the intended budget split in the handoff doc.
- **F2 — projection convention documentation (verify/comment).** The culling and projection
  paths all assume `projection[1]/[2]` = tan(half-angle) and `projection[3]/[4]` = near/far.
  The 34 new assertions presumably pin this; if any ambiguity remains, one comment on
  `CameraFrame` stating the convention prevents a future misuse that no compiler will catch.
- **F3 — conservative instance bounds (info).** `_instance_world_bound` uses
  `max(scale...) * local_radius`, which over-retains under non-uniform scale. Correct and
  standard for v6; revisit only when LOD/density work lands, because over-retention shows up as
  culling-telemetry expectations in tests.
- **F4 — shadow visibility default argument (verify).** `_mesh_resources!(...; shadow::Bool)`
  defaults `visibility` to the camera-frame visibility. Confirm both shadow call sites pass
  light-frustum visibility explicitly; a shadow caller relying on the default would cull shadow
  casters by camera instead of light. One grep confirms or dismisses.
- **F5 — exact-equality measurement agreement (info).** `measurements_agree` requires exact
  distinct-color and pixel-count equality with a 0.01 luminance tolerance. Deterministic given
  identical bytes; any future change to producer-side encoding (readback padding, row order)
  trips equality before the tolerance matters. Fine now; noted so a future failure is
  attributed quickly.
- **F6 — authorization cost in the future live path (info, forward-looking).**
  `validate_authorized_world_projection` lowers up to five full packets per validation. Correct
  for snapshot cadence; Campaign 1's live-loop design must cache the authorized set per world
  artifact rather than re-lowering per sample.
- **F7 — octagonal marker discs (info).** `_append_overlay_disc!` builds an 8-segment fan.
  Cosmetically fine at marker sizes; if close-up marker inspection ever becomes a profile,
  raise segment count behind a typed, deterministic capture parameter rather than in-place.
- **F8 — teardown semantics (no action).** `stop_child` kills without draining transport.
  Correct for fail-closed; the restart-replay evidence already exercises clean kill-then-restart.

## Owning-session disposition

- **F1 — resolved/documented.** The ready handshake validates source and adapter identity but
  deliberately does not initialize Vulkan. `capabilities()` is mandatory before promotion and
  receives the same bounded 120-second response deadline; the first promoted frame may pay lazy
  scene-pipeline work inside that same frame deadline. The handoff now states this explicitly.
  This is a bounded cold-start risk to measure on slower hosts, not a pass-shaped timeout or an
  authority defect in the current checkpoint.
- **F2 — resolved.** `CameraFrame` now documents the orthographic/perspective projection slots;
  the 34 projection and overlay assertions cover the convention.
- **F3 — accepted informationally.** Conservative non-uniform-scale bounds are correct for v6;
  tighter bounds belong with future LOD/density work.
- **F4 — dismissed.** Shadow resource call sites pass light-frustum visibility explicitly.
- **F5 — accepted informationally.** Exact byte-derived measurement agreement is intentional.
- **F6 — deferred as a live-path optimization.** Snapshot promotion is correct today; a live loop
  should cache the authorized projection set per world artifact.
- **F7 — accepted informationally.** Eight-segment marker discs are deterministic and adequate
  for the current capture profiles.
- **F8 — no action.** Fail-closed kill/restart semantics are intentional and exercised.

The decisive seven-test native integration suite passed 7/7. Fresh overview, close, material
showcase, and composed-world evidence was regenerated and recorded in the handoff/benchmark
reports. Rust unit tests (16/16), Julia protocol tests (14/14), Julia adapter tests (26/26 plus
34 projection/overlay assertions), formatting, linting, and `git diff --check` are green.

## Audit-close checklist (ordered)

1. **Decisive check — complete.** The seven-test native integration suite passed 7/7; the
   original promotion failure is closed by the disc repair at zero gate relaxation.
2. **Fresh evidence — complete.** Current v6 packet/capture digests for overview, close,
   material showcase, and world showcase are in the handoff and showcase reports.
3. **Benchmarks — complete.** The render and 30-frame/dense profiles were rerun; current v6
   distributions are beside the historical v4/v5 controls, with the wall-time attribution gap
   called out rather than hidden.
4. **Determinism — complete.** Fresh-process and warm replay digests are recorded.
5. **Status headers — complete.** The architecture, audit, handoff, benchmark, showcase, and
   capability reports carry the closed 2026-09-29 status.
6. **Commit — intentionally deferred.** The owning user did not request a commit; the verified
   checkpoint remains available as an uncommitted working tree.
7. **Explicitly deferred.** The root-README doctrine correction remains tracked in
   `WGE_GENERALITY_BRIDGE.md` and is outside this audit close.

### Minimal-viable close (if session budget runs out)

If budget expires mid-close, the minimal honest close is: (1) seven-test suite green,
(2) status line in the handoff doc, (3) fresh digests recorded for the canonical overview.
Items 2–5 can follow in a dedicated doc-refresh commit. Never close with stale digests silently
left labeled as current.

## Parallel-session protocol (for future second sessions)

- Read-only review sessions run no `cargo`, no `julia`, no GPU-touching commands while the
  owning session may be running them. Device contention invalidates timing evidence and risks
  false validation failures on the owning session's run.
- No tracked-file edits while another session holds an uncommitted checkpoint. New untracked
  companion documents are the coordination medium.
- Findings are relayed through the user or a companion doc, never patched in place; the owning
  session owns the code and the commit.
