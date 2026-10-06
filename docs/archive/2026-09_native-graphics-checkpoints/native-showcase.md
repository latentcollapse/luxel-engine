# WGE Native Graphics Showcase Evidence

Status: deterministic native visual-quality probe; adapter-v6 evidence closed,
with the v5 receipt retained as a historical control, 2026-09-29

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

## Composed-world stress profile

`render-world-showcase-layout` keeps the authored Riverwatch obstacle and
foliage instances alongside the shrine instead of retaining only the
objective. The profile is useful for testing packet size, batch grouping,
semantic importance accounting, shadow coverage, and a wider perspective
camera. It is deliberately not promoted as the high-fidelity scene: the
current crossed foliage remains sparse and the terrain is still a small
diagnostic field. The first accepted v5 run carried 40 instances (30
background, 9 landmark, 1 gameplay-critical), 28 draw calls, 4,656 submitted
mesh vertices, and a deterministic 768x512 capture. That result closes a
composition-path coverage gap while leaving the measured vegetation/terrain
quality gap visible. The current foliage-material camera-cut replay is bound to packet
`sha256:28a91341960a2b67057a5266f0c538f0ca1510bd344d8f6508a56fba987cb2b5`
and capture `sha256:e68989b5d54a73117fbdc4da7829a27ee3edb6802f67b3c5ca13a1880fd02e33`.

## Current adapter-v6 evidence

The fresh 2026-09-29 material showcase promoted packet
`sha256:bd96a891e590b84aec8e61f9a27a01efc81bd98c09338df28235fa82c2ad119b`
and capture
`sha256:fda34e0e82898ae29505f854b3374099e11ebf77ca92643e8b5c0056a3790332`.
It rendered 9 landmark instances at 640x480 with 20 draw calls, 4,608 mesh
vertices, and 547,153 uploaded bytes. The composed-world v6 profile promoted
packet
`sha256:ab8162df9af469c48da81747fa9c2691230c50c2ea31be10a201698e17139183`
and capture
`sha256:7d11a5caecc99c4f0b03f1f83f02105c938c1bee901ab59843d98172f54934b1`;
it rendered 40 instances at 768x512 with 24 draw calls and 4,656 mesh
vertices. Both captures passed the Rust-owned visual and telemetry validators.

## Frozen evidence

The accepted v5-adapter replay used:

- packet: `sha256:dac4f498b67e09239c9c2b34c794dc40c8d8d32832fe4f23cc9fee27cf96a983`;
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
