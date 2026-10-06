# WGE Native Graphics Architecture

Status: supervised native Lava backend checkpoint green; adapter-v6 audit,
Campaign 2 authored-frame slice, C2.5 imported-asset inspection, and C2.7
authority-side mip-chain conditioning closed, with the registered
visual-quality floor green and broader production gaps still explicit.

Date: 2026-09-30

Upstream substrate: Lava.jl
`11c7e31bdf62408d22bf379e9e59510f69d2103e`, with matching Vulkan.jl
`03b4ca2351477ccbb8ee378f512da50f7eec7bac` and VulkanCore.jl
`1d02829e8fa92da430d879db4dd7bf564a872035`.

The companion forensic record is [`docs/archive/2026-09_native-graphics-checkpoints/lava-graphics-audit.md`](../archive/2026-09_native-graphics-checkpoints/lava-graphics-audit.md).
This document turns that audit into an executable boundary. The design is
deliberately narrow: WGE owns meaning and certification; Rust supervises and
promotes; Julia owns numerical/GPU execution inside a persistent process; Lava
owns Vulkan machinery behind one adapter.

## Non-negotiable ownership

| Concern | Owner | Rule |
| --- | --- | --- |
| Project/specification, authored layout, world identity | Rust authority plane | A graphics process never invents or mutates semantic world state. |
| Terrain/numerical fields | Julia terrain boundary | Existing Julia contracts remain authoritative for numerical fields. |
| GPU scene lowering and Lava calls | Graphics Julia process | It receives a typed packet and returns typed measurements/bytes. It does not receive per-draw semantic commands. |
| Process lifetime, packet identity, capability policy, restart | Rust supervisor | Crash, device loss, protocol mismatch, and unsupported capabilities are explicit outcomes. |
| Receipt promotion and independent validation | Rust authority plane | A producer string, status field, or screenshot never grants a pass. |
| Inspection/regression view | Bevy | Bevy remains a renderer/oracle for parity and inspection, never the canonical graphics representation. |
| Transport | Typed length-prefixed protocol | Python may orchestrate transport at a higher layer, but it is not semantic authority. |

There is no Rust-to-Julia call per draw, resource, or shader. One request
contains a complete coarse scene packet. One response contains backend
identity, capability facts, frame measurements, capture bytes or an explicit
failure, and provenance sufficient for Rust to revalidate it.

## Process topology

```text
Rust world/reference runtime
        |
        | validate WorldArtifact, lower one GraphicsScenePacket
        v
Rust graphics supervisor
        | length-prefixed request/response; timeout; restart; digest checks
        v
persistent Julia graphics worker
        | WGE adapter only
        v
Lava.jl pinned revision -> Vulkan device/context/queues -> offscreen target
```

The worker owns exactly one long-lived `VkContext` and its GPU resources. The
supervisor owns the worker process and the authoritative packet/capture
identity. A worker restart invalidates all GPU handles and forces a complete
scene re-upload; it cannot resume from an unproven partial frame. The current
Rust supervisor has an explicit restart path and the integration gate proves
that a clean restart reproduces the exact capture bytes.

The first implementation is offscreen and headless. A window/swapchain path is
diagnostic tooling only; the bounded capability probe proves one native present
but is not a persistent scene loop. The first certified frame is a fixed-size capture from
a fixed camera, fixed seed, fixed world packet, and fixed backend capability
set.

## Typed packet boundary

The current Rust contract is `wge.graphics-scene-packet/v6`. It is a closed,
`deny_unknown_fields` structure with a canonical body digest. Its shape is:

```text
GraphicsScenePacket
├── packet identity and world/spatial digests
├── coordinate convention and frame seed
├── camera + capture request
├── terrain field/region/material inputs
├── mesh and texture buffer references
├── material intent
├── instance transforms and closed semantic importance hints
├── light and sky/horizon/ground environment intent
└── gameplay-visible markers (route, spawns, encounters, objective)
```

Every buffer/texture reference carries an artifact ID, relative path or inline
payload identity, byte length, digest, format, stride/count where applicable,
and color-space meaning where applicable. The initial vertical slice may use
inline numeric fields for small fixtures; the contract already has the same
identity fields needed to move large buffers into candidate artifacts without
changing semantic meaning.

Mesh packets also carry an authored, finite `uv0` channel with one coordinate
pair per position/normal vertex. Rust validates its cardinality and numeric
domain before promotion; the Lava adapter uploads it as a typed `Vec2f`
buffer. Texture lookup therefore depends on explicit asset geometry rather than
an implicit world-position projection.

The packet is a lowering of a validated `WorldArtifact` and, when present, a
validated `SceneArtifact` plus conditioned render-asset projections; it is
never a replacement for those canonical artifacts. `compose_bound_scene`
performs the current Rust-owned composition step, namespaces imported mesh,
material, and texture identities, and records the scene artifact identity in
the packet. Its validator must check:

- exact schema and canonical digest;
- finite numeric values and bounded dimensions/counts;
- unique IDs and referential integrity;
- coordinate handedness, up-axis, units, and winding;
- world, layout, spatial-field, and gameplay identity equality;
- buffer sizes, strides, formats, and source digests;
- camera/capture dimensions and deterministic settings;
- non-degenerate, non-collinear camera bases and directional vectors;
- non-zero authored normals and unit instance quaternions;
- material ranges, texture color-space declarations, and alpha policy;
- marker positions against the certified world;
- no unknown fields or producer-supplied “passed” fields.

The worker boundary has a separate bound-scene promotion operation. It accepts
the authorized base packet, canonical scene artifact, runtime/render receipts,
and graphics projections; Rust independently validates the receipts,
recomposes the packet, and rejects any packet drift before Julia/Lava sees it.
The Julia execution parser accepts the optional scene identity pair as typed
transport metadata and rejects a half-present or malformed pair; it does not
promote that identity.

The current imported-asset bridge preserves conditioned tangent streams in the
canonical v6 `MeshPacket`. Rust validates the optional stream, Julia validates
its transport shape and finite/handedness constraints, and Lava consumes it for
imported normal-basis construction. Procedural packets use an explicit
deterministic fallback. C2.7 now also carries a validated, versioned RGBA8
mip-chain through the neutral projection: each level has explicit dimensions,
bytes, and a digest over the concatenated decoded levels. The current Lava
adapter rejects multi-level residency with a typed unsupported-capability
result until the GPU uploader and sampler LOD seam is implemented; it never
silently drops the lower levels. This closes authority-side mip conditioning
without implying GPU residency, full material-graph semantics, or production
asset quality.

The render-asset conditioner also lowers the supported glTF
`KHR_texture_transform` subset into canonical UV0 before tangent generation.
The graphics packet therefore remains backend-neutral: the renderer receives
the already-conditioned UV space, while Rust retains the source transform in
the validated render package. Alternate UV sets and conflicting per-role
transforms are rejected at the asset boundary.

The packet does not contain an arbitrary shader string, arbitrary Vulkan handle,
or arbitrary code callback. Shader/material lowering is selected by a
registered WGE renderer capability and a content-addressed intent identity.

## Backend and capture contracts

The worker response is also closed and typed. It has four possible classes:

- `ready`: process protocol, Lava revision, Julia version, Vulkan/device
  identity, capabilities, and adapter revision;
- `frame`: packet digest, capture request digest, render/capture status, output
  digest, measurable properties, telemetry, and capture bytes/artifact ref;
- `unsupported`: a capability was not available; no frame is promotable;
- `failed`: protocol, compile, device, validation, or runtime failure with a
  bounded diagnostic and no pass-shaped output.

The Rust side independently checks the response. Requests use a bounded response
deadline; timeout clears the validated worker capability state and requires a
fresh handshake after restart. At minimum Rust recomputes:

- packet and capture-request digests;
- output bytes and declared dimensions/format;
- visual measurements from the RGBA8 bytes and packet marker colors, rather
  than trusting producer claims;
- finite measurements and bounded telemetry;
- backend identity and exact Lava/source revisions;
- expected world/spatial/camera identities;
- deterministic replay by requesting the same frame again after a clean worker
  reset.

The canonical `rgba8_srgb` payload is row-major, top-to-bottom RGBA8. RGB
capture bytes use the standard scene-linear-to-sRGB transfer before
quantization; alpha remains linear. sRGB albedo payloads are decoded to linear
values before material evaluation. The Rust receipt stores the independently
recomputed measurements and the adapter reports render-through-readback time
in microseconds. When the selected Vulkan queue exposes valid timestamp bits,
the adapter brackets the graphics pass and named prepare, scene-raster,
resolve, and overlay phases with Vulkan timestamp writes. Rust bounds and
reconciles the resulting telemetry; a queue without that capability reports
the optional GPU fields as `null`. The required framebuffer readback is the
synchronization point; timestamp writes themselves do not claim to be an
uninstrumented production frame budget.

Visual evidence remains a real gate. The reference Rust capture and the Lava
capture are compared for semantic markers and measured properties first, then
for quality properties appropriate to the slice. A mismatch is an explicit
failure or indeterminate result; it is not downgraded to a warning because the
image “looks plausible.”

## First vertical-slice renderer

The first native frame is intentionally bounded:

1. certified terrain heightfield and semantic region colors/material layers;
2. deterministic terrain mesh and normals;
3. deterministic authored mesh UV0 channels and one canonical terrain material
   family with explicit color space;
4. a deterministic objective landmark beacon with separate albedo/emissive
   material identity and close-range perspective inspection support;
5. deterministic opaque background foliage cross-mesh instances using the
   semantic-importance/culling path;
6. certified route, player/opponent spawn, encounter, and objective markers;
7. fixed camera, linear-HDR offscreen scene target, internal depth attachment,
   deterministic 2× spatial resolve, and final color evidence target (depth
   evidence remains explicitly deferred);
8. one directional light plus typed sky/horizon/ground environment intent,
   orientation-aware analytic environment lighting, deterministic directional
   shadow map, and depth-tested opaque raster path;
9. capture, readback, Rust measurement, and repeatability evidence.

This proves the entire authority and process path without pretending that a
single triangle is an Elden Ring renderer. A separate
`render-showcase-layout` profile now derives a deterministic shrine composition
from the same objective anchor. It exercises typed stone, metal, normal,
roughness, occlusion, emissive, instancing, directional-shadow, perspective,
and HDR-resolve behavior without changing world authority or semantic overlays.
It is a visual-quality probe, not imported-asset parity or new gameplay state.
PBR breadth beyond the bounded material path, shadow quality beyond one fixed
directional map, prefiltered IBL, temporal techniques beyond the deterministic
spatial resolve, production foliage systems beyond the opaque diagnostic
cross-mesh, particles, water, post-processing, and optional RT come only after
this path has real evidence and a quality-gap report.

`render-close-layout` derives a second, deterministic perspective packet from
the same certified world and packet identities. It removes gameplay overlays
for material inspection, while the overview packet remains the gameplay and
semantic-visibility gate. This is an inspection profile, not a second semantic
world or a claim of imported hero-asset parity.

The C2.5 imported-asset inspection profile uses the same Rust-owned composition
seam with a derived `real-asset-close` camera. The supervisor independently
recomposes that camera-bound packet before promotion. Its restart proof repeats
the context warm-up before the close frame so structural GPU telemetry remains
part of the deterministic receipt identity; the evidence bundle is recorded in
`docs/archive/2026-09_native-graphics-checkpoints/c2-5-imported-asset-inspection-handoff.md`.

`render-showcase-layout` is the stronger visual inspection profile. Rust seals
its extra geometry and material identities, binds them to the source world and
spatial-field digests, and independently revalidates the resulting capture.
`render-world-showcase-layout` composes that probe with the real Riverwatch
instances for a wider packet, batching, and culling check. Neither profile is
claimed as imported hero-asset parity. The frozen evidence and remaining quality limits are recorded in
[`docs/archive/2026-09_native-graphics-checkpoints/native-showcase.md`](../archive/2026-09_native-graphics-checkpoints/native-showcase.md).

The strict certification command, `render-quality-layout`, uses that composed
world-showcase packet so the technical visual floor measures a materially
structured WGE scene while preserving the authored world and spatial-field
bindings. This is an internal WGE quality probe, not a comparison against any
other engine; a failed measurement remains a failed gate.

The quality authority reports terrain and authored geometry separately. The
latest green capture measured `2,281 bp` terrain coverage and `28,269`
authored-geometry pixels, then measured content-level color/edge/tile
structure over their union. This closes the projected-prop contamination
finding without granting geometry a terrain pass by relabeling it.

## Campaign 2 — The Authored Frame

Campaign 2 is the first coherent authored calibration slice on the native path.
It is deliberately a Rust-owned projection over the certified Riverwatch world,
not a second semantic world and not an imported-asset or AAA claim. The command
is `render-campaign2-layout`; its three closed view roles are `close`, `medium`,
and `wide`.

`lower_campaign2_packet` validates the source packet, binds the objective anchor
and source world/spatial-field identities, fences the sparse diagnostic render
instances out of the composition, and deterministically adds authored calibration
geometry. The semantic instances remain in the `WorldArtifact` and are covered
by the world/runtime gates; the derived packet cannot mutate semantic state.
The calibration packet contains:

- a textured and normally perturbed terrain surface;
- a high-tessellation bronze/slate/cyan hero landmark with pedestal, halo,
  inlays, and emissive spire;
- rounded, clustered foliage with explicit trunks and shadow-casting instances;
- a distinct raised wet/reflective puddle with concentric glints;
- fixed perspective cameras, warm directional light, sky/ground environment,
  fog, linear HDR resolve, and deterministic shadow-map sampling.

The Rust-registered `campaign2-authored-frame` profile is the technical floor
for these cuts. The separate `wge.campaign2-visual-evidence/v1` vector records
silhouette/readability, material separation, composition, texture frequency,
density, artifact rate, frame/GPU cost, upload/readback memory, and instance
counts. Grounding/contact, lighting consistency, and atmospheric depth are
explicitly `indeterminate` until the native path has depth/contact classifiers,
reference-light comparisons, and a deterministic atmospheric evaluator.
The visual critic can describe or propose a repair, but Rust alone promotes the
receipt and evidence.

The final current-binary run is preserved under
`artifacts/campaign2/live-twelfth/`; its clean replay is preserved under
`artifacts/campaign2/live-eleventh/`; and the viewable captures are under
`/home/mattc/Pictures/WGE/campaign2-2026-09-29/`. The full measurements,
hashes, rejected experiments, and next frontier are in
[`docs/archive/2026-10_graphics-sprint-reports/graphics-campaign-2-report.md`](../archive/2026-10_graphics-sprint-reports/graphics-campaign-2-report.md).

## Capability and fallback policy

Capabilities are facts, not guesses. The adapter reports Vulkan version,
required features, offscreen support, supported formats, device identity,
shader/compiler identity, and optional RT support. Rust selects a registered
capability profile.

Fallback is allowed only when the packet explicitly requests an optional
feature and the lower-quality profile has its own validator. There is no silent
semantic fallback, no substitution of a Bevy capture for a missing Lava frame,
and no “GPU unavailable” result represented as a passing fixture.

The required first profile is:

```text
offscreen raster + internal color/depth attachments + typed buffers +
texture/sampler binding; promoted evidence is color-only until the versioned
depth payload contract exists
```

Hardware RT is optional. If it is attempted and causes device loss, the
supervisor records the loss, tears down the worker, and returns a failed or
indeterminate receipt with recovery evidence. It does not reuse stale handles.

## Julia adapter rules

The graphics Julia project is separate from `terrain_lab` so Lava’s Julia 1.12
dependency does not silently change the terrain package’s compatibility promise.
The adapter is the only WGE module allowed to import Lava names. It must:

- use concrete domain types and meaningful multiple dispatch for packet
  lowering, material variants, target formats, and capability profiles;
- keep `LavaArray`/Vulkan handles out of the Rust protocol;
- compile/cache by stable WGE identities, not only Julia object hashes;
- make ownership and lifetime explicit for buffers, textures, pipelines, and
  readback memory;
- expose phase timing and transfer/draw/dispatch counts;
- reject malformed packets before device submission;
- return structured failure data on compile/device/readback failures;
- avoid per-frame FFI and avoid hidden global semantic state.

The existing terrain Julia code remains responsible for numerical fields. A
graphics worker may derive normals, meshlets, visibility, or render buffers
from a validated packet, but those are renderer products and must be digestible
and reproducible from the packet.

## Research track: Tetrahedral Cage RT

TetCageRT is an optional representation-lowering investigation for dense,
connectivity-preserving animated geometry. It is not semantic authority, not a
replacement for the authored mesh contract, and not part of the current native
raster certification gate. The canonical source mesh and conventional animated
BLAS path remain available for every comparison.

The central question is when WGE should materialize animated geometry as
tetrahedral cages. A possible hybrid policy—full BLAS near, cluster AS at
intermediate scale, and TetCageRT for far/dense geometry—is only a hypothesis
until measured.

The research sequence is:

1. pin and completely reconstruct the exact AMD paper/source, including its
   assumptions and known failure modes; keep its measurements separate from
   later terrain-demo measurements;
2. implement a deterministic CPU reference for cage generation, triangle
   clipping, barycentric encoding, deformation, and correctness visualization;
3. prototype a typed, optional Lava/Vulkan transformed-ray or static mini-BLAS
   path with explicit capability and failure reporting;
4. compare conventional animated BLAS, cluster AS, and TetCageRT using visual
   deformation error, AS memory, animation cost, AS update cost, trace cost,
   total frame time, preprocessing expansion, and cage resolution;
5. investigate watertight 4D barycentric representations, temporal/topological
   continuity, boundary behavior, numerical robustness, and adversarial
   deformation cases;
6. materialize a Rust-owned policy that selects TetCageRT only when legal,
   bounded, visually acceptable, and measurably profitable.

The reference must account for clipping-induced geometry expansion (roughly
1.3x–2.3x in the cited discussion), the trade between cage resolution,
deformation artifacts, and update cost, and the possibility that traversal
and intersection work costs more than the saved animation/AS update time.
Unknown paper identity, unsupported hardware, numerical uncertainty, a failed
watertightness check, or an unprofitable comparison is an explicit research
outcome rather than an invented implementation decision. No benchmark-only
geometry may be promoted as authored semantic evidence.

## Verification ladder

Each layer must be green before the next depends on it:

1. Rust contract unit tests: canonical identity, bounds, referential integrity,
   malformed/fake receipt rejection.
2. Julia protocol tests: framing, decode/encode, import safety, concrete packet
   types, fail-closed malformed input.
3. Lava adapter probes: device discovery, minimal shader, buffer upload,
   offscreen clear, depth, texture, readback, teardown.
4. Rust-supervised process tests: ready handshake, restart, schema/provenance
   checks, stale-packet rejection, and bounded response handling.
5. Native frame test: certified WGE world -> packet -> Lava frame -> Rust
   validation -> clean-restart deterministic replay.
6. Bevy parity test: same packet intent and camera produce semantically
   equivalent marker/terrain measurements; representation may differ.
7. Adversarial tests: forged status, wrong digest, stale packet, changed
   camera, malformed buffer, unsupported capability, injected visual failure,
   worker crash between upload and readback.

## Explicitly deferred

This checkpoint does not authorize:

- rigging, skinning, retargeting, or arbitrary mesh-to-character generation;
- multiplayer or universal engine parity;
- a new physics/navigation engine;
- broad text-to-3D generation;
- replacing the Rust reference runtime;
- post-MVP renderer breadth before the native vertical slice is certified.

The supplied bad GLB remains a permanent negative/rejection control in the
asset and receipt suites. Rigging-dependent evidence stays explicitly
deferred/indeterminate.

## Implementation order

1. Add the Rust packet/capability/capture contract and tests.
2. Add a minimal Julia graphics project pinned to the audited Lava revision and
   a protocol-only worker that can validate/echo packets without Vulkan.
3. Add Rust supervisor lifecycle and exact source/provenance checks.
4. Add Lava offscreen probe and first deterministic clear/terrain frame.
5. Lower the certified world and gameplay markers; promote Rust-owned visual
   evidence.
6. Add repair/rebuild and deterministic restart evidence.
7. Measure and improve visual quality only inside the certified boundary.

The Rust packet contract and protocol worker are implemented. The Lava adapter
has passed real offscreen color/readback, depth-attachment, and texture/sampler
probes through a persistent Julia process and renders the certified `riverwatch`
packet: terrain height/slope buffers, the canonical orthographic camera plus a
validated perspective camera variant,
terrain material intent, and gameplay-visible route/spawn/encounter/objective
overlays all cross the native path. The Rust supervisor validates the exact
Lava/adapter identity, packet binding, capture digest, bounded telemetry, and
independently recomputed visual measurements before promoting a receipt.

The supported scene profile is intentionally fail-closed while quality systems
are being built: the Lava adapter accepts one directional light, opaque
material intents with role-specific, content-addressed inline RGBA8 albedo,
normal, roughness, occlusion, and emissive roles, orthographic and perspective native
terrain projections, and typed sky/horizon/ground/fog/exposure intent. Instances also carry closed
`background`, `landmark`, or `gameplay_critical` importance, which controls a
typed culling margin and is independently balanced in the promoted telemetry.
The payload is dimension-checked and digest-checked in
both Rust and Julia before Lava creates sampler bindings. Point lights and
blend/mask materials are typed and validated at the boundary but rejected by
the adapter until their semantics are implemented. Metallic/roughness and
bounded clearcoat response,
role-sampled normal/roughness/
occlusion/emissive maps, and orientation-aware analytic
sky/ground environment lighting are supported by the bounded material path,
but it is not yet a complete production PBR graph or prefiltered image-based
lighting system. The current
directional profile renders a deterministic 512² shadow map from terrain and
light-frustum-visible instanced meshes, then applies four-tap percentage-closer
visibility in the main pass. Scene color stays linear HDR until a deterministic
2× four-sample resolve applies the exposure/tone-map boundary; semantic
overlays are composited afterward so their Rust-validated role colors remain
exact. Cascades, contact refinement, soft shadows, prefiltered IBL, temporal AA,
and many-light shadow budgets remain quality gaps. This keeps
the packet extensible
without silently rendering less than the model requested; prefiltered
environment maps, richer foliage systems, and richer material graphs remain
explicit quality-gap work.

The supervised integration test also restarts Julia and proves the same packet
produces byte-identical RGBA8 capture bytes after a clean GPU-context rebuild.
The `render-layout` CLI can emit an inspectable PPM plus the promoted receipt.
The receipt additionally binds the physical device UUID, worker-script digest,
and a renderer-identity digest over the validated capability and worker-ready
messages. Rust rejects flat/black captures, insufficient RGB diversity, and
missing pixels for any semantic marker role present in the packet; producer
measurements are compared against Rust’s independent recomputation. Frame
telemetry now carries packet-checked total and per-importance instance visibility,
terrain/mesh submission counts, total CPU/GPU frame timing, and optional
capability-gated per-pass timing, exposing a measurable
semantic-culling/performance surface. A separate Rust command can derive an
explicitly synthetic dense-foliage packet for scalability measurements; it
preserves the source packet and is never authored semantic evidence. Visible
mesh/material groups are submitted with Lava instancing: base triangle data is
uploaded once per deterministic batch and transforms/material parameters are
indexed per instance on the GPU. The integration fixture duplicates an
obstacle and verifies that submitted mesh vertices remain at the base-mesh
count.
The canonical gameplay/overview frame remains a coarse diagnostic slice:
terrain shading derives finite-difference normals from the certified height
field, overlays are line primitives, and the shadow profile is one fixed
directional map with four-tap percentage-closer sampling followed by a
deterministic spatial HDR resolve. Campaign 2 adds a separate authored
calibration projection with smoother hero/foliage geometry, conditioned
materials, a wet reflective probe, fixed composition cuts, and the same
authority path. Cascades, contact/soft shadows, prefiltered IBL, production
foliage density/alpha/LOD, particles, post-processing, and Elden-Ring-level
visual quality remain quality-gap work behind this boundary, not reasons to
weaken the evidence gate.
