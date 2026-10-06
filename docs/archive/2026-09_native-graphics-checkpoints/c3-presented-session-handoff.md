# C3 Presented Session — Handoff

Status: **implemented and green** (2026-10-02, commit `3933c24`).

## What this slice closes

A persistent native window presenting continuous frames of the certified
scene, with the camera owned by Rust, and Tier-A evidence still sampled
through the offscreen authority path. This is the C3.1 session-contract
seed plus the C3.3 present loop. Input, simulation, frame pacing, and
checkpointing remain open.

## Architecture (authority-preserving)

```text
Rust (native_graphics_contract::session)
    PresentedGraphicsSession: validated request, world binding,
    camera walk (deterministic, normalized, typed),
    present_frames batches, close ledger
    -> worker op render_window {packet, camera_override?, frame_count,
       expected_packet_sha256}
Julia (LavaAdapter.render_window_frames!)
    PollEvents -> isopen? -> acquire_next_image!
    -> _record_scene_passes! (same as offscreen)
    -> _composite_capture! (shared resolve+overlay)
    -> readback_framebuffer(capture) -> upload! -> Lava.blit!
    -> present_frame!
    -> {frames_presented, frame_times_us, window_presented_frames}
Evidence: session.capture_and_promote re-seals the CURRENT camera once
and promotes through supervisor.render_and_promote (offscreen path).
The presented loop itself never carries un-promoted evidence.
```

The window shows the exact certified composite by construction: the
present path records the same scene passes and the same shared
resolve+overlay as the offscreen capture, and the blit source is a
readback of that capture framebuffer. There is no second renderer.

Camera movement re-seals only a camera variant; the supervisor accepts
it as an authorized projection (`validate_authorized_world_projection`),
and GPU resource caches keyed on `content_sha256` stay warm because the
worker receives the unchanged base packet plus a typed `camera_override`.

## Lava API facts (pinned revision `~/.julia/packages/Lava/V2Q5P`)

These cost real debugging time; recorded so the next slice does not
re-derive them. Also filed as FR-0009/0010/0011 in
`docs/platform/friction-ledger.md`.

- `Lava.checkopen(win)` is a **throwing assertion**, not a predicate: it
  returns nothing and throws only on a destroyed handle. The loop
  predicate is `Base.isopen(win)` (handle alive and no close request).
  `checkopen(win) || break` presents zero frames, silently.
- `draw!` to a `WindowTarget` has **no** `descriptor_set_layout`,
  `descriptor_set`, `depth`, or `depth_clear` kwargs (the offscreen
  variant has them): a window target has no depth attachment and no
  descriptor sets. Drawing the resolve pass directly to the window is a
  MethodError. The safe shape is: composite offscreen, then
  `Lava.blit!(bq, WindowTarget, source::LavaArray; clear=false)`.
- `blit!` fragment indexing is `idx = ix*height + iy + 1` (pixel `(x,y)`
  at `x*height + y + 1`). Vulkan image copies pack rows, so a
  `readback_framebuffer` matrix (column-major `(width,height)`, pixel
  `(x,y)` at `x + y*width`) is the **transpose** of the blit layout. The
  adapter rebuilds the flat blit-order buffer on the CPU
  (`_present_pixel_data`); no resampling, exact mapping.
- `readback_framebuffer(fb)` records an image→buffer copy into the
  active batch and flushes (`flush!` sets `bq.active_batch = nothing`).
  A readback recorded mid-frame therefore breaks the pending
  `present_frame!` ("called without an active recording batch"). Safe
  order used here: scene+composite → readback (flushes) → `upload!`
  (opens the next batch) → `blit!` → `present_frame!` (the submit that
  carries the acquire-semaphore wait; Lava adds it there, so callers
  must not add it again).
- `upload!(dst::LavaArray{T}, data::AbstractArray{T})` flushes on the
  device-local path (staging copy) but opens no batch of its own
  afterwards; `blit!` then `ensure_active_batch!`s the batch the present
  needs. Persistent window-sized `LavaArray{Vec4f,1}` lives on the
  `WindowSession`, reallocated only on extent change.
- `LavaArray{T,N}(undef, dims)` allocates only (no upload) — the right
  primitive for persistent present buffers.
- Swapchain format is B8G8R8A8_SRGB; the capture framebuffer is
  R32G32B32A32_SFLOAT, so the blit source is linear RGBA float and the
  swapchain does the sRGB encode. No host-side gamma is applied (and the
  presented image is therefore the certified pixel source, not a new
  color pipeline).
- `RenderWindow(width, height; ctx, title, vsync)`; `size(win)` returns
  the swapchain extent; `Base.close` is idempotent; one acquiring task
  per window (`acquire_next_image!` errors if the window already holds
  an image). `GLFW.PollEvents()` must be pumped by the session.

## Defects registered during this slice

- **C3.1 — telemetry tail UndefVarError.** The `_record_scene_passes!`
  splice left `_render_scene` reading `scene.terrain_vertices` before
  the returned tuple defined it. Fixed by reading the tuple field.
- **C3.2 — duplicated transition/timing triple.** The same splice left
  `_transition_color_to_sampled!` + `_end_gpu_pass_timing!` +
  `scene_raster_time_us = ...` twice at the tail of
  `_record_scene_passes!`. `_end_gpu_pass_timing!` is not idempotent;
  deduplicated. (Evidence passed only because the scene pass happened to
  tolerate the double-record on this driver.)
- **C3.3 — checkopen-as-predicate.** `checkopen(win) || break` always
  broke → 0 frames. Fixed to `isopen(session.window)`; frame 1 closed
  window raises typed `window_closed` AdapterError. See FR-0009.
- **C3.4 — resolve-to-window MethodError.** Offscreen `draw!` kwargs do
  not exist on the window variant. Redesigned to composite → readback →
  upload → `blit!` → present. See FR-0010/0011.
- **C3.5 — session test fixture race.** Per-test temp-dir layout copies
  raced across parallel Rust test threads (`fs::copy` truncates while
  Julia reads), failing a different test each run with a misleading
  `EOF while parsing a value`. Fixed with a process-shared `OnceLock`
  world fixture (WorldArtifact is Clone). See FR-0013.
- **C3.6 — repeat close contract.** The worker honestly reports
  `closed=false, presented_frames=0` for an already-closed session; the
  session now returns the durable last-known total instead, and the
  test's `again == presented_total` assertion holds.

## Evidence

- `julia --project=graphics_lab /tmp/c3_window_probe.jl`: 640×480 window
  on the C2.5 packet, `frames_presented=3`,
  `window_presented_frames=3`, clean close. First frame ~73.6s (pipeline
  compilation), then 56ms / 26ms steady state.
- `cargo test -p wge-native-graphics-contract --test presented_session`
  (both tests): negative control (present without open window) and the
  full loop — open 640×480 window → warmup offscreen promote → 12
  presented frames → camera move → Tier-A `capture_and_promote` → 4 more
  presented frames → close (total 16) → honest repeat close.
- `tests/session.rs` 5/5: request validation, camera extent mismatch,
  deterministic/normalized walk, nonfinite/degenerate rejection,
  world-divergence fail-closed.
- `lava_adapter.jl` 26/26; `runtests.jl` green (exit 0);
  `native_graphics` 7/7 per-test serial (6.5s–275s); lib 19/19;
  fmt clean; clippy clean.
- framerates above are with vsync=false in WSL2 on the RTX 5060; frame 1
  includes lazy pipeline compilation (the Rust test warms up offscreen
  first for exactly this reason).

## Doctrine notes

- No Unity/UE/Godot/Blender runtime; the window is Lava/GLFW on the
  pinned revision, owned by the adapter; Rust owns session semantics,
  camera truth, and all evidence claims.
- TetCageRT stays quarantined; nothing here touches it.
- Friction registered in `docs/platform/friction-ledger.md` (FR-0009..FR-0013).

## Next steps (in priority order)

1. C3.2 input + third-person camera: keyboard/mouse/gamepad
   abstraction, player movement on the existing grounded kinematic seam
   (`reference_runtime/src/physics.rs` `step` + collision), deterministic
   input traces, camera intent bound to the session camera.
2. C3.3 remainder: frame pacing policy + live capability reporting.
3. C3.4: wire Tier-B live / Tier-A sampled evidence into
   `LiveGraphicsSession` (the presented loop is the Tier-B side;
   `capture_and_promote` is the Tier-A side; the corruption-injection
   demotion proof remains).
4. C2 exit-gate remainder: imported-object collision participation,
   LOD/visibility policy.
5. C3.1 remainder: fixed-step simulation tick, checkpoint/restore,
   restart, session identity/telemetry.
