# C2.8 GPU Mip Residency and Sampler LOD — Handoff

Status: **implemented and green**, adapter seam bumped to `wge.lava-adapter/v7`
on both sides.

Predecessor: [`WGE_C2_7_MIP_CHAIN_HANDOFF.md`](WGE_C2_7_MIP_CHAIN_HANDOFF.md).
C2.7 gave the authority side a versioned, digest-bound `rgba8_mip_chain` while
the Lava adapter rejected multi-level payloads with a typed
unsupported-capability result. C2.8 closes the remaining seam: the GPU now
residents the full chain and the sampler spans it.

## What closed

The Julia adapter (`graphics_lab/src/LavaAdapter.jl`) now uploads and samples
multi-level textures:

- `MaterialTextureResources` carries `max_sampler_lod::UInt32`.
- `_texture_levels` validates a texture payload into a base level plus mip
  chain (`Vector{Matrix{NTuple{4,Float32}}}`), decoding every level through the
  existing per-role color-space path and revalidating mip byte lengths.
- `_lava_texture2d!` creates one Vulkan image
  (`FORMAT_R32G32B32A32_SFLOAT`, `mipLevels = length(levels)`), a whole-chain
  ImageView, and copies every level through one shared staging buffer with a
  per-level `BufferImageCopy`, transitioning
  `UNDEFINED -> TRANSFER_DST_OPTIMAL -> SHADER_READ_ONLY_OPTIMAL`. Image,
  memory, and view are owned by the returned texture; the pinned Lava rev owns
  destruction through finalizers. Nothing is destroyed manually.
- `_lod_sampler!` builds a Vulkan sampler with `maxLod = levels - 1`
  (Lava's own `LavaSampler` hardcodes `maxLod = 0`); argument order mirrors the
  pinned Lava rev.
- `_packet_residency_summary` mirrors Rust's `expected_texture_residency`
  exactly: texture ids deduplicated across all materials, mip-level and
  payload-byte sums over deduplicated ids, `max_sampler_lod =
  max(mip_levels - 1)`, and `nothing` when no source texture is referenced.
  `render_scene` reports it as the `texture_residency` telemetry field.

The Rust authority side already owned the contract: `GraphicsTextureResidencyTelemetry`
(`texture_count`, `mip_levels`, `payload_bytes`, `max_sampler_lod`),
`validate_texture_residency_telemetry`, and `expected_texture_residency`. A
multi-level packet requires residency telemetry and fails closed on mismatch;
`payload_bytes` counts source-packet RGBA8 bytes per level
(`width * height * 4`), not upload-format bytes. The Julia summary counts the
same source bytes, so promotion validates.

`payload_bytes` is source-byte identity. The adapter's internal
`state.upload_bytes` accounting still counts RGBA32F upload sizes (4x source
bytes for these levels); that is device-side truth, not packet identity, and
no gate consumes it as one.

## Replay control

Single-level textures clamp sampler LOD to `0..0`, so the C2.5 imported-asset
warm-state replay (hut textures are single-level) remains the byte-identical
backend regression control. Multi-level packets now resident their chain
instead of being rejected.

## Verification (full clean re-run, 2026-10-01)

After the last device/host-fault-class fix, every suite was re-run; no spot
checks.

- Isolation probes: single 512x512 upload OK; full C2.5 packet (18 textures,
  including the hut's 512x512 set) renders with all milestones green.
- `graphics_lab/test/lava_adapter.jl`: 26/26.
- `graphics_lab/test/runtests.jl`: all suites pass.
- `native_graphics` integration: 7/7, run per-test with
  `--test-threads=1` (the suite exceeds the 600 s aggregate timeout when run
  as one process). `rust_supervisor_promotes_a_bound_lava_frame` passed in
  258 s, including residency-telemetry validation against the Rust
  expectation and the deterministic replay comparison.
- `cargo fmt --check` green; strict clippy on `wge-native-graphics-contract`
  clean; contract lib tests 19/19.

## Defects registered during C2.8

Both defects are upload-path faults of the device/host class and triggered the
full clean re-run above. Symptoms were deliberately recorded as observed,
including the misleading ones.

- **C2.8.1 — inclusive-prefix staging offsets.** Julia's
  `accumulate(+, bytes; init=0)` folds `init` into the first element, yielding
  the inclusive prefix `[b1, b1+b2, ...]` — not the exclusive offsets
  `[0, b1, ...]` the copy loop needs. Level 1 was written at `base + b1`, past
  the end of the staging buffer. Small levels "worked" by landing in adjacent
  mapped pages, corrupting whatever lived there (observed downstream as a
  device-lost at worker restart and other nondeterministic misbehavior); the
  first 4 MiB level segfaulted in `memmove` (exit 139). The inclusive end
  equals the exclusive end, so `get_staging(bq, offsets[end])` masked the bug.
  Fix: `offsets = pushfirst!(accumulate(+, level_bytes), 0)`, used for both the
  host write and the `BufferImageCopy` buffer offsets. Lesson: exclusive-prefix
  offsets must be constructed explicitly, and an early full-render probe beats
  reading small-texture passes as evidence.
- **C2.8.2 — host copy lifetime and element typing.** The first copy
  materialized a temporary whose pointer was the last use, so the GC could
  collect it mid-`unsafe_copyto!` (Lava's staging docstring explicitly
  requires `GC.@preserve` for exactly this reason). The replacement fix then
  type-errored before writing anything:
  `unsafe_copyto!` requires both pointers to share an element type, and
  `Ptr{UInt8}` vs `Ptr{NTuple{4,Float32}}` has no method. Final form: copy
  element-typed from `pointer(level)` under `GC.@preserve level`, with
  `length(level)` 16-byte RGBA elements; staging offsets are 16-byte
  multiples, so alignment holds. Lesson: raw copies into mapped Vulkan memory
  must (a) GC-preserve the source object across the copy and (b) be typed on
  both ends; a segfault in `memmove` is a symptom class, not a diagnosis.

## Explicitly open

- A typed unsupported result when the device/profile cannot provide the
  requested residency (contract language from the C2.7 frontier list). No
  current packet hits this path; it is contract hardening, not a silent
  fallback.
- A deterministic single-level fallback only when the packet requests it.
  Currently no fallback exists and none is needed: every packet carries a
  base level.
- The upload format remains RGBA32F — the adapter's proven sample format.
  A narrower-format seam (fewer bytes resident per level) is a separate,
  later validation.
