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
  authored mesh UV0 channels, directional shadow map, linear HDR resolve, and
  semantic visibility telemetry.
- Material contract: `wge.graphics-scene-packet/v5`, adapter identity
  `wge.lava-adapter/v2`, with digest-bound albedo, normal, roughness,
  occlusion, and emissive roles.
- Capture: deterministic 384x256 PPM for the native Lava path; the Bevy
  inspection capture is 320x240 PNG.

## Results

| Path | Wall time | Max RSS | Result | Notes |
| --- | ---: | ---: | --- | --- |
| Reference-runtime `build` | 3.49 s | 383,604 KiB | passed | 78 traversal steps; gameplay won; visual gate passed |
| Reference-runtime `verify` | 0.75 s | — | passed | independently revalidated world, traversal, gameplay, and visual evidence hashes |
| Native Lava `render-layout` (cold CLI) | 111.67 s | 2,008,324 KiB | passed | Rust lowered, supervised, rendered, measured, and promoted the v5 receipt |
| Persistent Lava worker, first frame | see formal cold sample | — | passed | lazy device/pipeline/scene initialization |
| Persistent Lava worker, warm frames | 259.311 ms wall p50 | — | passed | 30 supervised warm samples; no new compilation; byte-identical capture digest |
| Formal persistent benchmark (1 cold + 30 warm) | 27.299 s cold wall; 259.311 ms warm p50 | — | passed | Rust promotion round trip; warm wall p95 309.788 ms, p99 320.549 ms; deterministic captures |
| Bevy native inspection capture (cold) | 155.52 s | 6,104,240 KiB | provenance passed | artifact revalidated; visual representation remains coarse and is not used as Lava parity evidence |

The persistent-worker values are deliberately separated from the cold CLI
value. The CLI includes Julia startup, capability probing, Lava initialization,
shader compilation, packet transport, capture, and process teardown. It is a
deployment/startup measurement, not a steady-state frame-time claim.

The warm replay produced the same capture digest on every frame:

`sha256:d9d5e2f2adc4ebc6095fe6bb6a54e362d9815223d6711d40e55321a4b97f1c09`

The promoted benchmark packet digest was:

`sha256:af3413b768a505e5ca6f95221147c0f93684d62731376de84457ddd92df2fa19`

The native Rust receipt for the promoted Riverwatch frame reported:

- 31 instances: 30 background, 1 gameplay-critical in the current Riverwatch
  fixture after deterministic foliage placement;
- 13 draw calls, 10 pipeline compilations, 41,833 uploaded bytes, and 308,224
  readback bytes;
- 31 visible and 0 culled instances for this camera;
- terrain vertex count 13,824 and mesh vertex count 48;
- visual measurements: luminance standard deviation `0.0822154541`, 2,685
  distinct colors, route pixels 290, player pixels 7, opponent pixels 6,
  encounter pixels 69, objective pixels 7.

The formal persistent benchmark uses the full supervisor round trip and Rust
promotion for every frame. Its cold promoted frame reported a 12.941 s
adapter interval and a 12.873 s graphics-pass GPU interval, dominated by
first-use pipeline/device work. Across 30 warm frames it reported:

- wall time: p50 `259.311 ms`, p95 `309.788 ms`, p99 `320.549 ms`, mean
  `267.667 ms`;
- renderer-reported frame time (including Lava readback): p50 `51.002 ms`,
  p95 `71.326 ms`, p99 `106.474 ms`, mean `52.138 ms`;
- graphics-pass GPU interval: p50 `722 µs`, p95 `1,277 µs`, p99 `1,278 µs`,
  mean `795 µs`;
- every warm capture matched the cold capture digest.

The separate cold `render-layout` receipt reported an 11.153 s adapter
interval and an 11.094 s graphics-pass GPU interval. Its process consumed
2,008,324 KiB maximum resident memory including the Rust/Cargo CLI boundary.

The GPU interval is a Vulkan timestamp bracket around the native graphics
commands; it excludes CPU packet/protocol work and the host readback. The
renderer-reported frame time includes Lava readback, while wall time includes
the full Rust-supervisor promotion round trip. These are measurements of the
small diagnostic scene, not a production frame budget.

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

cargo run -q -p wge-native-graphics-contract -- benchmark-layout \
  crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia \
  "/mnt/d/Code Projects/WGE/terrain_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab/bin/wge_graphics_worker.jl" \
  30
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
distribution, and a real graphics-pass GPU timer. It does not yet establish a
production frame budget. The next benchmark slice should add a denser authored
scene, per-pass GPU timing breakdown, and separate upload, compilation,
render, readback, and promotion timings. Quartz should be rerun after the
foliage pass so both current reference scenes share the same graphics revision.
