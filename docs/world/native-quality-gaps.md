# WGE Native Graphics Quality Gaps

Status: measured gap register, 2026-09-30

The native path is now real and supervised. Campaign 2 closes the first
coherent authored-frame slice while the canonical overview remains a diagnostic
vertical slice. This document prevents the authored calibration scene from
being mistaken for imported-asset parity, a complete production renderer, or
an Elden Ring-level presentation.

## Campaign 2 closure

The `render-campaign2-layout` command is green for close, medium, and wide
views. It proves a reusable authored composition over the certified world:
terrain material conditioning, smooth hero geometry, clustered foliage, a
wet/reflective surface, directional light and shadows, environment/fog intent,
fixed cameras, Rust promotion, vector evidence, and clean-process replay.
The clean replay matched all deterministic certification artifacts byte-for-byte.

The Campaign 2 visual vector measures silhouette/readability, material
separation, grounding/contact, lighting consistency, atmospheric depth, texture
frequency, composition, density, artifact rate, frame/GPU cost, upload/readback
memory, and visible/total instances. Grounding/contact, lighting consistency,
and atmospheric depth remain explicitly indeterminate; no status-only or
producer-only value is promoted for those axes.

This closes the campaign’s first authored-frame gate. It does not close the
following production systems: imported asset/source identity and conditioning,
true prefiltered IBL, contact/soft/cascaded shadows, dense terrain layer
mapping, production vegetation, temporal reconstruction, resource residency,
water simulation, animation, or character rendering.

## C2.5 imported-asset inspection closure

The permanent assembled log-hut fixture now completes the native imported-asset
conditioning, scene binding, close-camera lowering, Julia/Lava capture, Rust
promotion, worker restart, and warm-state deterministic replay path. Imported
tangents are canonical in the graphics packet. The historical C2.5 GPU
payload was single-level; C2.7 provides the versioned authority-side chain
representation and C2.8 closes GPU residency with a sampler LOD span. The
close/context
captures and their receipts are preserved in
`/home/mattc/Pictures/WGE/c2.5-imported-asset/`.

This is a technical inspection closure, not a visual-quality closure. The hut
is still framed inside the calibration shrine, and the next visual frontier is
material/mip fidelity plus a cleaner authored composition.

## C2.6 texture-transform conditioning closure

The render-asset boundary now understands the glTF `KHR_texture_transform`
subset that the native path can represent safely. A shared transform across
present material texture roles is preserved in the Rust render package and
lowered into canonical UV0 before tangent generation. A non-zero alternate
texcoord set or conflicting per-role transforms is rejected with typed findings;
the transform is never silently dropped or applied only to albedo.

This slice is contract/conditioning-only: Lava still receives the ordinary
canonical UV0 packet, and the C2.5 native replay remains the renderer gate.
Multi-level texture payloads/residency, alpha execution, collision/LOD
carry-through, and a cleaner authored composition remain open.

## C2.7 deterministic mip-chain conditioning closure

The authority-side texture contract now supports a bounded, deterministic
`generate_cpu_chain` policy. Rust preserves the base RGBA8 level and derives
descending levels through 1x1 with color-space-aware downsampling: sRGB RGB
channels are averaged in linear space, normal maps are averaged and
renormalized as vectors, and linear/data channels use a box average. Level
dimensions, byte lengths, chain count, and tamper-bound content identity are
independently validated.

The neutral graphics projection carries the chain as a versioned
`rgba8_mip_chain` payload, and Julia validates the same dimensions and digest.
At C2.7 closure the Lava adapter rejected multi-level payloads with an explicit
typed unsupported-capability result — an intentional seam, not a silent
one-level fallback. C2.8 has since closed that seam with full GPU residency
and a sampler LOD span validated against the Rust residency-telemetry
expectation. The C2.5 imported-asset GPU replay remains the backend regression
control and is byte-identical because single-level textures clamp LOD to zero.

## Measured or directly observed gaps

1. The canonical Riverwatch overview is still visually coarse: a small terrain
   field, simple obstacle geometry, diagnostic gameplay markers, and opaque
   crossed foliage. The composed-world profile now proves that real authored
   instances can coexist with the showcase composition, but its foliage still
   reads as sparse markers and its terrain remains a diagnostic field. The
   showcase profile demonstrates composition, procedural
   stone/metal/emissive roles, normal response, and shadowed perspective
   rendering, but it is still a bounded authored probe rather than imported
   hero-asset richness or production environment breadth.
   A real slope-readability remap was tested and rejected: capture-wide
   luminance spread fell 0.95% and a fixed terrain-crop spread fell 1.78%;
   no renderer change was retained. See `docs/archive/2026-09_campaign1-visual-experiments/visual-axis-design.md`.
2. The canonical material contract now has digest-bound albedo, normal,
   roughness, occlusion, and emissive roles plus scalar metallic/roughness and
   clearcoat controls.Rust and the neutral packet represent a validated
multi-level RGBA8 chain, and C2.8 now residents that chain on the GPU with a
sampler LOD span; alpha, transmission, and material-graph identity remain
open.
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
7. GPU scene scalability is now measured for both a synthetic instanced
   foliage stress profile and a 40-instance composed authored-world profile.
   The authored profile remains one bounded small scene (28 draw calls, 4,656
   submitted mesh vertices), while the synthetic profile still provides the
   larger 544-instance batch. Neither establishes traversal-aware culling,
   meshlets, GPU-driven indirect draws, residency/streaming policy, or an
   occlusion hierarchy.
8. Cold startup remains expensive. The current adapter-v6 close sample took
   31.131 seconds wall for its cold promoted frame. Across 30 warm frames, wall
   time reached 1,144.686 ms p95, while the adapter-reported frame interval was
   69.660 ms p95 and the GPU interval was 485 µs p95. The large wall-minus-
   renderer interval is not separately attributed yet; it must not be silently
   blamed on Rust, Julia, or GPU copies. Historical v4/v5 runs remain useful
   controls, but are not the current budget. Named pass timestamps add
   synchronization overhead, so this is not yet a production or dense-scene
   frame budget.
9. The native capture path renders a validated gameplay/world snapshot. The
   Rust reference runtime completes the 78-step traversal and wins gameplay.
   The bounded offscreen live-session supervisor now binds exact packet and
   capability identities and records Tier B attestations; a one-frame
   RenderWindow/swapchain/present capability probe is also green. The C3
   presented-session slice (2026-10-02, commit `3933c24`) now keeps a
   persistent window presenting the certified composite continuously, with a
   Rust-owned camera and Tier-A evidence sampled through the offscreen
   authority path; input, simulation tick, and frame pacing are still open, so
   Lava is attached to a present loop but not yet to a native input/update loop.
10. Bevy produced a provenance-valid native inspection PNG, but its current
    capture is flat/coarse for this packet. It is retained as a regression and
    inspection oracle; it is not presented as visual parity evidence for Lava.
11. A separate Rust-owned technical visual-quality gate now measures projected
    terrain separately from authored geometry and excludes declared semantic
    overlays. It has deterministic integer thresholds, known-good/known-bad
    controls, capture/receipt binding, renderer attestation, and independent
    remeasurement. The certification orchestrator exercises the
    provenance-bound composed-world showcase, while the canonical overview
    remains diagnostic. The latest registered profile is green for both the
    canonical Riverwatch sample (2,281 bp terrain coverage, 3,000 bp combined
    content coverage, 576 bp spatial edges, 28 luminance bins, 321 RGB bins,
    and 31/64 varied tiles) and the distinct Cedar sample (2,571 bp terrain
    coverage, 3,255 bp combined content coverage, 529 bp spatial edges, 28
    luminance bins, 314 RGB bins, and 33/64 varied tiles). The gate is narrower
    than an aesthetic or production-readiness judgment and remains fail-closed
    when any measured scene misses its technical floor.

## Explicitly deferred or negative controls

- character rigging, skinning, and retargeting;
- arbitrary mesh-to-character generation;
- the supplied bad GLB negative/rejection control remains permanently
  negative;

These are deferred or negative controls, not silently passed features. No
certification report may promote them based on a world snapshot or a status-only
receipt. There is no external-engine certification or comparison gate in the
current WGE scope.

## Priority order for closing gaps

1. Improve the permanent imported-asset capture from a technically certified
   static inspection into a genuinely authored material view: alpha policy,
   collision/LOD carry-through, and a cleaner camera/composition. Tangent-aware
   shading, texture-transform conditioning, GPU mip residency/sampler LOD
   (closed by C2.8), and the close-camera replay are green; the supplied bad
   GLB remains a rejection control and is not promoted.
2. Replace the synthetic stress population with a denser authored scene or
   camera-cut profile, and separate instrumented timing from uninstrumented
   production timing before optimizing cold startup or choosing a frame budget.
3. Exercise traversal-aware semantic culling with landmark and
   gameplay-critical visibility, camera cuts, and a stressable population.
4. Add prefiltered IBL and higher-quality shadow strategy behind typed packet
   contracts and independent visual gates.
5. Attach the renderer to a reference gameplay update/input loop and integrate
   the live-evidence session contract. The traversal stepping seam, incremental
   gameplay session, bounded offscreen supervisor, and grounded kinematic contact
   seam are now deterministic and snapshot-restorable; the persistent window
   present loop is now green (C3 presented-session slice), while input, full
   dynamics, frame pacing, and richer ability/NPC state are still open.
6. Add LOD/residency/streaming and effects systems only when their semantic
   ownership and receipt contracts are clear.

The next quality slice should improve one high-value visual axis at a time and
keep every new claim independently revalidated by Rust authority.
