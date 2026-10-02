# WGE — WorldGen Engine

**Concept art in. A playable, faithful 3D map out.**

> **Fresh-agent orientation:** read [ACTIVE_ARCHITECTURE.md](ACTIVE_ARCHITECTURE.md) and
> [WGE_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md](WGE_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md) first.
> They define the current native Rust/Julia/Lava path, authority boundaries, provider fence, and
> campaign order. Older migration/status sections below are retained as historical context and are
> not current instructions.

WGE is a game engine. You give it concept art and annotations describing a place; it produces a
certified, deterministic world — terrain, hydrology, traversal, roads, settlements, materials,
placements — and renders and runs that world through its native runtime.

## An AI-native engine, not a human-native one

Conventional editor-first engines are **human-native**. Their primary interface is a GUI built for
a person with a mouse, their primary verification is a human looking at the viewport, and their
authoring surface assumes an operator who can see.

**WGE is the same category of thing built for a different operator.** Its interface is a typed
authoring language, its verification is a gate that measures the world, and its intended author is a
model. Every design difference follows from that one substitution:

| | Human-native engine | WGE |
|---|---|---|
| Authoring surface | GUI, inspector, viewport | Typed language, parsed never executed |
| Verification | A person looks at it | Gates measure it and can fail the build |
| Iteration | Drag, undo, re-render | Compile, diff, repair-carrying diagnostics |
| Correctness | Convention and review | Provenance binding and a stable world hash |
| Skill floor | A trained artist | Minimized model capability, measured by quality and repair efficiency |

What makes something an engine is that it owns the authoritative representation of the world and
the rules for constructing it — terrain, hydrology, traversal, collision, navigation, placement,
materials, provenance. WGE owns all of that, including its native graphics path.

External delivery adapters may remain as compatibility material, but they are not part of the
current WGE authority or certification path. If a change to WGE requires an external editor/runtime,
that change is in the wrong layer.

**Inspection instruments are not semantic authority.** The existing Bevy viewer and other bounded
reference tools may help WGE inspect its own output, but they are not interchangeable runtime
backends and no external engine is required to ship a WGE game. Audit captures must remain bound to
the exact packet, renderer path, camera, and authority receipt that produced them.

The distinction matters because the valuable artifact is the *world*, not the render of it. The
world is a verified data structure with a stable identity hash. Any engine can be pointed at it, and
two engines pointed at the same world must produce the same place.

## The goal

Take a piece of concept art — a painted overview, a sketched battle map — and end up walking around
that exact place in 3D. Not "something in the same genre." *That* place: its ridgelines, its water,
its sightlines, its chokepoints, its keeps where the painting put them.

Two things make that hard, and WGE is organised around both:

1. **Faithfulness is measurable, not vibes.** Every stage emits evidence and every claim is gated.
   A world that says it has three roads must have three roads that a player can actually walk.
2. **The author should not have to be an expert.** See below — this is the part that shapes the
   whole design.

## Model capability is an efficiency axis

The success criterion for WGE is **not** a parameter-count threshold. WGE minimizes the model
capability required for professional game-development work without sacrificing output quality.
Small-model runs remain valuable stress tests for authoring ergonomics, recovery, and tool cost;
they are an efficiency axis, not the definition of product success.

This is why the authoring language is Python-shaped: every model already writes Python, so nobody
starts from a near-empty prior. But syntax turns out to be the easy half. Measured results on the
authoring surface:

| Change | Weak-model success |
|---|---|
| Errors that name the *fix* instead of the violation | 3/6 → **6/6** |
| A generated file to edit instead of a blank page | 0/4 → **4/4** |

Both numbers are from mechanical grading — the output had to compile, apply, contain the requested
change, and not damage anything else. The lesson from the second row is the sharper one: given the
complete vocabulary in the prompt but no starting file, a small model invented an API that did not
exist, every single time. **Recognition beats recall.** Documentation does not prevent hallucination;
a valid artifact to edit does.

So the design rule throughout: a change that makes WGE more expressive but materially harder to
drive, verify, or repair is a **regression**, not a feature. Model capability and cost are measured
alongside final mechanical and visual quality.

## Architecture

The native path has four language/runtime responsibilities, each doing the thing it is actually
best at:

| Layer | Language | Responsibility |
|---|---|---|
| Authoring DSL | Python (parsed, never executed) | What the author — human or model — writes |
| Native compiler & transaction authority | Rust | Typed lowering boundary, ProjectSpec, identity, receipts, gates, promotion |
| Numerical solvers | Julia | Terrain analysis, hydrology, placement solving, spatial fields |
| Provider and backend glue | Python | Source staging, image/Blender/Gaea/provider IO, process transport, adapters |
| Native runtime / graphics execution | Rust / Julia-Lava-Vulkan | Native delivery and deterministic runtime; Rust-owned gates are explicit |

### Compatibility/reference authoring DSL

The Python-shaped intent DSL below is retained as a compatibility and regression lane while the
Rust-owned native construction surface grows. It is not permission to move semantic authority back
into Python; new demo-critical operations belong behind typed Rust contracts and the native control
plane.

The language-wide design baseline is [docs/DSL docs/WGE_LANGUAGE_SPEC.md](docs/DSL%20docs/WGE_LANGUAGE_SPEC.md). It defines the planned
typed IR, safety boundary, geometry/asset and policy domains, backend contracts, conformance suite, and unresolved
decisions without treating unfinished proposals as language features.

Intent files look like Python and are **parsed, never executed** — no filesystem, network, process,
import, or runtime access. This is not only a security property. Because the file is data rather
than a program, it can be generated, diffed, repaired, and round-tripped:

```python
from worldbuilder import World, ridge_network

world = World('codeweald_alpine_arena_v1')

# eastern_alps -- alpine_massif (accepts: ridge_network)
world.landform(
    'eastern_alps',
    composition=ridge_network(
        spines=3,
        elevation_bias=0.72,
        along_jitter=0.08,
        cross_jitter=0.1,
    ),
)
```

There are no loops, conditionals, or functions. An intent file *describes* a world; it does not
compute one. Every rejection carries an executable repair, and a complete starting file can be
generated from any existing world.

## Current canonical status

The active product target is the demo-ready native engine slice in
[WGE_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md](WGE_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md), not
the older Codeweald target-engine MVP described in the historical sections below.

Green foundation: Rust-owned typed contracts, deterministic intake/project identity, registered
receipt authority, Julia terrain/spatial seams, native graphics packet execution, live-evidence
contracts, reference runtime/traversal machinery, bounded repair/rollback, and the native control
surface. The active gap is production composition: scene/object/asset lowering, live game-loop
integration, authored-quality materials/lighting/foliage/atmosphere, one native animated character
path, richer world population, style compilation/critique, and unattended fresh-agent packaging.

The supplied malformed GLB remains a permanent negative control. Blender is optional at build time;
it is never a runtime or semantic authority dependency. Unity, Unreal, and Godot are compatibility
or archaeological lanes only and are not demo gates.

Use the focused commands in [ACTIVE_ARCHITECTURE.md](ACTIVE_ARCHITECTURE.md) and the gates named by
the current campaign. Do not infer present status from the dated snapshot that follows.

The first reproducible native command is:

```sh
python3 pipeline/wge_native_gate.py
```

## Historical compatibility snapshot (not current status)

The vertical slice — `codeweald_alpine_arena_v1`, a 256×256m alpine arena — compiles end to end.
It does **not** currently pass every gate; see the failing gates below.

| Suite | Result |
|---|---|
| Python | 501/501, 161 subtests (2026-08-03) |
| Rust | 23/23 across the workspace (2026-08-03) |
| Julia | 8/8 (2026-08-03) |
| Bevy four-view visual acceptance | passed, zero failures |

Failing gates as of 2026-08-03, all known and tracked rather than surprises:

- **navmesh acceptance:** 3 requirements unmet. None of the three lanes runs end to end; the longest
  gap is 18 m on `central_lane`. Twelve structure placements obstruct a lane (D13) — but broken down
  by owner, those twelve block `central_lane` only. `north_lane` and `south_lane` are blocked mostly
  by keep *gate* components standing in the lane the gate exists to admit, which is a separate and
  newly logged defect (D29). Moving the villages will not clear this gate on its own.
- **`bevy_visual_acceptance` foliage floor:** 0.00682 against 0.008 (D28). *New, and not a
  regression.* The scatter now obeys `canopy_suitability`, the forests thinned to what the ecology
  supports, and a gate that counts green pixels tripped. Either the ecology is miscalibrated
  (decisions item 4) or the gate is measuring the wrong thing; the second world settles which.
- **`bevy_visual_acceptance` road readability:** 0.00686 against 0.008. Pre-existing and previously
  unrecorded — it was masked while the black-pixel gate was the loud failure.

**No longer failing:** the black-pixel gate reads 0.0645 against 0.08, down from 0.094 (D25). It was
not fixed directly. The old surfacing darkened the preview by up to 22% wherever wetland was, and
wetland was wrongly on the cliffs; slope-gating it removed the darkening. A good part of D25 was the
wetland bug.

Current world: 66,049 terrain vertices, 275 render-plan instances, 0 prop placements (intentional —
every landform is terrain-native), 3 compiled roads, certified traversal. The instance count fell
from 337 when the scatter began obeying the ecology; the difference is trees that were standing on
ground S6 says will not grow them.

Pipeline stages, all green: `compile → worldbuilder_dsl → evidence_overlay → style_reference →
runtime_effects → terrain → terrain_analysis → terrain_contract → traversal_probe →
terrain_materials → asset_catalog → asset_plan → asset_visual_preflight →
asset_physical_acceptance → julia_placement_solver → rust_placement_contract → rust_render_plan →
godot_adapter`.

### What is done

- Deterministic terrain compilation with a stable world identity hash
- Rust-certified provenance: analysis is bound to exact raster bytes, stale inputs are rejected
- Julia terrain analysis, hydrology with uphill-reversal rejection, placement solving
- Ecology-obeying foliage: the Rust scatter draws in proportion to S6's canopy suitability, and the
  authored instance count is a ceiling the terrain may refuse rather than a quota it must meet
- A five-layer terrain surface — grass, road, rock, snow, wetland — so peat and bog reach a material
  instead of rendering as grass
- Traversal certification — the walkable claims are verified, not asserted
- Material/texture gates that reject bad source art instead of certifying it
- Four-view Bevy audit captures (overview, both wall faces, player height)
- Authoring DSL with repair-carrying errors and scaffold generation
- Legacy external delivery adapters with an honest preflight boundary

### What is not done

- **Art fidelity.** This is the live frontier. Settlement and objective kits still read as prototype,
  foliage density needs work, wetland rendering is weak, the ground palette is flat, and cliff faces
  still show heightmap-slab grammar.
- **Gameplay/system DSL.** The `wge.game` vocabulary does not exist yet. When it does, it shares the
  authoring kernel with `wge.world` — one sandbox, two vocabularies, never two dialects.
- **The critic→DSL loop.** Audit metrics are still numbers. Turning `foreground_edge_density: 0.044`
  into a concrete suggested patch is what closes the loop and is the next major piece of work.

## Historical layout snapshot (not current instructions)

The engine and the game are separate git repositories as of 2026-08-02. WGE lives at the **workspace
root** (`Code Projects/WGE/`), not under `Game Projects/`. It is an engine that games consume, not a
game.

Engine paths, relative to `Code Projects/WGE/`:

| What | Where |
|---|---|
| Compiler, DSL, pipeline | `pipeline/` |
| Rust WorldSpec core | `world_core/` |
| Bevy reference renderer | `world_core/apps/world_viewer/` |
| Julia solvers | `terrain_lab/` |
| Legacy delivery adapters | `engine_adapters/` |
| Specifications and decision records | `docs/` |

Game paths, relative to `Code Projects/Game Projects/Codeweald/`:

| What | Where |
|---|---|
| Worlds and their evidence | `godot_renderer/concept_batches/` |
| Assets and map prep | `godot_renderer/` |

`godot_renderer/` keeps its name although the Godot pipeline was removed on 2026-07-31; the rename
is outstanding as D8.

A world is built by pointing the engine at a batch in the game repo:

```sh
python3 pipeline/build_zone.py <game>/concept_batches/<batch>/annotations.json --project-root <game>
```

A batch can live anywhere; the same batch compiled from two locations produces byte-identical
artifacts.

## Historical compatibility commands (not current native entry points)

Generate a starting intent file for an existing world — never author from a blank page:

```sh
python3 pipeline/worldbuilder_dsl.py <zone_spec.json> --output map.py --scaffold
```

Compile intent into WorldSpec patches:

```sh
python3 pipeline/worldbuilder_dsl.py map.py --output intent.json
```

Add `--lenient` while authoring to clamp out-of-range values and correct misspelled enums into a
reported repair list instead of failing. Certification always stays strict.

Run the suites:

```sh
python3 -m unittest discover -s tests        # from godot_renderer/
cargo test --workspace                       # from world_core/
julia --project=. test/runtests.jl           # from terrain_lab/
```

## Engine-neutral native MVP

The current model-facing vertical slice is driven by the Rust-owned native
contracts. `wge-control-plane` is the canonical project transaction, pointer,
candidate, receipt, repair, and promotion authority. Python authoring/provider
modules parse or stage caller-owned bytes and transport typed requests; they do
not decide semantic identity, gate status, or promotion. The Rust project
ledger compiles the typed intake/template into `ProjectSpec`, Julia supplies
bounded numerical fields, and the Rust reference runtime owns world,
traversal, gameplay, capture, and revalidation.

`semantic_kernel` and the older Python snapshot/build paths remain available
as compatibility/regression lanes. They cannot promote a native current
snapshot; the native transaction and registered Rust validators are the source
of truth for the converged path. The Neura-MCP launcher exposes that same
bounded operation surface without moving semantics into MCP.

Run the focused acceptance suite:

```sh
python3 -m unittest tests.test_wge_native_mvp -v
```

The model-facing command is:

```sh
python3 pipeline/wge_native_mvp.py \
  --source-dir <brief-concept-and-layout> \
  --project-template <typed-project-template.json> \
  --rigging-glb <provider-output.glb> \
  --rigging-request <typed-rigging-request.json> \
  --output-dir <certified-handoff>
```

That command remains the native-MVP regression oracle. The engine-neutral
convergence path uses the same typed intake/world artifacts through
`wge-control-plane` and deliberately leaves production rigging, skinning,
retargeting, and arbitrary mesh-to-character generation deferred. The
supplied bad GLB remains a permanent negative control. This checkpoint is
judged by WGE's own semantic, mechanical, traversal, visual, repair,
determinism, provenance, and archive-revalidation gates.

## As substrate for a world model (Project Aisling)

> Design intent, not measured. Aisling is not trained yet. Recorded here because it shapes what WGE
> should expose.

The reason a world compiler matters to a generative world model is that **it moves everything
solvable out of the weights.**

A model asked to emit a world directly must learn geometry, drainage, collision, navigability, and
material coherence — all of which are *already solved problems* with exact algorithms. It will learn
them approximately, at enormous parameter cost, and still produce rivers that flow uphill. Against
WGE, the model emits ~50 lines of intent and the compiler does the rest exactly. The model's job
shrinks from "represent a world" to "have taste about a world."

Four concrete uses:

1. **The output space collapses.** ~50 lines of DSL instead of a 66,049-vertex mesh. This is what
   makes a model viable as a world author: it emits a compact, typed intent surface instead of
   having to serialize a coherent mesh directly. Same thesis as the skill floor, one level up.
2. **A programmatic verifier.** WGE's gates are exact, not learned — traversal certification,
   hydrology reversal rejection, provenance binding, style scoring. That makes them usable as an RL
   reward signal that **cannot be hacked the way a neural critic can**, because there is no critic
   to fool. The world either has three walkable roads or it does not.
3. **A labelled data factory.** Every compiled world carries ground truth: heightfields, semantic
   annotations, traversal graphs, hydrology, and four canonical views from known camera poses.
   3D-consistent labelled data is exactly what generative world models are starved of, and WGE emits
   it as a byproduct of doing its normal job.
4. **Faithfulness stays checkable.** "Does this match the concept art" remains a measured quantity
   rather than a human judgement call, which is what makes iteration tractable.

The honest caveat: this only holds while the DSL stays expressive enough to describe worlds worth
generating. If the vocabulary is too thin, the model's taste has nowhere to go. Growing the
vocabulary without raising the skill floor is the standing tension.

## Relationship to Codeweald

Codeweald is the first consumer, not the owner. It is a fantasy RPG/arena target that exercises WGE
against a real art brief, and the two ping-pong: engine capability unlocks map fidelity, and map
defects drive engine work.

Codeweald's game DSL and WGE's world DSL are intended to converge on the same shape — one authoring
kernel, two vocabularies — so that a model fluent in one is already fluent in the other.
