# WGE Native Graphics Showcase Evidence

Status: deterministic native visual-quality probe, 2026-09-28

The canonical Riverwatch overview remains the semantic/gameplay evidence
profile. `render-showcase-layout` derives a separate, engine-neutral visual
composition from the same validated packet and world artifact. It is intended
to exercise the renderer's quality surface without inventing gameplay state or
silently promoting a benchmark fixture as authored world meaning.

## Composition

Rust lowers a bounded shrine around the certified objective anchor:

- a multi-tier radial stone plinth and two profiled stone columns;
- a stone lintel with deterministic UVs and procedural albedo variation;
- bronze/metal halo and collar geometry;
- a cyan emissive rune ring and beacon inlay;
- the existing objective beacon with a showcase material override;
- one typed directional light, analytic environment response, normal/roughness/
  occlusion roles, directional shadowing, linear HDR resolve, and readback.

The composition is a visual probe, not a replacement world. It keeps the
source `world_artifact_id`, `world_artifact_sha256`, and
`spatial_fields_sha256`, clears gameplay overlays for inspection, and retains
only the objective anchor plus explicitly named showcase instances. Rust seals
and independently validates the resulting packet before the Julia/Lava worker
sees it.

## Frozen evidence

The accepted replay used:

- packet: `sha256:362d3726ffe7a74a6fb9ef2ef5585d350948d25235153367ed3344271a330424`;
- capture: `sha256:468e0612d1fe6e8ef09abd6329ed0ee984698a41acb67ed04cb5a24cb278ca3e`;
- capture size: 640x480 RGBA8 sRGB;
- Rust measurements: luminance standard deviation `0.1336471737`,
  `12,638` distinct RGB colors;
- promoted telemetry: 24 draw calls, 9 visible landmark instances, 4,608
  submitted mesh vertices, 399,665 uploaded bytes, and 1,229,824 readback
  bytes.

A fresh-process replay produced byte-identical PPM bytes and the same capture
digest. The authoritative frame receipt was promoted only after Rust decoded,
hashed, and remeasured the returned bytes; the Julia producer's status was not
used as evidence.

## What this proves

This is a real native PBR-style material/rendering probe: multiple typed
material identities, albedo and role textures, normal perturbation, scalar
metalness/roughness/occlusion, emissive response, directional shadows, camera
perspective, depth-tested instancing, deterministic HDR resolve, and
authority-owned visual promotion all cross the Rust/Julia/Lava boundary.

It does not prove imported hero-asset quality, temporal AA, prefiltered IBL,
many-light rendering, production shadow cascades, skeletal animation, or
Elden-Ring-level breadth. Those remain explicit quality gaps rather than being
hidden behind this probe.
