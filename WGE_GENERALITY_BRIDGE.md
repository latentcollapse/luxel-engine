# WGE Generality Bridge

Status: strategic R&D plan; the full-generality successor to the Game Dev harness roadmap
Date: 2026-09-29
Scope: bridging today's certified engine-neutral checkpoint to general-purpose game and world
creation — professional quality, zero external engine runtime, driven end to end by Luna-class
models. Companion docs: [GAME_DEV_HARNESS_ROADMAP.md](GAME_DEV_HARNESS_ROADMAP.md) (phasing and
operating model), [WGE_NATIVE_GRAPHICS_ARCHITECTURE.md](WGE_NATIVE_GRAPHICS_ARCHITECTURE.md)
(renderer authority boundary), [WGE_NATIVE_QUALITY_GAPS.md](WGE_NATIVE_QUALITY_GAPS.md)
(measured gaps), [CODEX_WGE_PEAK_SHAPE_HANDOFF.md](CODEX_WGE_PEAK_SHAPE_HANDOFF.md) (current
truth), [WGE_NATIVE_CONVERGENCE_REPORT.md](WGE_NATIVE_CONVERGENCE_REPORT.md) (convergence
evidence). The autonomous-construction research companions are
[WGE_AUTONOMOUS_GAME_CONSTRUCTION_FORENSICS.md](WGE_AUTONOMOUS_GAME_CONSTRUCTION_FORENSICS.md),
[WGE_CAPABILITY_REGISTRY_DESIGN.md](WGE_CAPABILITY_REGISTRY_DESIGN.md),
[WGE_STYLE_PROFILE_CONTRACT.md](WGE_STYLE_PROFILE_CONTRACT.md), and
[WGE_CONSTRUCTION_LEVERAGE_BENCHMARK.md](WGE_CONSTRUCTION_LEVERAGE_BENCHMARK.md). The
demo-focused execution roadmap is
[WGE_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md](WGE_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md),
with the standing model-friction evidence ledger in
[docs/FRICTION_LEDGER.md](docs/FRICTION_LEDGER.md).

This plan starts after the adapter-v6 adversarial audit closes. Nothing here reorders that work
or weakens any promotion gate.

---

## The mission

One brief. One stack of concept art. One master prompt. Out comes a playable, certified,
professional-quality game — built by the model, verified by the engine, and shipped by the
engine, with no external runtime required. **This is the engine.** It has to float on its own.

That is the destination. It is farther away than the current checkpoint, and the distance is
measurable, which is why it is crossable. Every system between here and there lands as the same
shape the engine already uses everywhere: a typed contract, an independent validator, adversarial
controls, and a receipt that cannot be faked. Nothing in this plan requires inventing a new way
to build WGE. It requires extending the one it has until the answer to "can WGE make *that*?" is
"give me the brief."

Two consequences are worth saying out loud, because they set the tone for everything below:

1. **The hard part is not the plumbing.** The plumbing — contracts, supervisors, receipts,
   gates — is the part WGE is already good at. The genuinely hard parts are model taste (does
   generated content meet a professional bar), deformation research (auto-rigging is a real
   research problem, not an integration task), and scale (a real game scene is three orders of
   magnitude denser than Riverwatch). This plan allocates effort accordingly.
2. **Generality is a portfolio, not a monolith.** "Any game" decomposes into five keystone
   capabilities. Each is independently valuable. Each unlocks application families the moment it
   lands. None of them waits for all the others.

## Where we stand (the honest inventory)

Green and load-bearing as of the 2026-09-29 convergence checkpoint:

- **Worldgen is the origin competency.** Terrain, hydrology with uphill-reversal rejection,
  ecology-obeying placement, traversal certification, stable world identity. Two mechanically
  different certified worlds (`riverwatch`, `quartz_marsh`) plus a fresh-input smoke
  (`cedar_saddle_relay`) prove the path is not fixture-shaped.
- **Gameplay substrate generalizes.** Typed entities, tags, resources, abilities, costs,
  cooldowns, effects, stacking, events, objectives, NPC policies, deterministic replay — two
  mechanically different scenarios through the same contracts. This is the GAS-equivalent the
  roadmap called for, already native.
- **The native renderer is real and supervised.** Rust lowers `wge.graphics-scene-packet/v6` →
  persistent Julia worker → pinned Lava/Vulkan → offscreen capture → Rust independently
  revalidates and promotes. PBR-lite materials with role maps and clearcoat, one directional
  shadow map, analytic environment lighting, HDR resolve, semantic instancing and culling,
  GPU timestamps, deterministic replay after clean restart. The current adapter-v6 close sample
  is 918.113 ms wall p50 / 1,144.686 ms p95, 39.750 ms renderer p50 / 69.660 ms p95, and
  58 µs GPU p50 / 485 µs p95. The large wall-minus-renderer interval remains unattributed;
  these are diagnostic measurements, not a 60-Hz frame-budget claim. Campaign 1 now also has
  a separate Rust-owned technical visual-quality contract that excludes semantic overlays from
  terrain measurements and binds results to the promoted frame receipt; the current diagnostic
  capture is not silently promoted as production quality. The bounded offscreen live-session
  supervisor and incremental gameplay session now bind exact packet/capability digests and
  produce Tier B attestations; a live window/present loop is still deliberately open.
- **The transaction spine is enforced.** `wge_control_plane::ProjectStore` owns staging,
  evidence, repair, promotion, rollback; receipts re-run registered validators; corruption fails
  closed. The MCP surface exposes 16 semantic tools with zero semantic authority.
- **The character path has a live skeleton.** `pipeline/rigging_provider.py` deterministically
  generates a rigged, skinned, socketed, animated control humanoid through Blender;
  `asset_contract` parses GLB containers and accessors natively and fails closed on everything
  it does not understand (skin influence is currently reported *unsupported*, not guessed). The
  bad GLB remains a permanent rejection control.
- **The authoring doctrine is measured.** Fix-naming errors took weak-model success from 3/6 to
  6/6; scaffolds-from-existing-worlds took it from 0/4 to 4/4. Recognition beats recall; the
  toolchain, not the model, owns correctness.

Known structural gaps (measured, not hidden): no persistent live window/swapchain/present loop
(a one-frame native capability probe is landed and the bounded offscreen supervisor is landed),
no full dynamics physics substrate (a deterministic grounded-kinematic contact seam is landed), no
skeletal/skinning/animation contracts in the graphics path, no prefiltered IBL / cascades / TAA /
alpha foliage / particles / water / post, no LOD-meshlet-streaming-occlusion, one geometric
representation (heightfield — no caves, overhangs, or interiors; MISSING_INVENTORY M4), expensive
cold start (28–116 s, ~1.7 GB RSS), and the root README still preaches "WGE deliberately does not
implement rasterization," which the native convergence superseded. Fixing that README is
housekeeping, but it matters: stale doctrine gets re-absorbed.

## The doctrine that generalizes (and what it forbids)

Every capability WGE has ever gained landed the same way, and the universality of that shape is
the actual argument for generality:

    intent (model) → typed spec → deterministic lowering → certified artifact → evidence → delivery

A certified world, a certified frame, a certified rig, a certified playtest, a certified avatar:
same shape, different payload. What the doctrine *forbids* is equally general, and it is the
reason this plan can be trusted with scope this large:

- No semantic authority below the Rust plane. Producers never grade their own work.
- No silent fallback, clamping, substitution, or omission. Unsupported is an outcome.
- No source-as-code. Authoring is parsed, never executed, in every domain, forever.
- No new vocabulary without scaffolds, repair-carrying diagnostics, and conformance tests.
- No gate satisfied by degrading declared intent (language spec invariant 14).

And one deliberate non-forbidding, because it trips people up: **Blender-as-provider is allowed;
external-engine-as-runtime is not.** Blender is a headless tool that produces digest-bound
artifacts; it never ships in the runtime and never touches semantic identity. An external runtime
would own the frame and split authority — that is the line. Lava owns the frame; therefore Lava is
the renderer.

## Two stars to steer by

These are not gates. They are the reason the work is worth doing, and they are chosen because
each one drags a different keystone onto the critical path — which makes them excellent
out-of-the-box thinking tools: when designing any slice below, ask *"does this slice make either
star closer?"* If it does, the slice is probably shaped right.

**Star 1 — the morphing avatar.** A custom VTuber avatar requested through the forked
Open-LLM-VTuber surface wrapping Cyan: concept art and a description in; a rigged, skinned,
expression-capable character out, morphable into any character you can think of — because every
rig conforms to a WGE skeleton profile and every face to a named blendshape profile, so "become a
different character" is a certified rebinding operation rather than new art. This star forces the
character pipeline (K2), the live render path (K1), and the skill-floor discipline (K5) — and it
needs *no game at all*. It is the character campaign wearing a delivery runtime.

**Star 2 — the living wallpaper world.** A playable 3D background fantasy world: brief in; an
ambient, continuously rendered world out — time-of-day, wind, water, wildlife, no win condition,
runs for hours, costs its frame budget. This star forces the live loop (K1), the effects and
atmosphere slice of renderer breadth (K3), and packaging into an embedded delivery runtime. Like
Star 1, it exercises the full "brief → certified world → live delivery" spine without requiring
gameplay depth — the gameplay substrate that already exists covers the interactive cases when a
real game is wanted.

And once both are real, the same base serves more than anyone will predict in advance, because
the spine is universal: machinima and procedural shorts from a script brief; tabletop/RPG world
viewers with certified sightlines and encounter spaces; a mod/kit factory where per-model asset
families mix safely (S16 authorship provenance); educational and archviz walk-through worlds;
NPC-dialogue-rich sandboxes backed by Cyan; certified labelled-world data for training world
models (the Project Aisling thesis, already in the README). The engine becomes a base for a ton
of applications exactly because it is one engine with one evidence culture, not a pile of
generators.

---

## The five keystones

Each keystone states: current state, the gap, campaign slices with exit conditions, and the R&D
risk. Slices are ordered so that no slice can smuggle in an unverified representation — the same
rule the native graphics work has been following since the audit.

### K1 — Live loop and simulation substrate

**Current state.** The renderer processes validated world *snapshots*; the reference runtime
completes traversal/gameplay deterministically but nothing renders "while playing." Current
adapter-v6 warm snapshots pay full readback + rehash + promotion (918.113 ms wall p50,
1,144.686 ms p95; 39.750 ms renderer p50). The wall-minus-renderer interval is not yet
attributed. Physics now has a bounded deterministic grounded-kinematic contact seam; full
dynamics and engine-adapter mapping remain open. Historical cold start is 28–116 s. The first
Campaign 1 contract slice now includes bounded Tier A/Tier B live-session
  evidence scheduling, an offscreen live-session supervisor, and a deterministic incremental
  gameplay/traversal session with sealed snapshots and restore verification. It is not yet wired
  to a live window/present loop; physics and richer incremental ability/NPC state remain open.

**The gap.** A game is a loop, not a snapshot. The evidence architecture that makes WGE
trustworthy is per-frame unaffordable at 60 Hz — and the fix must not weaken it.

**Slices.**

1. **Two-tier evidence model (contract and offscreen integration landed; continuous loop remains).** Tier A:
   certification snapshots — unchanged, full promotion, byte-deterministic, the only grade that
   can move a pointer. Tier B: live frames — present instead of readback, no per-frame digest,
   bounded-telemetry attestation, and *periodic sampled revalidation*: every N seconds (and at
   every semantic event: scene change, camera cut, capability change) the live path freezes a
   frame and runs it through the identical Tier A promotion path. A live session that cannot
   produce a passing snapshot sample is demoted to indeterminate — the live path never outranks
   the evidence path. Exit condition: a live session rendering continuously with hourly-rate
   snapshot samples all promoting, and a forced-failure control (injected visual corruption)
   demoting the session within one sample window.
2. **Lava window path as a first-class target.** `RenderWindow`/swapchain exists upstream and is
   currently diagnostic-only. Promote it behind the same capability/failure reporting as the
   offscreen path; present-to-screen becomes a typed capture mode, not a second renderer. Exit
   condition: the same packet renders through both offscreen (certified) and windowed (live)
   targets with a documented correspondence check.
3. **Physics substrate decision (bounded kinematic seam landed; full dynamics remains).** Requirements, in order: deterministic stepping and state
   serialization (replay depends on it); Rust residency in the authority plane (collision and
   contacts are semantics, so they cannot live in Julia-the-renderer or Python-the-glue);
   character-controller-friendly queries. The first seam now selects a custom minimal grounded
   kinematic substrate; Rapier behind a typed WGE simulation contract and avian remain candidates
   for full dynamics only. Exit condition: a decision record with a deterministic replay test (same
   inputs → identical world state digest after 10,000 steps) and adversarial controls (spawned
   interpenetration, tunneling, stale-state rejection) — mirroring how every other substrate was
   adopted.
4. **Frame budget instrumentation, uninstrumented.** The benchmark doc already separates
   instrumented from uninstrumented timing; extend that discipline to the live path: a
   no-timestamp steady-state mode as the budget claim, timestamps as the diagnostic. Exit
   condition: a published steady-state distribution for a representative scene, with the
   instrumented/uninstrumented delta recorded.
5. **Cold start attack.** PackageCompiler sysimage for the graphics worker first (it is pure
   startup cost), worker pooling/reuse second, capability-probe caching third. Exit condition:
   first-certified-frame under 10 s and first-live-frame under 15 s on the reference host,
   with determinism receipts unchanged.

**R&D risk.** Low research risk; the work is architectural discipline. The one trap is
normalizing "the live path is approximate, so evidence can slide" — no. The two tiers are
different *costs* for the same *standards*.

### K2 — Character and content pipeline

**Current state.** A Blender provider produces a structurally correct rigged control GLB;
`asset_contract` validates container/accessor/bounds facts and reports skin influence
unsupported; the graphics packet has no skeletal concept; no animation graph exists anywhere.

**The gap.** "Any game" and the avatar star both require characters that import, rig, animate,
retarget, and play — with acceptance gates that catch unusable deformation, not plausible pixels.

**Slices.**

1. **Skeleton profile registry (the keystone of morphability).** A closed, versioned registry of
   named skeleton profiles — joint hierarchy, naming, rest pose, socket attachment points —
   starting with one humanoid profile. Every rig in the system *must* conform to exactly one
   profile; nonconforming rigs are rejected with repair-carrying diagnostics ("bone
   `lowerarm_l` unknown to profile `humanoid_v1`; nearest: `forearm_l`"). This is what makes
   "morph into any character" a rebinding operation: animation data, retarget maps, sockets,
   and gameplay attachments all bind to the *profile*, and any conforming mesh can wear them.
   Exit condition: profile registry with canonical identity in the Rust plane, conformance
   validator, and two independently authored meshes conforming to the same profile sharing one
   animation set with measured joint correspondence.
2. **Skeletal packet contract (packet v7).** Extend the scene packet with skins (joint lists,
   inverse bind matrices), per-vertex joint indices/weights, animation clips (per-joint
   time-sampled or curve-bound transforms, root motion flag), and blendshape targets. Same
   rules as always: closed, digest-bound, cardinality-checked in Rust *and* Julia, bounded,
   fail-closed on unknown fields. `asset_contract`'s skin-influence "unsupported" becomes
   supported behind real validation (weight normalization, joint-count bounds, degenerate
   influence rejection). Exit condition: the control humanoid from `rigging_provider.py`
   crosses the native path, renders skinned in Lava at three pose times, and Rust remeasures
   pose-dependent pixel evidence — byte-deterministic per (clip, time).
3. **GPU skinning in Lava.** Joint matrices per instance in a storage buffer; vertex-path
   skinning first (linear blend, bounded joint influences), compute-path later if profiling
   demands. Blendshape path for facial targets next. Exit condition: skinned draw at the same
   telemetry standards as instanced draws (counts Rust-checked, determinism replay after
   restart).
4. **Animation semantics.** Clips, blend trees, layers, and state machines as *typed semantic
   data* in the gameplay substrate (not sandboxed code — animation state machines are
   declarative domain data like abilities are). Deterministic sampling: animation state is part
   of replay state. Exit condition: a locomotion loop (idle/walk/run blend by speed) running in
   the reference runtime with replay-identical poses, and a repair-carrying diagnostic when a
   referenced clip/socket does not exist.
5. **Retargeting.** Profile-to-profile retarget maps as content-addressed artifacts with
   acceptance gates taken straight from roadmap §6's failure list: foot sliding (measured root/
   stride consistency), clipping (bounded self-intersection probes), broken deformation (joint
   limit violations), missing sockets, scale errors. Exit condition: one animation set authored
   for profile A driving a conforming mesh of profile B through a retarget map with all gates
   green and a known-bad retarget rejected.
6. **Asset ingestion at scale.** The content-addressed project store the roadmap describes:
   intake → identity → typed report → catalog registration → per-asset LOD/collision/bounds
   acceptance. Texture/material provider outputs get the same provenance treatment. The bad-GLB
   control stays permanently negative; a known-good *external* art asset becomes the quality
   control for imported topology. Exit condition: a ten-asset catalog (varied: rock, tree,
   building, prop, character) through the full path with per-asset receipts.
7. **Auto-rigging R&D (the campaign's research frontier — treated like TetCageRT, not like an
   integration task).** The sequence that has worked before: pin the literature (pin-based skin
   prediction: heat-diffusion/geodesic bone weighting, voxel skeleton inside mesh volume,
   learned skinning as *optional* later), build a deterministic CPU reference, constrain
   predictions to the profile registry (the registry massively shrinks the problem — predict
   weights and joint centers, not topology), measure deformation error against artist-rig
   ground truth, and only then materialize a Rust-owned policy for when an auto-rig is
   promotable. Uncertain, unprofitable, and failed remain explicit outcomes. Exit condition: a
   research note with reconstruction citations, CPU reference, measured deformation error on a
   benchmark mesh set, and a materialization policy — or an honest record of why not yet.
8. **Facial layer (avatar star).** Named blendshape profile (ARKit-52-shaped, owned by WGE) +
   viseme mapping + expression intent in the authoring surface. Exit condition: a conforming
   face driving visemes from the dialogue substrate with Rust-measured mouth-region variation
   per viseme (the visual-gate pattern, applied to faces).

**R&D risk.** High and localized: auto-rigging quality and facial expression quality are real
research with taste components. The registry-first strategy is the mitigation — it converts
"generate a rig" (open-ended, unreliable) into "predict weights against a fixed skeleton"
(bounded, measurable). Note also what *not* to build: no generic mesh-to-anything generation,
no arbitrary topology surgery. Meshes that cannot be profile-rigged are rejected with
diagnostics; that is a feature of the quality bar, not a limitation to apologize for.

### K3 — Renderer breadth and scene scale

**Current state.** The bounded material path, one 512² directional shadow map, spatial 2×
resolve, analytic environment lighting, opaque cross-mesh foliage. The gap register (priority
ordered) is the contract for this keystone; the architecture doc's "quality only inside the
certified boundary" rule is its law.

**Slices, following the register's own priority order.**

1. **Hero-asset textured capture.** One real authored/imported asset with albedo/normal/
   occlusion/emissive roles through a close-range validated capture. This is the register's #1
   and the gateway to everything visual. Exit condition: promoted close-range receipt with
   Rust-measured material response; the imported asset's provenance bound end to end.
2. **Prefiltered IBL.** Environment convolution + probe contract behind typed packet intent;
   analytic sky remains the fallback profile, explicitly declared. Exit condition: two
   environment probes with independently recomputed irradiance statistics and a fallback that
   is reported, never silent.
3. **Shadow strategy.** Cascaded maps for the directional sun (2–4 cascades), contact
   hardening later, filtered soft shadows behind a quality profile selector. Exit condition:
   cascade split selection derivable from camera + light intent (deterministic), Rust-checked
   coverage per cascade, and the register's "many-light budgets" explicitly deferred until a
   scene needs them.
4. **Alpha-tested foliage and LOD.** The foliage system graduates from cross-mesh diagnostic to
   alpha-tested cards with distance LOD, ecology-obeying placement (S6 already computes the
   suitability field), wind intent as typed shader parameters. Determinism preserved per
   (seed, LOD-set, camera-distance-class). Exit condition: a dense authored forest scene with
   per-LOD promoted captures and culling telemetry that Rust balances.
5. **Temporal techniques under a declared grade.** TAA and history-aware reconstruction
   break byte-determinism. The language spec already has the mechanism: a backend that can only
   provide tolerance-bounded determinism must *declare the lower grade*. So: TAA ships as a
   live-path-only quality profile (Tier B, declared), while the certification path keeps the
   deterministic spatial resolve (Tier A). Never the reverse, never silent. Exit condition: a
   profile negotiation test where a TAA-requesting certification capture fails with a
   diagnostic naming the deterministic alternative.
6. **Particles, water, decals, post-processing.** Each as its own typed contract + validator +
   adversarial control; order by star pressure (water and atmosphere for Star 2; post stack —
   bloom/DOF/motion-blur-intent, live-path grade rules like TAA — for both). Exit conditions:
   per-system, mirroring the visual-gate pattern.
7. **Scene scale machinery.** Meshlets, GPU-driven indirect draws, residency/streaming, occlusion
   — behind the same typed packet extensions and telemetry. Exit condition: a 10⁵-instance
   scene with per-frame submission counts, per-class visibility, and budget-constrained
   residency, all Rust-checked. The dense benchmark's 544 instances is the floor to beat by
   ~200×; the synthetic profile stays synthetic (never authored evidence).

**R&D risk.** Low-moderate: this keystone is mostly the "typed contract + validator +
implementation" treadmill the engine already does well, applied to a known list. The one
genuine tension is (5), and the declared-grade mechanism resolves it doctrinally rather than
technically.

### K4 — World representation breadth

**Current state.** One geometric representation: the heightfield. MISSING_INVENTORY M4 names the
consequence: no caves, overhangs, arches, tunnels, or interiors. "Any kind of game" needs at
least interiors and overhangs; "any kind of world" needs volumetric expression.

**The open decision (the one genuine fork in this plan).** The second representation, with the
honest tradeoffs:

- **SDF/voxel patches bound into the heightfield world.** Terrain stays heightfield (everything
  downstream — hydrology, ecology, traversal, materials — keeps working unchanged); volumetric
  features (caves, overhangs, interiors) are bounded patches with their own digests, meshed
  deterministically at build time, merged into collision/navigation as first-class geometry.
  Pros: preserves the entire existing pipeline; bounded scope; patch digests fit the artifact
  model perfectly. Cons: patch blending seams need real solver work; interior lighting is its
  own problem.
- **Full voxel/brickmap worlds.** Uniform, expressive, and a rewrite of everything downstream.
  Not recommended except under evidence that patches fail.
- **Authored mesh volumes in the render plan.** Cheapest, least general; right answer for
  interior *kits* but not for carved terrain features.

Recommendation: SDF/voxel patches, flagged as a decision record needing review before
implementation, with the heightfield remaining canonical for open terrain. Exit condition for
the keystone: a certified world containing a traversable cave and an overhang — hydrology flows
around it, ecology refuses to grow trees inside it, navigation certifies through it, the native
renderer draws it, and every claim is gated. Interiors (rooms, portals, multi-floor) are a
follow-on slice only after patches are proven; do not start them early.

**R&D risk.** Moderate: patch-to-heightfield blending is honest numerical work best placed in
`terrain_lab` where that ownership already lives. The trap is scope: "second representation" is
a representation, not a full interior system with portals and streamed interiors. Keep the
boundary explicit.

### K5 — Vocabularies and the Luna floor

**Current state.** The authoring kernel is frozen (spec draft 0.6 core), world vocabulary is
real, gameplay vocabulary exists in the substrate but not yet as `wge.game` authoring surface,
and the master-prompt front door is typed intake with provider-neutral interpretation.

**The gap.** "Concept art and a master prompt → any game" needs: the `wge.game` vocabulary
(share the kernel, never fork a dialect), a character vocabulary, an effects/audio vocabulary,
and every one of them drivable by Luna alone — because capability-first is the rule, but Luna is
the only model being tested with, which makes the skill floor an engineering specification, not
a nice-to-have.

**Slices.**

1. **`wge.game` vocabulary.** Abilities, entities, objectives, encounters, progression, rules —
   all as declarative data over the existing gameplay substrate. Recognition-first: scaffold
   from any existing world; every error names the fix. Exit condition: a second mechanically
   different game authored in `wge.game` on a second world, per roadmap Phase 5's generality
   test.
2. **Character and effects vocabularies** as versioned domains per language spec §3: character
   (skeleton profile choice, proportions intent, material palette, expression set), effects
   (particle/water/ambient intent), audio (cues, music states, spatialization intent — contract
   first, synthesis is provider work). Exit condition: each domain round-trips, scaffolds,
   and passes conformance; no domain widens the kernel.
3. **The master-prompt front door.** Brief + concept art + master prompt → typed interpretation
   (observations, inferences, conflicts, confidence, regions — the intake contract already
   defines these) → ProjectSpec → campaign. Add what's missing for one-shot operation: a
   project-level plan artifact the model revises between iterations, and the critic→DSL loop
   (README's named next-major-work: turning `foreground_edge_density: 0.044` into a concrete
   suggested patch). Exit condition: one brief produces a complete certified game with every
   internal iteration recorded as candidates/receipts — the roadmap's one-shot definition, made
   literal.
4. **Luna-floor measurement harness.** The two measured levers (fix-naming errors, edit-don't-
   initate scaffolds) applied to every new domain as a *release gate* for that domain's
   authoring surface: mechanical grading (compiles, applies requested change, contains it, no
   collateral damage), Luna as the graded operator. Capability-first means the engine never
   waits for ergonomics — but a domain whose authoring surface fails Luna grading is not done,
   because the mission says the engine must be drivable by exactly this model. Exit condition:
   per-domain grading harness results recorded beside each domain's conformance suite.

**R&D risk.** Low technical, high discipline: the temptation is to grow vocabularies into
general-purpose languages. The spec already forbids it (§3: no arbitrary gameplay scripting, no
all-genre DSL); unique mechanics stay sandboxed code modules with typed interfaces. Also
housekeeping with doctrinal weight: reconcile the root README's stale "WGE does not implement
rasterization" framing with the native convergence, so no future session re-absorbs the old
backend-swap doctrine.

---

## The determinism boundary (short, and load-bearing)

Everything above rides on one distinction, stated once: **certification is byte-deterministic;
live operation is declared-grade.**

- Tier A (certification): fixed seed, fixed camera, fixed packet, readback, digest, independent
  recomputation, restart replay. Unchanged from today. The only path that promotes receipts,
  moves pointers, or passes gates.
- Tier B (live): present-to-screen, no per-frame hashing, sampled snapshot revalidation on a
  bounded cadence and at semantic events, TAA/temporal techniques and any nondeterministic
  quality features allowed *only here*, under a declared grade recorded in the session
  telemetry.

A frame that cannot be certified never certifies; a live session whose samples fail gets demoted
and reported. This is how AAA techniques arrive without sacrificing the property that makes WGE
different from every engine that ships a screenshot and calls it QA.

## Campaign sequencing

Dependencies, not dates. Each campaign closes with its exit conditions green and its evidence
committed; nothing starts on vibes.

| Order | Campaign | Keystones | Unlocks |
| --- | --- | --- | --- |
| 0 | Adapter-v6 audit close (complete); hero-asset textured capture (pending) | K3.1 | First imported-asset quality evidence |
| 1 | Live loop: evidence contract + offscreen supervisor + incremental gameplay landed; window/present path pending | K1.1, K1.2 | Everything interactive; both stars become possible |
| 2 | Bounded kinematic contacts landed; full physics decision + deterministic replay integration pending | K1.3 | Real gameplay motion; replay-grade simulation |
| 3 | Skeletal packet + GPU skinning + animation semantics | K2.1–K2.4 | First animated certified character; avatar star viable |
| 4 | Renderer breadth I: IBL, cascades, alpha foliage/LOD | K3.2–K3.4 | Register's core visual gaps closed |
| 5 | Retargeting + asset ingestion at scale | K2.5, K2.6 | Content variety; profile-swap morphing |
| 6 | Second representation decision + cave/overhang slice | K4 | Worlds beyond the heightfield |
| 7 | `wge.game` + character/effects vocabularies + front door | K5.1–K5.3 | One-shot generality; roadmap Phase 5 |
| 8 | Auto-rigging R&D + facial layer | K2.7, K2.8 | Model-authored characters; avatar star complete |
| 9 | Scene scale machinery + effects breadth + cold start | K3.5–K3.7, K1.4, K1.5 | Production-density scenes; wallpaper star viable |
| 10 | Luna-floor harness across all domains; Star 1 and Star 2 shipped as proofs | K5.4 | The mission, demonstrated twice |

Two ordering notes. First, K1.1 is deliberately before everything interactive: without the
two-tier evidence model, every subsequent live feature would be built against the wrong
architecture and re-paid later. Second, auto-rigging R&D (K2.7) is ordered *after* the registry,
packet, and retargeting slices because its success criterion is defined by them — it is also
the only slice that may report "not yet" as a completion, exactly like TetCageRT.

## The problem register (the brutal list)

The honest statement of what is genuinely hard, so nobody discovers these mid-campaign:

1. **Model taste is the ceiling.** Generated meshes, textures, and layouts must clear a
   professional bar, and no contract makes taste automatic. Mitigation is the roadmap's
   perceptual-gate portfolio (silhouette, composition, material response at player-encountered
   distances) plus adversarial controls; the S16 insight — different models are strong at
   different asset families — means mixing provider strengths safely is part of the design, not
   an afterthought.
2. **Auto-rigging quality** is real research (K2.7). Registry-first shrinks it; it does not
   eliminate it.
3. **Facial performance** is taste plus research plus a live path; viseme gating only measures
   correctness, not charm.
4. **Scale is a 200× jump** from the dense benchmark to production density, and streaming/
   residency interact with determinism in ways the packet contract will have to absorb.
5. **The second representation** has real numerical risk at patch seams (K4).
6. **Cold start** threatens the iteration loop that makes agent-driven development fast at all;
   it is a first-class work item, not cleanup.
7. **Scope gravity.** Every slice above is bounded on purpose. The failure mode of a plan like
   this is not a failed slice — it is an unbounded one. The explicit-deferrals sections of every
   companion doc remain in force.

## What this document does not authorize

- No weakening of the Rust promotion gate, the fail-closed policies, or the parsed-never-executed
  boundary — for any feature, at any pressure, at any phase.
- No external-engine work resumption; no external runtime, ever, in the delivery path.
- No TetCageRT promotion ahead of its research sequence; no auto-rigging promotion ahead of its
  materialization policy.
- No new vocabulary without scaffolds, repair-carrying diagnostics, conformance tests, and
  Luna-floor grading.
- No multiplayer/netcode, no universal engine parity, no all-genre DSL — the roadmap's original
  deferrals all still apply until their phase arrives with evidence.

## If you are picking this up

Read, in order: this file, `GAME_DEV_HARNESS_ROADMAP.md` (operating model and phases),
`WGE_NATIVE_QUALITY_GAPS.md` (the measured gap contract), `WGE_NATIVE_GRAPHICS_ARCHITECTURE.md`
(the boundary you must not cross), `CODEX_WGE_PEAK_SHAPE_HANDOFF.md` (current truth). The
adapter-v6 audit is closed. Campaign 1 is in progress: its evidence contract, offscreen
supervisor, incremental gameplay/traversal seam, strict visual-quality gate, and authority
red-team slice are green, while live window/present integration and richer gameplay state remain
the next integration boundary.
