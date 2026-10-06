# Campaign 1 Visual / Runtime Red-Team

Date: 2026-09-29
Role: Critic C, read-only review
Write scope: this report only

## Verdict

The adapter-v6 snapshot authority chain is materially stronger than a producer-claimed pass: Rust validates the world-to-packet binding, hashes the returned RGBA bytes, recomputes measurements, and seals the receipt. The current docs also correctly describe the renderer as a diagnostic checkpoint and explicitly list its quality gaps.

Campaign 1 is not yet acceptance-testable as written. A standalone Rust live-evidence state machine and a GCS registry/resolver have appeared in the checkpoint, but neither is integrated into the renderer/supervisor or gameplay runtime. The live contract improves sample identity and scheduling; it cannot yet attest that telemetry came from a presented frame. The existing gameplay contract still only runs complete discrete traces, and the current visual gate is a capture-presence/liveness check, not a professional-quality gate.

The existing PPMs inspected from `/tmp` support the documented limits: the overview is a mostly uniform terrain field with small semantic marks; the material showcase is one isolated low-poly shrine composition; the composed-world image places a small set of objects on a largely empty plane. They are useful substrate probes, not representative professional game worlds.

## Findings

### C1 — P1 — Standalone live-evidence contract is not tied to a presented frame

**Status:** Partially addressed by a new standalone contract; presentation and supervisor integration remain absent.

**Anchors:** `docs/gameplay/live-loop-design.md:3,58-88`; `world_core/crates/live_evidence_contract/README.md:3-21`; Tier-A sampling identity/window in `world_core/crates/live_evidence_contract/src/lib.rs:82-112,154-167`; Tier-B identity/counters at `:284-296,500-555`; current snapshot receipt fields in `world_core/crates/native_graphics_contract/src/lib.rs:468-486`.

The new crate adds typed session/request IDs, packet and capability digests, monotonic frame sequence, bounded caller-configured sample interval/window, stale-result rejection, and fail-closed state transitions. But it is standalone and explicitly does not implement a window/swapchain, live telemetry source, presentation acknowledgement, supervisor integration, or receipt revalidation. Tier-B records bind one session packet and counters, not the actual framebuffer or a per-frame packet/state digest; there is no simulation tick/input watermark or dynamic-state digest. The current worker still serves `render_packet` only (`graphics_lab/bin/luxel_graphics_worker.jl:88-103`). Therefore an offscreen Tier-A sample cannot yet be proven to correspond to the frame currently displayed, and caller-selected timing policy is not a Campaign 1 acceptance default.

**Adversarial test:** At integration, present frame `n`, change camera/world state, then deliver telemetry or Tier A from `n-1`; require stale state rejection and a fixed demotion bound. Spoof a monotonically increasing counter without presenting frames, and inject a missed-sample window and worker stall. The standalone crate already has contract-level expiry/stale identity tests; these must not be mistaken for renderer integration evidence.

**Bounded fix:** Integrate the contract in a Rust supervisor that obtains frame sequence and telemetry from the actual presentation path, binds state/camera/content identity, and freezes the exact Tier-A packet for native revalidation. Choose a numeric policy and end-to-end demotion latency. Keep Tier B as runtime health only; only native Tier-A validation may certify.

### C2 — P1 — Existing gameplay replay cannot drive an interactive live loop

**Status:** Current implementation gap relevant to Campaign 1.

**Anchors:** `world_core/crates/gameplay_contract/src/general.rs:148-157,319-334,758-833`; `world_core/crates/reference_runtime/src/gameplay_binding.rs:24-38,215-237`; Campaign 1 intent in `docs/content-sdk/generality-bridge.md:145-171`.

`GeneralGameSpec` describes navigation, entities, abilities, and objectives. Inputs are discrete commands (`Move`, `ActivateAbility`, `Wait`, etc.). The public runtime API consumes an entire `GeneralTrace`; it constructs state, clones it for each event, applies the event, and returns a complete receipt. There is no public single-tick state transition or live input queue. The new live-evidence crate tracks presentation counters but explicitly does not add a game loop or gameplay simulation (`docs/gameplay/live-loop-design.md:74-84`). Current gameplay capture metadata explicitly labels the image `WorldOverviewBeforePlaythrough` at simulation tick 0. Thus a continuous window can currently demonstrate continuous presentation of a static world, but not gameplay state advancing on screen.

**Adversarial test:** Feed one trace through incremental step calls and through the existing batch replay; compare every intermediate state digest and final receipt. Vary render cadence independently of fixed simulation ticks; inject repeated, dropped, delayed, and reordered inputs and require deterministic rejection or handling.

**Bounded fix:** Add a Rust-owned deterministic step/restore interface over the existing gameplay state and discrete commands, or state explicitly that Campaign 1 proves live rendering only. Keep physics selection in its planned later slice; do not silently treat the current offline replay as a live simulation.

### C3 — P1 — The promoted visual gate accepts noise-level output

**Status:** Current gate is too weak for any professional-quality claim; current checkpoint docs do disclose this limitation.

**Anchors:** thresholds at `world_core/crates/native_graphics_contract/src/lib.rs:31-35`; whole-image measurements at `:819-867`; gate at `:870-928`; existing checkerboard acceptance test at `:4110-4131`; quality disclaimers in `docs/world/native-quality-gaps.md:5-21,93-94` and `docs/archive/2026-09_native-graphics-checkpoints/native-showcase.md:81-92`.

Rust currently accepts luminance standard deviation `0.01`, three distinct RGB colors, sixteen non-dominant pixels, and at least one exact-color pixel for each marker role present. The luminance and color-diversity calculations include the entire capture, including overlays. The role test accepts any nonzero supported-pixel count. A 320×240 near-black image with sixteen bright noise pixels plus one correctly colored pixel at each declared marker’s projected support can satisfy these conditions while showing no useful world. The unit test proves the gate accepts a high-contrast checkerboard, not a scene-quality control. The `Passed` frame status must not be read as perceptual approval.

**Adversarial test:** Add a minimal near-black/noise capture with marker-color pixels at the computed support samples and assert that it fails any gate named or consumed as visual quality. Also test a uniformly colored terrain with the semantic overlays providing all color diversity.

**Bounded fix:** Keep the current check named and documented as capture integrity/minimum presence. Add a separate versioned quality gate before any professional-grade claim: evaluate terrain-only regions separately from overlays, require meaningful spatial coverage/structure and target-specific thresholds, and include a known-good plus known-bad visual control. Do not broaden Campaign 1 into general renderer work to close this.

### C4 — P1 — Published warm-frame figures are stale and materially understate current wall time

**Status:** Current documentation drift; affects Campaign 1 budgeting.

**Anchors:** current v6 measurements in `docs/world/native-graphics-benchmark.md:31-57`; old figures in `docs/content-sdk/generality-bridge.md:56-61,147-150` and `docs/world/native-quality-gaps.md:44-50`.

The bridge describes the warm frame as about 263 ms wall, 48 ms renderer, and 0.7 ms GPU. The quality-gap register uses the earlier 289.520 ms wall p95. The current v6 30-frame sample is 918.113 ms wall p50 / 1,144.686 ms p95, 39.750 ms renderer p50 / 69.660 ms p95, and 58 µs GPU p50. The benchmark correctly says the large interval outside renderer time is still unattributed. These older figures describe historical runs and should not be labeled current. Even before input/simulation work, the certified snapshot path is far from a 16.67 ms 60-Hz frame budget; Campaign 1 must demonstrate that Tier B avoids the snapshot cost instead of assuming it.

**Reproduction:** Compare the “Current adapter-v6 close sample” table with the bridge inventory and K1 current-state paragraph. Re-run the documented benchmark only after an instrumented/uninstrumented split is available; preserve the present v6 distribution as the baseline.

**Bounded fix:** Refresh both docs from the v6 benchmark and label the 263/289 ms results as historical. Make a Tier-B, no-readback steady-state distribution an explicit Campaign 1 exit gate, separate from Tier-A promotion timing.

### C5 — P1 — The snapshot transport performs full-packet and full-frame copies; live costs are unmeasured

**Status:** Confirmed snapshot-path copying; contribution to wall-time gap is unproven.

**Anchors:** full packet JSON request in `world_core/crates/native_graphics_contract/src/supervisor.rs:295-305,397-405`; capture decode and retained copies at `:334-355,390-394`; Julia readback/conversion in `graphics_lab/src/LavaAdapter.jl:1495-1499,1413-1425,3332-3348`; worker response serialization in `graphics_lab/bin/luxel_graphics_worker.jl:25-33,88-103`.

Each snapshot request serializes the complete packet. Julia reads the framebuffer into host memory, allocates RGBA bytes, base64-encodes them, serializes JSON, and copies the response to a byte vector. Rust clones the frame JSON value, decodes base64 into another RGBA vector, then returns both the `GraphicsFrameOutput` containing `capture_base64` and `PromotedFrame.capture_bytes`. The benchmark’s wall-minus-renderer gap is large, but no evidence attributes it to these copies; report this as an allocation/transport risk, not a proven root cause. A 60-Hz path cannot reuse this round trip unchanged.

**Adversarial/performance test:** Profile a representative and dense packet across request serialization, Julia decode/validation, GPU upload, readback, RGBA conversion, base64/JSON serialization, Rust parse/decode, and authority measurement. Record allocation volume and p50/p95/p99. Assert Tier-B frames perform no framebuffer readback or full-image digest, and static mesh/texture bytes are not resent for camera-only or transform-only updates.

**Bounded fix:** First add stage timing and allocation counters. Then use retained immutable scene resources plus typed small frame-state updates for live rendering; use binary framed capture transport for Tier A and discard the encoded string after Rust validation. Preserve full Rust-owned snapshot validation.

### C6 — P2 — The exported PPM is not covered by a saved provenance manifest

**Status:** Current artifact-handoff gap; raw promoted RGBA provenance is sound.

**Anchors:** PPM export and receipt-to-stdout behavior in `world_core/crates/native_graphics_contract/src/main.rs:210-222`; RGBA-to-P6 conversion at `:491-513`; raw capture digest in `world_core/crates/native_graphics_contract/src/supervisor.rs:353-388`; human evidence hashes in `docs/archive/2026-09_native-graphics-checkpoints/native-graphics-handoff.md:40-61`.

Rust’s receipt binds the raw RGBA8 bytes. The CLI writes a transformed PPM file (drops alpha, adds a P6 header) and prints the receipt only to stdout. It does not save a sidecar containing the PPM file digest and the receipt together. A reviewer can therefore receive a modified/replaced image beside a valid stdout receipt without a direct artifact-level check. Existing v6 images inspected for this review were temporary `/tmp/luxel-v6-*.ppm` files, not a durable review bundle.

**Adversarial test:** Save an image/receipt bundle, alter or truncate the PPM, and require the verifier to reject it. Confirm the unmodified image decodes to RGBA whose hash equals the promoted raw-capture digest.

**Bounded fix:** Export a capture bundle with the image, receipt JSON, and a manifest binding output-file SHA-256, raw-capture SHA-256, packet/world/camera identity, dimensions, format, and exporter version. Store human review images in the agreed Luxel screenshot folder; keep the receipt as the authority artifact.

### C7 — P2 — GCS resolver is semantic composition, not executable gameplay

**Status:** A typed deterministic registry/resolver has appeared; runtime execution and several policy contracts remain deliberate limits.

**Anchors:** `docs/gameplay/gcs-foundation-design.md:1-4,19-38,63-74`; `world_core/crates/gameplay_contract/src/gcs.rs:268-347`; implemented runtime schema at `world_core/crates/gameplay_contract/src/general.rs:148-157`.

The new `gcs` module supplies typed IDs, a capability/profile registry, deterministic transitive dependency and profile resolution, cycle/conflict diagnostics, optional integration handling, canonical bytes/digest, and a small reference profile set. Its own design correctly limits the claim to semantic composition: it does not implement abilities, inventory, combat, persistence/networking, or execute validation suites. Version constraints, tuning ranges/override precedence, provider provenance, runtime budgets, and migration semantics remain open; `GeneralGameSpec` is still the separate replay schema. This is not a defect if the kit is presented as a semantic foundation, but it would be false to claim gameplay-kit runtime support from resolver success alone.

**Adversarial test:** Preserve the resolver's existing cycle/conflict/order tests; add version-constraint and tuning-conflict tests only when those semantics are specified. Ensure user-facing certification distinguishes resolved metadata from executable systems and passing validation suites.

**Bounded fix:** Keep this foundation accurately labeled as semantic composition and leave runtime/system execution to its own bounded slice. Do not expand into the broad genre catalogue.

### C8 — P2 — Campaign sequencing leaves the hero-asset gate ambiguous

**Status:** Current roadmap inconsistency; dependency decision is open.

**Anchors:** Campaign table in `docs/content-sdk/generality-bridge.md:419-433`; “start Campaign 1 next” at `:479-485`; still-open hero-asset priority in `docs/world/native-quality-gaps.md:73-80`.

Campaign 0 is marked in flight and contains both adapter-v6 audit close and the K3.1 hero-asset capture. The audit is closed, while the hero-asset requirement remains open. The final paragraph says to start Campaign 1 next, leaving unclear whether K3.1 gates that transition. Campaign 1 can exercise live timing with the diagnostic scene, but it cannot close the imported-asset or professional visual-quality gap.

**Bounded fix:** Split the completed adapter audit and pending K3.1 hero-asset slice into separate rows. State whether K3.1 is a prerequisite for starting Campaign 1 or a parallel/follow-on quality gate; preserve the explicit low-quality diagnostic label if Campaign 1 proceeds first.

## Deliberate deferrals and scope

The report treats rigging/skinning/retargeting, arbitrary mesh-to-character generation, external delivery/comparison gates, TetCageRT promotion, multiplayer, and broad genre-catalogue work as deferred. The handoff and bridge state those limits explicitly (`docs/archive/2026-09_native-graphics-checkpoints/native-graphics-handoff.md:99-114`; `docs/content-sdk/generality-bridge.md:467-477`). No finding asks to reopen them.

## Checks performed

- Read the requested bridge, quality-gap, handoff, benchmark, and gameplay-kit documents; inspected the Rust supervisor/validator, Julia worker/adapter, gameplay contracts, reference runtime, and existing tests.
- Inspected existing `/tmp/luxel-v6-overview.ppm`, `/tmp/luxel-v6-showcase.ppm`, and `/tmp/luxel-v6-world-showcase.ppm` without writing converted images to disk.
- Repository inspection found no Luxel `RenderWindow` or swapchain/present integration. A standalone Tier-A/Tier-B contract now exists under `world_core/crates/live_evidence_contract`, but its README says it is not a workspace member and the live-loop design explicitly leaves renderer/supervisor integration unimplemented. The worker path handles `render_packet` only.
- Attempted `cargo test -q -p luxel-gameplay-contract`; it queued behind an already-running native-graphics Rust compile in the shared `target` directory. Stopped only the waiting test command. No test result is claimed.
- No world render, code edit, or edit outside this report was performed.
