# WGE Native Graphics Benchmark

Status: reproducible checkpoint, 2026-09-28

This report measures the current engine-neutral Riverwatch vertical slice. It
is a substrate and evidence report, not a claim of AAA visual parity. Unity is
intentionally not part of this checkpoint.

## Test subject

- Host GPU: NVIDIA GeForce RTX 5060.
- Native graphics backend: Julia 1.12, Lava.jl pinned at
  `11c7e31bdf62408d22bf379e9e59510f69d2103e`, Vulkan offscreen rendering.
- Rust authority: `wge-native-graphics-contract` and the reference runtime.
- World: `riverwatch.layout.json`, lowered to graphics packet
  `wge.graphics-scene-packet/v5`.
- Native packet contents: deterministic terrain, one gameplay-critical
  obstacle instance in the Riverwatch fixture, thirty background foliage
  instances, route/spawn/
  encounter/objective projections, typed materials, environment lighting,
  authored mesh UV0 channels, a semantic objective beacon with a distinct
  emissive role, directional shadow map, linear HDR resolve, and semantic
  visibility telemetry.
- Material contract: `wge.graphics-scene-packet/v5`, adapter identity
  `wge.lava-adapter/v3`, with digest-bound albedo, normal, roughness,
  occlusion, and emissive roles, including a beacon-specific albedo/emissive
  pair.
- Capture: deterministic 320x240 PPM for the native Lava path; the Bevy
  inspection capture is 320x240 PNG.

## Results

| Path | Wall time | Max RSS | Result | Notes |
| --- | ---: | ---: | --- | --- |
| Reference-runtime `build` | 3.49 s | 383,604 KiB | passed | 78 traversal steps; gameplay won; visual gate passed |
| Reference-runtime `verify` | 0.75 s | — | passed | independently revalidated world, traversal, gameplay, and visual evidence hashes |
| Native Lava `render-layout` (cold CLI) | 124.01 s | 1,722,324 KiB | passed | Rust lowered, supervised, rendered, measured, and promoted the v5 receipt |
| Persistent Lava worker, first frame | see formal cold sample | — | passed | lazy device/pipeline/scene initialization |
| Persistent Lava worker, warm frames | 291.245 ms wall p50 | — | passed | 30 supervised warm samples; no new compilation; byte-identical capture digest |
| Formal persistent benchmark (1 cold + 30 warm) | 26.960 s cold wall; 291.245 ms warm p50 | — | passed | Rust promotion round trip; warm wall p95 322.104 ms, p99 328.911 ms; deterministic captures |
| Synthetic dense-foliage benchmark (1 cold + 2 warm; 512 added) | 30.650 s cold wall; 389.322 ms warm p50 | — | passed | 544 total instances, 542 synthetic background; benchmark-only scalability evidence |
| Bevy native inspection capture (cold) | 155.52 s | 6,104,240 KiB | provenance passed | artifact revalidated; visual representation remains coarse and is not used as Lava parity evidence |

The persistent-worker values are deliberately separated from the cold CLI
value. The CLI includes Julia startup, capability probing, Lava initialization,
shader compilation, packet transport, capture, and process teardown. It is a
deployment/startup measurement, not a steady-state frame-time claim.

The warm replay produced the same capture digest on every frame:

`sha256:564ffb3b4d5ef0e6545cc7cf53397628f354c9c2ba5613540f03fd172e8fba8f`

The promoted benchmark packet digest was:

`sha256:ecffb37c4a44dc5d7fd0bce81e81592ad8e136d10a6e25055a45e5ce15c421e9`

The native Rust receipt for the promoted Riverwatch frame reported:

- 32 instances: 30 background, 1 landmark beacon, and 1 gameplay-critical in the current Riverwatch
  fixture after deterministic foliage placement;
- 15 draw calls, 10 pipeline compilations, 59,433 uploaded bytes, and 308,224
  readback bytes;
- 32 visible and 0 culled instances for this camera;
- terrain vertex count 13,824 and mesh vertex count 240;
- visual measurements: luminance standard deviation `0.0822184531`, 2,730
  distinct colors, route pixels 290, player pixels 7, opponent pixels 6,
  encounter pixels 69, objective pixels 7.

The formal persistent benchmark uses the full supervisor round trip and Rust
promotion for every frame. Its cold promoted frame reported a 12.326 s
adapter interval and a 12.260 s graphics-pass GPU interval, dominated by
first-use pipeline/device work. Across 30 warm frames it reported:

- wall time: p50 `291.245 ms`, p95 `322.104 ms`, p99 `328.911 ms`, mean
  `293.247 ms`;
- renderer-reported frame time (including Lava readback): p50 `47.596 ms`,
  p95 `61.962 ms`, p99 `67.665 ms`, mean `46.735 ms`;
- graphics-pass GPU interval: p50 `405 µs`, p95 `619 µs`, p99 `928 µs`, mean
  `377 µs`;
- every warm capture matched the cold capture digest.

The separate cold `render-layout` receipt reported a 12.958 s adapter
interval and a 12.892 s graphics-pass GPU interval. Its process consumed
1,722,324 KiB maximum resident memory including the Rust/Cargo CLI boundary.

The current v3 adapter also reports named pass timings. The 30-frame overview
warm p50s were 11 µs CPU preparation, 453 µs CPU scene raster, 58 µs CPU
resolve, 44 µs CPU overlay, and 2,515 µs flush/readback. GPU pass p50s were
1 µs preparation, 383 µs scene raster, 20 µs resolve, and 3 µs overlay. The
CPU p99s include occasional host-side scheduling/GC outliers (5.385 ms scene
raster, 5.473 ms resolve, and 33.136 ms flush/readback); they are retained
rather than trimmed.

The named Vulkan pass timestamps deliberately insert synchronization barriers
so their intervals reconcile with the whole-frame interval. They are therefore
instrumented measurements, not an uninstrumented production frame budget.

The synthetic dense-foliage profile adds 512 deterministic background
instances to the same validated base packet. Its packet digest was
`sha256:79c7be241ee6165041ce77cae8b1575c2f0c916e93d3cd7cf190dee1a040e5be`;
the warm p50/p95 values were 365.103/367.856 ms wall, 26.567/36.345 ms
renderer time, and 477/504 µs GPU time. It submitted the same 15 draw calls
and 240 base mesh vertices, with 157,737 uploaded bytes and 544 visible
instances. This measures instanced dense-scene scaling only; it is not authored
world content and is not certification evidence.

The close-range objective inspection command produced a separate promoted
receipt from the same world:

- packet digest `sha256:392e5b58eb5d46952efebfca83a02257fb862ec811616eeb75cd3590bb2a90b`;
- capture digest `sha256:4c3c32bad21d66b96bc6b123a2b652163b863d6f91bb9be2028e19492243b4a6`;
- 13 draws, 9 pipeline compilations, 48,265 uploaded bytes, 32 total
  instances, 10 visible and 22 culled, including 1 visible landmark;
- 204 mesh vertices, 1,704 distinct colors, and luminance standard deviation
  `0.0374367692`.

This view intentionally has no gameplay overlays, so zero marker pixels there
is expected; overview promotion remains the gameplay-visible visual gate.

The GPU interval is a Vulkan timestamp bracket around the native graphics
commands with pass-level instrumentation enabled; it excludes CPU
packet/protocol work and the host readback. The renderer-reported frame time
includes Lava readback, while wall time includes the full Rust-supervisor
promotion round trip. These are measurements of the small diagnostic scene and
the synthetic stress profile, not a production frame budget.

These numbers are evidence of a real deterministic path, not a quality score.
The current scene is intentionally small and diagnostic; the quality-gap
report records what is still missing.

## Reproduction

From `world_core`:

```text
cargo run -q -p wge-reference-runtime -- build \
  --layout crates/reference_runtime/examples/riverwatch.layout.json \
  --output-dir /tmp/wge-bench-riverwatch

cargo run -q -p wge-reference-runtime -- verify \
  --bundle /tmp/wge-bench-riverwatch

cargo run -q -p wge-native-graphics-contract -- render-layout \
  crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia \
  "/mnt/d/Code Projects/WGE/terrain_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab/bin/wge_graphics_worker.jl" \
  /tmp/wge-riverwatch-native.ppm

cargo run -q -p wge-native-graphics-contract -- render-close-layout \
  crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia \
  "/mnt/d/Code Projects/WGE/terrain_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab/bin/wge_graphics_worker.jl" \
  /tmp/wge-riverwatch-objective-close.ppm

cargo run -q -p wge-native-graphics-contract -- benchmark-layout \
  crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia \
  "/mnt/d/Code Projects/WGE/terrain_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab/bin/wge_graphics_worker.jl" \
  30

cargo run -q -p wge-native-graphics-contract -- benchmark-dense-layout \
  crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia \
  "/mnt/d/Code Projects/WGE/terrain_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab/bin/wge_graphics_worker.jl" \
  2 512
```

For the Bevy inspection path, use the certified world artifact produced by the
reference-runtime build:

```text
cargo run -q -p codeweald-world-viewer -- \
  --native-world /tmp/wge-bench-riverwatch/world_artifact.json \
  --capture /tmp/wge-bevy-riverwatch.png \
  --view overview
```

The known `liblsfg-vk-layer.so` loader warning is emitted by the host Vulkan
environment and is skipped by the loader. It did not produce a validation
failure in these runs.

## Regression evidence

- Rust native graphics contract: 11 unit tests, clippy with `-D warnings`, and
  the six-test native graphics suite passed.
- Julia protocol worker: 7 worker protocol tests and 2 fake-rejection tests
  passed.
- Julia Lava adapter: 26 persistent GPU tests plus focused color, lighting,
  importance, capture, and camera test sets passed.
- Reference runtime: 10 tests passed.
- Bevy viewer: 22 binary/unit tests passed.

## Interpretation and next benchmark improvements

This checkpoint establishes a repeatable native path, a measured 30-frame warm
distribution, named CPU/GPU pass timing, a deterministic synthetic dense
profile, and a close-range material inspection profile. It does not yet
establish a production frame budget, traversal-aware culling behavior, or
imported hero-asset quality. The next benchmark slice should add a denser
authored scene or camera-cut profile, separate uninstrumented-vs-instrumented
timings, and explicit upload/compilation/render/readback/promotion attribution.
Quartz should be rerun after the foliage pass so both current reference scenes
share the same graphics revision.
