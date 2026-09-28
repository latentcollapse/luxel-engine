# WGE Native Graphics Quality Gaps

Status: measured gap register, 2026-09-28

The native path is now real and supervised, but the current frame is a
diagnostic vertical slice rather than an Elden Ring-level presentation. This
document prevents the substrate milestone from being mistaken for the final
visual bar.

## Measured or directly observed gaps

1. The Riverwatch Lava capture is still visually coarse: a small terrain field,
   a bounded octagonal objective beacon, simple obstacle geometry, diagnostic
   gameplay markers, and opaque crossed foliage. The beacon’s close-range
   profile proves typed mesh/material/emissive inspection, but luminance and
   color-diversity gates do not establish imported hero-asset richness,
   silhouette quality, or composition.
2. The canonical material contract now has digest-bound albedo, normal,
   roughness, occlusion, and emissive roles plus scalar controls. Alpha,
   clearcoat, transmission, texture transforms, mip policy, and material-graph
   identity are not yet represented.
3. Lighting uses one directional shadow map with fixed resolution and PCF.
   Cascades, contact shadows, soft-shadow filtering, many-light clustering,
   prefiltered image-based lighting, reflection probes, and robust atmospheric
   scattering remain absent.
4. The HDR resolve is deterministic and linear, but it is a spatial 2x resolve,
   not temporal anti-aliasing or a history-aware reconstruction pipeline.
5. Foliage proves typed instancing and semantic visibility accounting, not a
   production vegetation system. There is no alpha-tested foliage, wind,
   ecological placement model, impostor, LOD, streaming, or occlusion system.
6. There are no native water, particle, decal, volumetric, post-processing,
   skeletal animation, or character-rendering paths.
7. GPU scene scalability is unmeasured. There are no meshlets, GPU-driven
   indirect draws, residency/streaming policy, occlusion hierarchy, or dense
   scene stress benchmark.
8. Cold startup is currently expensive: the current cold CLI sample took
   127.20 seconds and reached 1,933,720 KiB maximum RSS, including the
   Rust/Cargo boundary. The formal 30-frame supervisor benchmark took 25.361
   seconds for its cold wall sample. The warm distribution reached 301.552 ms
   wall-time p95 and 61.933 ms adapter-frame p95. Its graphics-pass GPU interval
   was 55 µs at p50, 57 µs at p95, and 213 µs at p99 for this 15-draw
   diagnostic scene; this is not yet a dense-scene frame budget.
9. The native capture path renders a validated gameplay/world snapshot. The
   Rust reference runtime completes the 78-step traversal and wins gameplay,
   but Lava is not yet the renderer attached to a live native input/update
   loop.
10. Bevy produced a provenance-valid native inspection PNG, but its current
    capture is flat/coarse for this packet. It is retained as a regression and
    inspection oracle; it is not presented as visual parity evidence for Lava.

## Explicitly deferred gates

- character rigging, skinning, and retargeting;
- arbitrary mesh-to-character generation;
- the supplied bad GLB negative/rejection control remains permanently
  negative;
- Unity import, build, and playthrough;
- conventional Unity + MCP benchmarking.

These are deferred gates, not silently passed features. No certification report
may promote them based on a world snapshot or a status-only receipt.

## Priority order for closing gaps

1. Replace the bounded procedural beacon with a real authored/imported hero
   asset/material packet with normal, occlusion, and emissive roles, then
   validate a textured close-range capture. The supplied bad GLB remains a
   rejection control and is not promoted by this step.
2. Add a denser stress scene and per-pass GPU timing breakdown before
   optimizing cold startup or choosing a frame budget.
3. Exercise traversal-aware semantic culling with landmark and
   gameplay-critical visibility, camera cuts, and a stressable population.
4. Add prefiltered IBL and higher-quality shadow strategy behind typed packet
   contracts and independent visual gates.
5. Attach the renderer to a reference gameplay update/input loop only after
   snapshot rendering and evidence promotion remain deterministic.
6. Add LOD/residency/streaming and effects systems only when their semantic
   ownership and receipt contracts are clear.

The next quality slice should improve one high-value visual axis at a time and
keep every new claim independently revalidated by Rust authority.
