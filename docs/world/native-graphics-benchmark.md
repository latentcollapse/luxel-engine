# WGE Native Graphics Benchmark

Status: adapter-v6 native backend and Campaign 2 authored-frame checkpoint
measured; registered visual-quality certification is green, while broader
production visual quality remains open, 2026-09-29

This report measures the current engine-neutral Riverwatch vertical slice. It
is a substrate and evidence report, not a claim of AAA visual parity. External
engines are reference literature only and are not a WGE comparison axis.

## Test subject

- Host GPU: NVIDIA GeForce RTX 5060.
- Native graphics backend: Julia 1.12, Lava.jl pinned at
  `11c7e31bdf62408d22bf379e9e59510f69d2103e`, Vulkan offscreen rendering.
- Rust authority: `wge-native-graphics-contract` and the reference runtime.
- World: `riverwatch.layout.json`, lowered to graphics packet
  `wge.graphics-scene-packet/v6`.
- Native packet contents: deterministic terrain, one gameplay-critical
  obstacle instance in the Riverwatch fixture, thirty background foliage
  instances, route/spawn/
  encounter/objective projections, typed materials, environment lighting,
  authored mesh UV0 channels, a semantic objective beacon with a distinct
  emissive role, directional shadow map, linear HDR resolve, and semantic
  visibility telemetry.
- Material contract: `wge.graphics-scene-packet/v6`, adapter identity
  `wge.lava-adapter/v6`, with digest-bound albedo, normal, roughness,
  occlusion, and emissive roles, including a beacon-specific albedo/emissive
  pair plus bounded clearcoat response.
- Capture: deterministic 320x240 PPM for the native Lava path; the Bevy
  inspection capture is 320x240 PNG.

## Strict technical visual-quality sample

The fresh `native-world-showcase-v1` capture passed the Rust-registered visual
profile and authority revalidation. Packet/capture/evidence identities are
`sha256:a65274feffb3d3f2e2c5e454de680b65d7bffb8810a60e7a4b344a3d15ec23cc`,
`sha256:29eda4cd5b32a579fbf3957f20223e0130850d520d00a99cfb4960762e4c5a54`,
and `sha256:ea4f4eca05532659164177091ea9a4f1369f54344d8f5f41fda3b59123733f63`.
The 768x512 capture measured 2,281 bp terrain coverage, 28,269 projected
authored-geometry pixels, 3,000 bp combined content coverage, 576 bp spatial
edges, 28 luminance bins, 321 RGB bins, and 31/64 varied tiles. The terrain
and geometry masks are reported separately so foreground meshes cannot satisfy
the terrain-coverage requirement by relabeling themselves as terrain.

This is a deterministic technical floor, not an aesthetic, AAA, or imported
hero-asset claim. The visual artifact is preserved under
`/home/mattc/Pictures/WGE/native-world-showcase-certified-2026-09-29.png`.

The same profile was then run against the distinct non-fixture
`cedar_saddle_relay` layout. It passed with `2,571 bp` terrain coverage,
`3,255 bp` combined content coverage, `529 bp` spatial edges, `28/314`
luminance/RGB bins, and `33/64` varied tiles. The capture is preserved at
`/home/mattc/Pictures/WGE/cedar-world-showcase-certified-2026-09-29.png`.

## Campaign 2 authored-frame measurement

The final Campaign 2 verification used two fresh Rust-supervised processes over
the same Riverwatch input. `live-twelfth` is the final current-binary run and
`live-eleventh` is the clean replay. The exact command was:

```text
world_core/target/debug/wge-native-graphics-contract render-campaign2-layout \
  world_core/crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia terrain_lab graphics_lab \
  graphics_lab/bin/wge_graphics_worker.jl artifacts/campaign2/live-twelfth
```

All three cuts passed the Rust-registered `campaign2-authored-frame` profile
and the independently revalidated Campaign 2 vector evidence. Runtime timing
is host/load-dependent; the certification receipt deliberately zeros timing so
identity remains deterministic.

| View | Capture | Terrain / content coverage | Luma span | RGB bins | Edge bp | CPU frame | GPU frame | Upload / readback | Visible / total |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| close | 768×512 | 4,757 / 7,104 bp | 122 | 527 | 515 | 12.658 s | 12.485 s | 2.70 MiB / 1.50 MiB | 22 / 34 |
| medium | 960×640 | 4,669 / 5,765 bp | 86 | 475 | 293 | 0.300 s | 0.106 s | 2.70 MiB / 2.34 MiB | 34 / 34 |
| wide | 960×640 | 3,182 / 3,831 bp | 80 | 450 | 328 | 0.438 s | 0.271 s | 2.70 MiB / 2.34 MiB | 34 / 34 |

The first cut includes fresh device/context and shader initialization. The two
later cuts reuse the persistent worker. These are small-scene measurements, not
a production frame budget; the broad cold-start and per-pass attribution gaps
remain in the quality register.

Certification identities from the clean replay:

- close packet `sha256:db6129922c0cbbcbc7cdbe8be478b7ac899e0955f0b5d6e6381a1931781c085d`,
  capture `sha256:57e4b618b4f7e19c8af41ac974433da95610a267f5505abff792db1146115260`;
- medium packet `sha256:4ec1168bdbb396055cfb399e612a331650b65797a8b949d084358500aee37032`,
  capture `sha256:4a37de535b6ae81cebd5d4f50157f524da59f0ff3ab1ea25046a33cbe87e34fa`;
- wide packet `sha256:5c1ae0b334adc527181cb35c6c073e02cf08a442b67c8b656c0eee8f2a5295ba`,
  capture `sha256:0e1666373754589e2ed5e7c45350f78ca24057b72fcfc90b6db4b4f733af2c00`.

The following certification artifacts matched byte-for-byte between
`live-eleventh` and `live-twelfth` for every view: world artifact, scene packet, deterministic
frame receipt, renderer attestation, raw RGBA, PPM, and visual-quality evidence.
The runtime-bound Campaign 2 vector evidence intentionally differs only in its
non-deterministic runtime receipt/timing identity. This is recorded rather than
folded into deterministic provenance.

The final PNG/PPM/evidence bundle is available at
`/home/mattc/Pictures/WGE/campaign2-2026-09-29/`. This campaign did not add an
external-engine comparison gate.

## Current adapter-v6 close sample

Fresh Rust-supervised runs on 2026-09-29 used the same Riverwatch input and
the pinned Lava/Vulkan revisions above. The canonical packet was
`sha256:07f2a88c4acb0cba16ac45db0fd3fb1a3a7bb8fe7aa81a99da6365c6e1cd486a`,
with capture `sha256:066963d333fe3b7da42204ad27d3603c79ea889c53d268556558e438dfbb6081`.
The cold promoted frame took 31.131 s wall, 14.560 s in the adapter, and
reported 11 draws, 7 pipeline compilations, 82,857 uploaded bytes, and 307,200
readback bytes. It had 32/32 visible instances and passed all five role gates.

The formal 30-warm-frame run remained byte-deterministic. Warm wall time was
918.113 ms p50, 1,144.686 ms p95, and 1,274.622 ms p99; renderer frame time
was 39.750 ms p50, 69.660 ms p95, and 75.054 ms p99; GPU frame time was 58 µs
p50, 485 µs p95, and 559 µs p99. The current wall distribution contains a
large interval outside the renderer-reported frame time; that interval is not
separately attributed by this benchmark. It is recorded as a fresh sample, not
silently substituted for the historical baseline. All 30 warm captures matched
the cold digest.

The fresh dense profile used packet
`sha256:9cf3504ff637258c2597f9fb43910fb8c9bb15a1ae377e0a4c438115cb9a013e`:
544 total instances (542 synthetic background), 29.483 s cold wall, 1,123.848
ms warm wall p50 and 1,139.561 ms p95/p99, with 25.486 ms
renderer p50 and 94 µs GPU p50. Its deterministic capture was
`sha256:55eb6fe54cff3f8f8a5e007bd0deb41dfc5b715e7fe5002115ec028d6d7a4b47`.
The dense profile remains benchmark-only scalability evidence, not authored
world evidence.

## Results (v4 baseline before clearcoat)

The original 30-frame distribution below was collected before the v6/v5
clearcoat contract refresh. It remains useful as the longer historical timing
baseline; the current adapter identity and a fresh deterministic smoke sample
are recorded immediately above and below it.

| Path | Wall time | Max RSS | Result | Notes |
| --- | ---: | ---: | --- | --- |
| Reference-runtime `build` | 3.49 s | 383,604 KiB | passed | 78 traversal steps; gameplay won; visual gate passed |
| Reference-runtime `verify` | 0.75 s | — | passed | independently revalidated world, traversal, gameplay, and visual evidence hashes |
| Native Lava `render-layout` (cold CLI) | 116.17 s | 1,693,752 KiB | passed | Rust lowered, supervised, rendered, measured, and promoted the v4 receipt |
| Persistent Lava worker, first frame | see formal cold sample | — | passed | lazy device/pipeline/scene initialization |
| Persistent Lava worker, warm frames | 263.067 ms wall p50 | — | passed | 30 supervised warm samples; no new compilation; byte-identical capture digest |
| Formal persistent benchmark (1 cold + 30 warm) | 28.965 s cold wall; 263.067 ms warm p50 | — | passed | Rust promotion round trip; warm wall p95 289.520 ms, p99 344.163 ms; deterministic captures |
| Synthetic dense-foliage benchmark (1 cold + 2 warm; 512 added) | 27.352 s cold wall; 329.167 ms warm p50 | — | passed | 544 total instances, 542 synthetic background; benchmark-only scalability evidence |
| Bevy native inspection capture (cold) | 155.52 s | 6,104,240 KiB | provenance passed | artifact revalidated; visual representation remains coarse and is not used as Lava parity evidence |

### Historical v5 clearcoat contract refresh

The following sample predates the current adapter-v6 audit and is retained as
a historical performance and determinism control.

After the material contract moved to packet schema v6 and adapter revision v5,
a fresh supervised five-warm-frame benchmark remained deterministic:

- packet: `sha256:b7f7b3b8b8c3b4d0589f98bcea516f7f4c464bea7a45da000c06b0cda78e9a63`;
- cold wall time: `26.890 s`; warm wall p50/p95: `292.807/321.597 ms`;
- warm renderer-frame p50: `46.455 ms`; warm GPU-frame p50: `57 µs`;
- draw calls: `15`; pipeline compilations: `10`; mesh vertices: `240`;
- warm capture: `sha256:9e29309aa8a8afd1ea79e9a73278f8b0659b3efdd11ee8d6415a175df2c2f043`;
- deterministic capture: `true` across cold and all warm frames.

The five-frame sample is a contract-refresh smoke benchmark, not a replacement
for the earlier 30-frame distribution. It confirms that the clearcoat buffer
and shader path do not change the existing scene's draw topology or capture
determinism.

The composed-world camera-cut probe then rendered 40 real instances at 768x512
with 28 draw calls, 4,656 submitted mesh vertices, 413,297 uploaded bytes, and
8,056 distinct RGB colors in the foliage-material camera trial.
Its latest promoted capture is
`sha256:e68989b5d54a73117fbdc4da7829a27ee3edb6802f67b3c5ca13a1880fd02e33`.
This is coverage and stress evidence, not a claim that the current vegetation
or terrain presentation has reached the desired high-end bar.

The persistent-worker values are deliberately separated from the cold CLI
value. The CLI includes Julia startup, capability probing, Lava initialization,
shader compilation, packet transport, capture, and process teardown. It is a
deployment/startup measurement, not a steady-state frame-time claim.

The warm replay produced the same capture digest on every frame:

`sha256:6659d6b36209a7ad43c429ce7e94932e6cdc67e6240a76d1937f7c2bad125b85`

The promoted benchmark packet digest was:

`sha256:ecffb37c4a44dc5d7fd0bce81e81592ad8e136d10a6e25055a45e5ce15c421e9`

The native Rust receipt for the promoted Riverwatch frame reported:

- 32 instances: 30 background, 1 landmark beacon, and 1 gameplay-critical in the current Riverwatch
  fixture after deterministic foliage placement;
- 15 draw calls, 10 pipeline compilations, 59,433 uploaded bytes, and 308,224
  encoded RGBA8 capture bytes for that frame (telemetry counters are now
  per-render deltas, not process-lifetime totals);
- 32 visible and 0 culled instances for this camera;
- terrain vertex count 13,824 and mesh vertex count 240;
- visual measurements: luminance standard deviation `0.0821845230`, 2,696
  distinct colors, route pixels 290, player pixels 7, opponent pixels 6,
  encounter pixels 69, objective pixels 7.

The formal persistent benchmark uses the full supervisor round trip and Rust
promotion for every frame. Its cold promoted frame reported a 14.230 s
adapter interval and a 14.161 s graphics-pass GPU interval, dominated by
first-use pipeline/device work. Across 30 warm frames it reported:

- wall time: p50 `263.067 ms`, p95 `289.520 ms`, p99 `344.163 ms`, mean
  `267.258 ms`;
- renderer-reported frame time (including Lava readback): p50 `47.687 ms`,
  p95 `69.898 ms`, p99 `85.338 ms`, mean `48.244 ms`;
- graphics-pass GPU interval: p50 `724 µs`, p95 `1,323 µs`, p99 `1,694 µs`,
  mean `809 µs`;
- every warm capture matched the cold capture digest.

The separate cold `render-layout` receipt reported an 11.972 s adapter
interval and an 11.913 s graphics-pass GPU interval. Its process consumed
1,931,268 KiB maximum resident memory including the Rust/Cargo CLI boundary.

The v4 adapter also reports named pass timings. The 30-frame overview
warm p50s were 9 µs CPU preparation, 443 µs CPU scene raster, 58 µs CPU
resolve, 46 µs CPU overlay, and 3,007 µs flush/readback. GPU pass p50s were
1 µs preparation, 484 µs scene raster, 247 µs resolve, and 3 µs overlay. The
CPU p99s include occasional host-side scheduling/GC outliers (4.181 ms scene
raster, 3.991 ms resolve, and 6.374 ms flush/readback); they are retained
rather than trimmed.

Named Vulkan pass timestamps are recorded in the same command stream as the
draws. The required framebuffer readback supplies completion synchronization;
the timestamp fields remain instrumented measurements, not an uninstrumented
production frame budget.

The synthetic dense-foliage profile adds 512 deterministic background
instances to the same validated base packet. Its packet digest was
`sha256:79c7be241ee6165041ce77cae8b1575c2f0c916e93d3cd7cf190dee1a040e5be`;
the warm p50/p95 values were 329.167/379.100 ms wall, 21.779/22.861 ms
renderer time, and 54/67 µs GPU time. It submitted the same 15 draw calls
and 240 base mesh vertices, with 157,737 uploaded bytes and 544 visible
instances. This measures instanced dense-scene scaling only; it is not authored
world content and is not certification evidence.

The close-range objective inspection command produced a separate promoted
receipt from the same world:

- packet digest `sha256:392e5b58eb5d46952efebfca83a02257fb862ec811616eeb75cd3590bb2a90b`;
- capture digest `sha256:12483c2584a8ce9feaf83faad2dea870765680e06c18ac9698eaf02bdce35029`;
- 13 draws, 9 pipeline compilations, 48,265 uploaded bytes, 32 total
  instances, 10 visible and 22 culled, including 1 visible landmark;
- 204 mesh vertices, 1,705 distinct colors, and luminance standard deviation
  `0.0374517518`.

This view intentionally has no gameplay overlays, so zero marker pixels there
is expected; overview promotion remains the gameplay-visible visual gate.

The native showcase inspection profile produced a separate, higher-resolution
promoted receipt from the same world:

- packet digest `sha256:362d3726ffe7a74a6fb9ef2ef5585d350948d25235153367ed3344271a330424`;
- capture digest `sha256:468e0612d1fe6e8ef09abd6329ed0ee984698a41acb67ed04cb5a24cb278ca3e`;
- 640x480 capture, 24 draws, 9 visible landmark instances, and 4,608 mesh
  vertices;
- 12,638 distinct colors and luminance standard deviation `0.1336471737`;
- a fresh-process replay produced byte-identical PPM bytes and the same
  capture digest.

This is the accepted native visual-quality probe documented in
[`docs/archive/2026-09_native-graphics-checkpoints/native-showcase.md`](../archive/2026-09_native-graphics-checkpoints/native-showcase.md). It demonstrates a typed
stone/metal/emissive composition and is not imported-asset parity evidence.

The GPU interval is a Vulkan timestamp bracket around the native graphics
commands with pass-level instrumentation enabled; it excludes CPU
packet/protocol work and the host readback. The renderer-reported frame time
includes Lava readback, while wall time includes the full Rust-supervisor
promotion round trip. These are measurements of the small diagnostic scene and
the synthetic stress profile, not a production frame budget.

These numbers are evidence of a real deterministic path, not a quality score.
The canonical benchmark and Campaign 2 authored slice are intentionally small;
the quality-gap report records what is still missing for production breadth.

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

cargo run -q -p wge-native-graphics-contract -- render-showcase-layout \
  crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia \
  "/mnt/d/Code Projects/WGE/terrain_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab/bin/wge_graphics_worker.jl" \
  /tmp/wge-riverwatch-native-showcase.ppm

cargo run -q -p wge-native-graphics-contract -- render-world-showcase-layout \
  crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia \
  "/mnt/d/Code Projects/WGE/terrain_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab" \
  "/mnt/d/Code Projects/WGE/graphics_lab/bin/wge_graphics_worker.jl" \
  /tmp/wge-riverwatch-native-world-showcase.ppm

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

- Rust native graphics contract: 16 unit tests, clippy with `-D warnings`, and
  the seven-test native graphics suite passed after the foliage-material
  refresh; the promoted CLI captures are current.
- Julia protocol worker: 7 worker protocol tests, 2 fake-rejection tests, and
  5 degenerate-boundary tests passed (14 total).
- Julia Lava adapter: 26 persistent GPU tests plus focused color, lighting,
  importance, capture, projection, and overlay test sets passed (34 camera/
  overlay assertions).
- Reference runtime: 10 tests passed.
- Bevy viewer: 22 binary/unit tests passed.

## Interpretation and next benchmark improvements

This checkpoint establishes a repeatable native path, a measured 30-frame warm
distribution plus a current v6 close sample, named CPU/GPU pass timing, a
deterministic synthetic dense profile, close-range and showcase material
inspection profiles, and a composed authored-world camera-cut profile. It does
not establish a production frame budget, meshlet/streaming/occlusion breadth,
or imported hero-asset quality. Those remain measured gaps rather than hidden
claims. A future renderer campaign should separate uninstrumented-vs-
instrumented timings and add higher-fidelity terrain/vegetation, IBL, effects,
and asset-import paths only behind new typed evidence.
