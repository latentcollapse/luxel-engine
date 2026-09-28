# WGE Lava Graphics Audit

Status: Phase 0 complete; renderer implementation has not started.

Date: 2026-09-28

Audited WGE checkpoint: `f8fee90` (`harden Julia numerical and protocol contracts`)

Audited upstream:

- repository: [SimonDanisch/Lava.jl](https://github.com/SimonDanisch/Lava.jl)
- exact commit: [`11c7e31bdf62408d22bf379e9e59510f69d2103e`](https://github.com/SimonDanisch/Lava.jl/tree/11c7e31bdf62408d22bf379e9e59510f69d2103e)
- commit date: 2026-08-06
- upstream `main` and `HEAD` resolved to this commit during the audit

This document is the pre-implementation forensic record required by the WGE
native-graphics goal. It separates capabilities demonstrated by source or
tests from capabilities merely claimed by upstream documentation. The word
“renderer” below means a game-facing rendering system; Lava itself is a
low-level Julia/Vulkan graphics, compute, and ray-tracing substrate.

## Audit method

The audit used the pinned upstream checkout, its `Project.toml`, README,
known-issues record, source tree, graphics and ray-tracing tests, and the WGE
working tree. The important upstream evidence is linked directly:

- [upstream package manifest](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/Project.toml)
- [upstream API and exports](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/src/Lava.jl)
- [graphics API](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/src/graphics/api.jl)
- [graphics pipeline implementation](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/src/graphics/pipeline.jl)
- [offscreen framebuffer](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/src/graphics/framebuffer.jl)
- [window and swapchain](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/src/graphics/window.jl)
- [textures and samplers](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/src/graphics/textures.jl)
- [Vulkan device/runtime](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/src/runtime/device.jl)
- [command batching and synchronization](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/src/runtime/command.jl)
- [Julia-to-SPIR-V graphics emitter](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/src/compiler/spirv/graphics.jl)
- [hardware acceleration structures](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/src/raytracing/hwtlas.jl)
- [upstream test harness](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/test/runtests.jl)
- [graphics integration tests](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/test/test_graphics_pipeline.jl)
- [graphics phase tests](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/test/test_phase6_graphics.jl)
- [compute capture/replay tests](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/test/test_capture_replay.jl)
- [known upstream issues](https://github.com/SimonDanisch/Lava.jl/blob/11c7e31bdf62408d22bf379e9e59510f69d2103e/KNOWN_ISSUES.md)

Evidence labels used in the matrices:

- **EXISTS UPSTREAM** — present in source and exercised by a relevant test or
  direct implementation inspection.
- **EXISTS IN WGE** — already exists in WGE as an authority-owned contract or
  implementation, though it may not yet be connected to Lava.
- **PARTIAL** — some machinery exists, but the game-facing contract, coverage,
  or verification boundary is incomplete.
- **MISSING** — required by the goal and not present in the audited scope.
- **DO NOT NEED** — deliberately outside the native graphics goal.

README benchmark numbers and upstream “feature complete” statements are not
treated as WGE evidence. They remain useful leads, but WGE promotion requires
our own reproducible probes and Rust-owned receipts.

## Upstream Lava capability matrix

| Capability | Evidence found | Status | WGE interpretation |
| --- | --- | --- | --- |
| Julia GPU compute backend | `LavaBackend`, `LavaArray`, KernelAbstractions and GPUArrays adapters; GPU array tests | EXISTS UPSTREAM | Good numerical/scene-preparation substrate; it does not define WGE semantics. |
| Unified GPU buffers | `LavaArray` is usable by compute, graphics, and RT paths; buffer-device-address plumbing is present | EXISTS UPSTREAM | Suitable for persistent scene buffers once WGE defines layouts and lifetime rules. |
| Vulkan device/context ownership | `VkContext` owns device state, queues, caches, diagnostics, and device-lost state | EXISTS UPSTREAM | Keep one long-lived Julia graphics owner behind a Rust supervisor; never let WGE call Vulkan through per-draw FFI. |
| Queue submission and synchronization | `BatchQueue` contains command batching, timeline semaphores, staging, deferred frees, indirect slabs, and queue policy | EXISTS UPSTREAM | Reuse behind an adapter, but make WGE frame/packet boundaries explicit and observable. |
| CPU/GPU transfers and memory policy | staging/BAR paths, GPU-to-GPU copies, deferred retirement, pinned references | EXISTS UPSTREAM | The adapter must expose residency and transfer telemetry rather than hiding it in a black box. |
| Batched and indirect dispatch/draw | exported indirect commands and indirect dispatch/draw paths | EXISTS UPSTREAM | Candidate machinery for WGE instance and visibility batches; culling policy is still WGE work. |
| Julia-to-SPIR-V compilation | custom Julia/LLVM/SPIR-V emitter with graphics, compute, and RT stages | EXISTS UPSTREAM | Strong reason to use Lava; shader compilation must be isolated behind a stable WGE shader identity. |
| Vertex and fragment stages | typed graphics pipelines, varyings, vertex/fragment compilation, offscreen draw tests | EXISTS UPSTREAM | Enough for the first raster path, not enough for a complete material/lighting system. |
| Geometry and tessellation stages | exported stage types and compiler support | EXISTS UPSTREAM | Available, but not a reason to make them canonical for WGE. |
| Specialization and pipeline caching | type-driven compilation and per-context pipeline caches | PARTIAL | The existing cache key is derived from Julia function/type/state hashes; WGE needs content-addressed shader/material/pipeline identities and invalidation rules. |
| Swapchain/window path | `RenderWindow`, GLFW surface, swapchain, acquire/present, resize, per-image synchronization | EXISTS UPSTREAM | Useful for interactive probes, but not the canonical evidence path. |
| Offscreen rendering | `LavaFramebuffer`, color/depth attachments, readback, blit, deterministic-sized target support | EXISTS UPSTREAM | Use offscreen targets for WGE captures; window presentation remains optional tooling. |
| Textures, samplers, descriptors | `LavaTexture2D`, `LavaSampler`, texture bindings and descriptor setup | EXISTS UPSTREAM | Low-level binding exists; WGE still needs canonical texture/material asset contracts, color space, mip, streaming, and provenance policy. |
| Raster state | depth, blending, culling, topology, attachments, viewport/scissor-related state | EXISTS UPSTREAM | Sufficient primitives for a first forward/deferred experiment; WGE render intent is missing. |
| PBR/material graph | Hikari has material/shading code in its own path, but Lava graphics has no WGE material model or game scene material graph | MISSING | Build a narrow WGE material intent and lower it to Lava. Do not make Hikari’s scene representation canonical. |
| Lights, shadows, IBL, HDR, AA, fog | no game-renderer-owned canonical system found in Lava graphics | MISSING | Implement only the vertical-slice subset behind typed WGE intent; retain a quality-gap ledger for the rest. |
| Terrain, foliage, decals, particles, water, post-processing | no WGE/game-level implementation in Lava | MISSING | These are downstream renderer systems, not upstream Lava features. Start with terrain plus instancing and a measurable capture path. |
| GPU scene, culling, LOD, meshlets, streaming | indirect and buffer primitives exist; no WGE scene/culling/LOD/streaming policy | PARTIAL | WGE must own semantic visibility, LOD, residency, and capture priorities. |
| BLAS/TLAS and hardware RT | `HardwareAccel`, HWTLAS, BLAS/TLAS update/refit paths, RT shader pipeline, Raycore compatibility | EXISTS UPSTREAM | Optional capability lane, fail-closed; not a prerequisite for the first certified raster vertical slice. |
| Hikari/Raycore integration | Raycore source entry and Hikari-oriented integration/tests/examples | EXISTS UPSTREAM | Evidence that RT is viable, not evidence that a WGE gameplay renderer exists. Keep it behind an optional adapter. |
| Profiling and diagnostics | state dumps, logging, validation configuration, memory counters, phase timing, profiling hooks | EXISTS UPSTREAM | Connect selected telemetry to Rust receipts; avoid promoting opaque upstream logs as evidence. |
| API stability | upstream README calls compute stable and graphics/RT functional but evolving | PARTIAL | Pin the exact commit and isolate all upstream API use in a narrow Julia adapter. |
| Dependency reproducibility | Lava commit is known, but `Project.toml` sources Raycore at `rev = "master"` | PARTIAL | A WGE lock must pin the complete transitive source graph, especially Raycore and Vulkan source choice. |
| Test reproducibility | tiered test harness distinguishes SPIR-V/no-GPU/GPU, but requires an umbrella environment and warns that plain `Pkg.test` is insufficient | PARTIAL | WGE needs capability probes and explicit `indeterminate` results; “test suite ran” is not a GPU certification. |
| Capture/replay | upstream capture/replay tests cover compute command replay; no WGE visual-capture receipt or graphics replay contract | PARTIAL | WGE must add deterministic graphics capture, scene identity, camera identity, device identity, and Rust revalidation. |
| Device/vendor robustness | lavapipe and self-hosted GPU paths exist; known RADV RT device-loss and BDA validation blind spots remain | PARTIAL | Treat device capability and known-driver hazards as first-class gates. Never silently fall back to a different semantic result. |

### Upstream hazards that affect adoption

1. **The upstream revision is not a complete lock.** Lava itself is pinned for
   this audit, but its `[sources]` entry points Raycore at `master`. The WGE
   graphics environment must resolve and record every source revision before a
   renderer receipt can claim reproducibility.

2. **The public graphics/RT API is explicitly evolving.** WGE must have one
   narrow adapter module. No semantic WGE code may import Lava types directly.

3. **GPU-assisted validation is not a complete memory-safety oracle.** The
   upstream known-issues record documents that BDA out-of-bounds writes can
   escape validation. WGE therefore needs bounds/layout checks before dispatch,
   deterministic sentinels, and Rust-side result validation.

4. **RT has a documented RADV device-loss sequence.** The failure follows many
   Hikari software-path dispatches and then a hardware RT dispatch. RT cannot be
   an implicit requirement for the first certified path, and device loss must
   cause a supervised restart plus an indeterminate/failed receipt rather than
   a successful-looking capture.

5. **The graphics pipeline cache is not a WGE asset identity system.** Julia
   function/type/state hashing is useful for process-local reuse but is not by
   itself a durable content address. WGE will bind shader source/specification,
   material intent, pipeline state, Lava revision, device identity, and compiler
   configuration into a canonical identity.

6. **The upstream package requires a newer Julia environment than the terrain
   package currently promises.** Lava’s manifest requires Julia 1.12 through its
   dependency graph, while `terrain_lab` remains compatible with Julia 1.10.
   The least surprising initial design is a separate graphics Julia project and
   process, with typed packets between terrain/semantic Julia and graphics
   Julia. Upgrading the terrain environment is not authorized by this audit.

7. **Window rendering is not evidence rendering.** A swapchain and GLFW window
   are useful for probes, but WGE certification needs headless/offscreen,
   fixed-size, fixed-camera, fixed-seed captures with independent validation.

## WGE capability and assumption matrix

The current WGE authority split is documented in
[`WGE_NATIVE_CONVERGENCE_REPORT.md`](WGE_NATIVE_CONVERGENCE_REPORT.md): Julia
owns numerical terrain fields, Rust owns the certified world and reference
runtime, and Bevy is an inspection renderer rather than semantic authority.

| WGE concern | Current evidence | Status | Lava/native consequence |
| --- | --- | --- | --- |
| Semantic world artifact | Rust `wge.world-artifact/v1` contains validated layout, Julia provenance, terrain fields, collision, navigation, spawns, and encounters | EXISTS IN WGE | This remains the canonical scene input. Lava consumes a lowered packet, never authors it. |
| Julia numerical ownership | `terrain_lab` produces typed terrain/erosion/placement outputs; recent hardening removed hot-loop `Any` vectors and tightened fail-closed policies | EXISTS IN WGE | Preserve this boundary. Graphics Julia must not become a second semantic solver. |
| Rust authority/revalidation | reference runtime validates artifacts, gameplay, and capture provenance | EXISTS IN WGE | Rust owns native receipt promotion and must independently revalidate graphics evidence. |
| Bevy inspection path | `world_viewer` loads the certified artifact and presents native worlds; it has terrain shaders, materials, foliage projection, HUD, and PNG provenance | EXISTS IN WGE | Keep Bevy as a regression/inspection oracle, not as Lava’s representation or a canonical scene graph. |
| Engine-neutral visual evidence | Rust reference runtime produces deterministic PPM-style captures and validates metrics/bytes; Bevy produces PNG plus provenance | PARTIAL | Extend the same authority boundary to Lava captures. Do not replace the existing gates with screenshots. |
| Native terrain geometry | Rust builds a one-metre inspection mesh from the certified f32 heightfield | EXISTS IN WGE | Lower the same semantic terrain into a Lava GPU mesh; preserve field digest and coordinate convention. |
| Terrain material intent | Bevy native/compiled paths have splat/grass/road/rock/snow/wetland inputs and a terrain shader | PARTIAL | The intent is present in a backend-specific form. Extract a small canonical material packet before writing Lava shaders. |
| Mesh/material/texture asset contract | glTF/GLB placements and renderer-relative texture paths exist, but there is no unified engine-neutral GPU asset graph | PARTIAL | Define asset identities, formats, coordinate systems, color spaces, mip policy, and source provenance. |
| Lighting and camera | fixed inspection views, camera transforms, ambient/sky lighting and Bevy capture identity exist | PARTIAL | Define engine-neutral camera/light/capture intent and lower it to both Bevy and Lava for parity testing. |
| Gameplay-visible rendering | reference runtime owns collision/navigation/spawns/encounters/traversal; viewer displays the world and overlays but is not a complete gameplay renderer | PARTIAL | The first Lava slice must render at least terrain, route, spawn/encounter markers, and a gameplay-relevant camera state. |
| Navigation/collision semantics | Rust-owned artifact and runtime already validate these | EXISTS IN WGE | Lava must consume visual projections of them; it must not recompute authoritative traversal. |
| Capture provenance | Bevy sidecar binds PNG to world/spatial/camera/view/renderer identity; Rust reference capture validates bytes and metrics | EXISTS IN WGE | Add a Lava renderer identity, packet digest, shader/material identities, device capabilities, and capture digest. |
| Reference runtime | deterministic Rust runtime can run the current world/gameplay checks | EXISTS IN WGE | Keep as a semantic oracle. A Lava frame cannot make a runtime failure pass. |
| Python ownership | Python is transport/orchestration glue; no new semantic behavior is authorized | EXISTS IN WGE | Graphics protocol must be Rust-owned/Julia-owned typed data; Python remains out of semantic authority. |
| Bevy canonical leakage | viewer code has explicit native separation and comments rejecting Bevy as authoring authority | PARTIAL | Audit every new packet and receipt for Bevy-specific types, shader assumptions, or asset paths. |
| Target-engine export | optional adapters exist, but Unity/engine comparison is intentionally deferred | DO NOT NEED | This goal is the native WGE graphics path, not engine parity or Unity benchmarking. |
| Rigging/skinning/retargeting | intentionally deferred by the current certification goal | DO NOT NEED | Keep the supplied bad GLB as a permanent negative control in the asset/receipt suite. |

## What Lava gives WGE, and what it does not

Lava is a credible candidate for WGE’s native GPU substrate because it already
solves difficult low-level problems: a Julia-to-SPIR-V path, Vulkan resource
ownership, unified device buffers, queue synchronization, offscreen graphics,
and optional hardware RT. That is the part that was standing in the corner
looking suspiciously overqualified.

It does not give WGE the game-development harness by itself. In particular, it
does not define:

- a semantic project/specification intake;
- a canonical world, asset, material, light, camera, or capture schema;
- terrain meaning, navigation, encounter semantics, or gameplay state;
- mesh/texture provenance and asset preparation policy;
- GPU scene organization, visibility, LOD, residency, or streaming policy;
- deterministic evidence promotion;
- repair diagnosis and bounded mutation;
- a quality bar or comparison oracle for a real game scene.

The architectural conclusion is therefore **Lava as a supervised native
backend**, not “replace WGE’s semantic authority with Lava.”

## Required native work after this audit

These are the minimum implementation slices implied by the evidence, ordered so
that no later slice can smuggle in an unverified representation:

1. **Freeze the environment.** Record the Lava commit and all transitive source
   revisions, including Raycore; define the separate Julia 1.12 graphics
   environment; add capability probes for Vulkan, offscreen rendering, device
   features, and headless operation.
2. **Define the packet boundary.** Add a versioned, typed Rust-owned scene and
   capture contract. Packets contain coarse arrays and stable identities, not
   per-draw FFI calls. The packet must be sufficient to reconstruct the same
   frame after a process restart.
3. **Build the persistent supervisor.** Rust starts, health-checks, supervises,
   and can revive one long-lived graphics Julia process. Crash/device-loss,
   unsupported capabilities, and protocol mismatch are explicit non-success
   outcomes.
4. **Implement a narrow Lava adapter.** Keep all upstream API use in one Julia
   module. Add probes for buffer upload, shader compilation, offscreen color and
   depth, texture/sampler binding, batched instances, readback, and teardown.
5. **Lower engine-neutral intent.** Extract a small canonical terrain/material/
   camera/light contract from the existing Bevy path; implement one Lava
   raster path and a Bevy lowering for parity.
6. **Add Rust-owned visual receipts.** Captures bind world/packet/shader/material/
   camera/device identity and are independently decoded, hashed, re-measured,
   and promoted by Rust. A visual failure is a failed gate.
7. **Add semantic optimization telemetry.** Measure transfer bytes, resident
   buffers, draw/dispatch counts, culling, instance counts, LOD decisions,
   capture timing, and recovery behavior.
8. **Only then add quality breadth.** PBR, shadows, IBL/HDR, temporal AA, fog,
   foliage, decals, particles, water, post-processing, and optional RT are
   added behind the same contracts and quality-gap report.

## Phase 0 exit record

Completed:

- pinned and inspected the exact upstream Lava revision;
- inspected upstream graphics, runtime, compiler, texture, RT, test, and issue
  surfaces;
- mapped WGE’s existing semantic, reference-runtime, Bevy, and provenance
  boundaries;
- recorded adoption hazards and the Julia-version split;
- produced this implementation matrix before starting renderer construction;
- committed the preceding Julia quality hardening separately as `f8fee90`.

Not claimed:

- no Lava dependency has been added to WGE;
- no GPU device or driver has been certified for WGE;
- no native Lava frame has been promoted;
- no visual-quality gate has been weakened or marked passed by fixture;
- no rigging, Unity integration, engine comparison, or post-MVP breadth has
  started.

The next checkpoint is the architecture/contract document. It must preserve
the conclusions above, especially the Rust authority plane, separate Julia
graphics process, exact dependency lock, engine-neutral packet boundary, and
Bevy-as-oracle rule.
