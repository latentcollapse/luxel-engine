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

The accepted v5-adapter replay used:

- packet: `sha256:6927c97d25e481833e4a594cfabdef319b132bbc69d5d7e06713fdaa43c17bc8`;
- capture: `sha256:e8f960e61145f44c5803a24cfe5e3c2d9b19ac6d4df0dc59c04a2e1dd2172b27`;
- capture size: 640x480 RGBA8 sRGB;
- adapter identity: `wge.lava-adapter/v5`;
- Rust measurements: luminance standard deviation `0.1382377132`,
  `12,636` distinct RGB colors;
- promoted telemetry: 24 draw calls, 9 visible landmark instances, 4,608
  submitted mesh vertices, 399,953 uploaded bytes, and 1,229,824 readback
  bytes.

A fresh-process replay produced byte-identical PPM bytes and the same capture
digest. The authoritative frame receipt was promoted only after Rust decoded,
hashed, and remeasured the returned bytes; the Julia producer's status was not
used as evidence.

## What this proves

This is a real native PBR-style material/rendering probe: multiple typed
material identities, albedo and role textures, normal perturbation, scalar
metalness/roughness/occlusion/clearcoat, emissive response, directional shadows, camera
perspective, depth-tested instancing, deterministic HDR resolve, and
authority-owned visual promotion all cross the Rust/Julia/Lava boundary.

It does not prove imported hero-asset quality, temporal AA, prefiltered IBL,
many-light rendering, production shadow cascades, skeletal animation, or
Elden-Ring-level breadth. Those remain explicit quality gaps rather than being
hidden behind this probe.
