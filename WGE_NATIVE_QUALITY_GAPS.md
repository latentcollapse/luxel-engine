# WGE Native Graphics Quality Gaps

Status: measured gap register, 2026-09-28

The native path is now real and supervised. The canonical overview remains a
diagnostic vertical slice, while the separate `render-showcase-layout` profile
now provides a deterministic, materially varied shrine composition for visual
quality inspection. This document prevents either profile from being mistaken
for an Elden Ring-level presentation.

## Measured or directly observed gaps

1. The canonical Riverwatch overview is still visually coarse: a small terrain
   field, simple obstacle geometry, diagnostic gameplay markers, and opaque
   crossed foliage. The new showcase profile demonstrates composition,
   procedural stone/metal/emissive roles, normal response, and shadowed
   perspective rendering, but it is still a bounded authored probe rather than
   imported hero-asset richness or production environment breadth.
2. The canonical material contract now has digest-bound albedo, normal,
   roughness, occlusion, and emissive roles plus scalar metallic/roughness and
   clearcoat controls. Alpha, transmission, texture transforms, mip policy, and
   material-graph identity are not yet represented.
3. Lighting uses one directional shadow map with fixed resolution and PCF.
   Cascades, contact shadows, richer soft-shadow filtering, many-light clustering,
   prefiltered image-based lighting, reflection probes, and robust atmospheric
   scattering remain absent.
4. The HDR resolve is deterministic and linear, but it is a spatial 2x resolve,
   not temporal anti-aliasing or a history-aware reconstruction pipeline.
5. Foliage proves typed instancing and semantic visibility accounting, not a
   production vegetation system. There is no alpha-tested foliage, wind,
   ecological placement model, impostor, LOD, streaming, or occlusion system.
6. There are no native water, particle, decal, volumetric, post-processing,
   skeletal animation, or character-rendering paths.
7. GPU scene scalability is now measured only for a synthetic instanced
   foliage stress profile: 512 added background instances (544 total) remain
   one bounded mesh batch, with 15 draw calls, 240 base mesh vertices, and a
   warm GPU p50/p95 of 477/504 microseconds under pass-level instrumentation.
   This does not establish authored-scene scalability, traversal-aware culling,
   meshlets, GPU-driven indirect draws, residency/streaming policy, or an
   occlusion hierarchy.
8. Cold startup is currently expensive: the current cold CLI sample took
   116.17 seconds and reached 1,693,752 KiB maximum RSS, including the
   Rust/Cargo boundary. The current formal 30-frame supervisor benchmark took
   28.965 seconds for its cold wall sample. The warm distribution reached
   289.520 ms wall-time p95 and 69.898 ms adapter-frame p95 under the current
   pass-instrumented adapter. Its graphics-pass GPU interval was 724 µs at p50,
   1,323 µs at p95, and 1,694 µs at p99 for this 15-draw diagnostic scene.
   Named pass timestamps add synchronization overhead, so this is not yet a
   production or dense-scene frame budget.
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

1. Replace or augment the bounded showcase composition with a real
   authored/imported hero asset/material packet with normal, occlusion, and
   emissive roles, then validate a textured close-range capture. The supplied
   bad GLB remains a rejection control and is not promoted by this step.
2. Replace the synthetic stress population with a denser authored scene or
   camera-cut profile, and separate instrumented timing from uninstrumented
   production timing before optimizing cold startup or choosing a frame budget.
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
