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
  `wge.graphics-scene-packet/v3`.
- Native packet contents: deterministic terrain, two gameplay-critical
  obstacle instances, thirty background foliage instances, route/spawn/
  encounter/objective projections, typed materials, environment lighting,
  directional shadow map, linear HDR resolve, and semantic visibility
  telemetry.
- Capture: deterministic 384x256 PPM for the native Lava path; the Bevy
  inspection capture is 320x240 PNG.

## Results

| Path | Wall time | Max RSS | Result | Notes |
| --- | ---: | ---: | --- | --- |
| Reference-runtime `build` | 3.49 s | 383,604 KiB | passed | 78 traversal steps; gameplay won; visual gate passed |
| Reference-runtime `verify` | 0.75 s | — | passed | independently revalidated world, traversal, gameplay, and visual evidence hashes |
| Native Lava `render-layout` (cold CLI) | 105.59 s | 1,909,480 KiB | passed | Rust lowered, supervised, rendered, measured, and promoted the receipt |
| Persistent Lava worker, first frame | 54.862 s frame time | 1,928,136 KiB process | passed | lazy device/pipeline/scene initialization |
| Persistent Lava worker, second frame | 26.774 ms frame time | same process | passed | byte-identical capture digest; seven pipelines, no new compilation |
| Bevy native inspection capture (cold) | 155.52 s | 6,104,240 KiB | provenance passed | artifact revalidated; visual representation remains coarse and is not used as Lava parity evidence |

The persistent-worker values are deliberately separated from the cold CLI
value. The CLI includes Julia startup, capability probing, Lava initialization,
shader compilation, packet transport, capture, and process teardown. It is a
deployment/startup measurement, not a steady-state frame-time claim.

The warm replay produced the same capture digest on both frames:

`sha256:2466d14b...`

The native Rust receipt for the promoted Riverwatch frame reported:

- 31 instances: 30 background, 1 gameplay-critical in the current Riverwatch
  fixture after deterministic foliage placement;
- 13 draw calls, 10 pipeline compilations, 36,297 uploaded bytes, and 308,224
  readback bytes;
- 31 visible and 0 culled instances for this camera;
- terrain vertex count 13,824 and mesh vertex count 48;
- visual measurements: luminance standard deviation `0.0715599372`, 4,481
  distinct colors, route pixels 290, player pixels 7, opponent pixels 6,
  encounter pixels 69, objective pixels 7.

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

- Rust native graphics contract: 9 unit tests, clippy with `-D warnings`, and
  the six-test native graphics suite passed.
- Julia protocol worker: 7 worker protocol tests and 2 fake-rejection tests
  passed.
- Julia Lava adapter: 25 persistent GPU tests plus focused color, lighting,
  importance, capture, and camera test sets passed.
- Reference runtime: 10 tests passed.
- Bevy viewer: 22 binary/unit tests passed.

## Interpretation and next benchmark improvements

This checkpoint establishes a repeatable native path and a meaningful warm
frame measurement. It does not yet establish a production frame budget. The
next benchmark slice should add GPU timestamp queries, at least 30 warm frames
with p50/p95/p99 reporting, a denser authored scene, a second material class,
and separate upload, compilation, render, readback, and promotion timings.
Quartz should be rerun after the foliage pass so both current reference scenes
share the same graphics revision.
