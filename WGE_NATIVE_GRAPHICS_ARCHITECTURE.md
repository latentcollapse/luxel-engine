# WGE Native Graphics Architecture

Status: supervised native Lava checkpoint green; high-quality renderer breadth
and engine-neutral parity remain ahead.

Date: 2026-09-28

Upstream substrate: Lava.jl
`11c7e31bdf62408d22bf379e9e59510f69d2103e`, with matching Vulkan.jl
`03b4ca2351477ccbb8ee378f512da50f7eec7bac` and VulkanCore.jl
`1d02829e8fa92da430d879db4dd7bf564a872035`.

The companion forensic record is [`LAVA_GRAPHICS_AUDIT.md`](LAVA_GRAPHICS_AUDIT.md).
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
diagnostic tooling only. The first certified frame is a fixed-size capture from
a fixed camera, fixed seed, fixed world packet, and fixed backend capability
set.

## Typed packet boundary

The first Rust contract is `wge.graphics-scene-packet/v1`. It is a closed,
`deny_unknown_fields` structure with a canonical body digest. Its shape is:

```text
GraphicsScenePacket
├── packet identity and world/spatial digests
├── coordinate convention and frame seed
├── camera + capture request
├── terrain field/region/material inputs
├── mesh and texture buffer references
├── material intent
├── instance transforms and semantic roles
├── light intent
└── gameplay-visible markers (route, spawns, encounters, objective)
```

Every buffer/texture reference carries an artifact ID, relative path or inline
payload identity, byte length, digest, format, stride/count where applicable,
and color-space meaning where applicable. The initial vertical slice may use
inline numeric fields for small fixtures; the contract already has the same
identity fields needed to move large buffers into candidate artifacts without
changing semantic meaning.

The packet is a lowering of a validated `WorldArtifact`, never a replacement
for it. Its validator must check:

- exact schema and canonical digest;
- finite numeric values and bounded dimensions/counts;
- unique IDs and referential integrity;
- coordinate handedness, up-axis, units, and winding;
- world, layout, spatial-field, and gameplay identity equality;
- buffer sizes, strides, formats, and source digests;
- camera/capture dimensions and deterministic settings;
- material ranges, texture color-space declarations, and alpha policy;
- marker positions against the certified world;
- no unknown fields or producer-supplied “passed” fields.

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

The Rust side independently checks the response. At minimum it recomputes:

- packet and capture-request digests;
- output bytes and declared dimensions/format;
- visual measurements from the RGBA8 bytes and packet marker colors, rather
  than trusting producer claims;
- finite measurements and bounded telemetry;
- backend identity and exact Lava/source revisions;
- expected world/spatial/camera identities;
- deterministic replay by requesting the same frame again after a clean worker
  reset.

The canonical `rgba8_srgb` payload is row-major, top-to-bottom RGBA8. The
Rust receipt stores the independently recomputed measurements and the adapter
reports render-through-readback time in microseconds.

Visual evidence remains a real gate. The reference Rust capture and the Lava
capture are compared for semantic markers and measured properties first, then
for quality properties appropriate to the slice. A mismatch is an explicit
failure or indeterminate result; it is not downgraded to a warning because the
image “looks plausible.”

## First vertical-slice renderer

The first native frame is intentionally bounded:

1. certified terrain heightfield and semantic region colors/material layers;
2. deterministic terrain mesh and normals;
3. one canonical terrain material family with explicit color space;
4. certified route, player/opponent spawn, encounter, and objective markers;
5. fixed camera and deterministic offscreen color/depth target;
6. one directional/ambient light intent and depth-tested opaque raster path;
7. capture, readback, Rust measurement, and repeatability evidence.

This proves the entire authority and process path without pretending that a
single triangle is an Elden Ring renderer. PBR breadth, shadow quality, IBL,
temporal techniques, foliage, particles, water, post-processing, and optional
RT come only after this path has real evidence and a quality-gap report.

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
offscreen raster + color/depth + typed buffers + texture/sampler binding
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
- Unity integration or conventional Unity+MCP comparison;
- multiplayer or universal engine parity;
- a new physics/navigation engine;
- broad text-to-3D generation;
- replacing the Rust reference runtime;
- post-MVP renderer breadth before the native vertical slice is certified.

The supplied bad GLB remains a permanent negative/rejection control in the
asset and receipt suites. Rigging-dependent and Unity-dependent evidence stays
explicitly deferred/indeterminate.

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
packet: terrain height/slope buffers, the canonical orthographic camera,
terrain material intent, and gameplay-visible route/spawn/encounter/objective
overlays all cross the native path. The Rust supervisor validates the exact
Lava/adapter identity, packet binding, capture digest, bounded telemetry, and
independently recomputed visual measurements before promoting a receipt.

The supported scene profile is intentionally fail-closed while quality systems
are being built: the Lava adapter accepts one directional light, opaque
non-metallic material intents without scene-texture bindings, an orthographic
native terrain projection, and typed sky/fog/exposure intent. Perspective
cameras, point lights,
textured materials, blend/mask materials, and metallic materials are typed and
validated at the boundary but rejected by the adapter until their semantics
are implemented. This keeps the packet extensible without silently rendering
less than the model requested.

The supervised integration test also restarts Julia and proves the same packet
produces byte-identical RGBA8 capture bytes after a clean GPU-context rebuild.
The `render-layout` CLI can emit an inspectable PPM plus the promoted receipt.
The receipt additionally binds the physical device UUID, worker-script digest,
and a renderer-identity digest over the validated capability and worker-ready
messages. Rust rejects flat/black captures, insufficient RGB diversity, and
missing pixels for any semantic marker role present in the packet; producer
measurements are compared against Rust’s independent recomputation. Frame
telemetry now carries packet-checked instance visibility and terrain/mesh
submission counts, exposing a measurable culling/performance surface.
The inspected frame is intentionally only a coarse diagnostic slice: terrain
shading is height/slope-based, overlays are line primitives, and the path does
not yet claim PBR, shadows, IBL, foliage, particles, post-processing, or
Elden-Ring-level visual quality. Those are quality-gap work behind this native
authority boundary, not reasons to weaken the current evidence gate.
