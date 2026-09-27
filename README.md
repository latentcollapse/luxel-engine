# WGE — WorldGen Engine

**Concept art in. A playable, faithful 3D map out.**

WGE is a game engine. You give it concept art and annotations describing a place; it produces a
certified, deterministic world — terrain, hydrology, traversal, roads, settlements, materials,
placements — and then hands that world to whichever renderer you want to look at it in.

## An AI-native engine, not a human-native one

Godot, Unity and Unreal are **human-native** engines. Their primary interface is a GUI built for a
person with a mouse, their primary verification is a human looking at the viewport, and their
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
| Skill floor | A trained artist | A 3B-class model |

What makes something an engine is that it owns the authoritative representation of the world and
the rules for constructing it — terrain, hydrology, traversal, collision, navigation, placement,
materials, provenance. WGE owns all of that. Rasterization is one subsystem, and it is the one
subsystem WGE deliberately does not implement.

That delegation is an architectural choice, not a disqualification. Godot, Unreal 5, and Unity are
**backends**: consumers of WGE output, interchangeable by design and individually droppable. If a
change to WGE can only be expressed in one of them, that change is in the wrong layer.

**Bevy is not a backend.** It is WGE's *reference renderer* — the instrument WGE uses to look at its
own output, plus the surface for lightweight inspection and editing. It ships with WGE and is not
swappable. Audit captures deliberately route through it so that no target engine's quirks
contaminate the measurement: a visual gate must measure the world, not the engine. Treating the
reference renderer as just another backend would mean acceptance thresholds silently move whenever
you change engines.

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

## The skill floor is the point

The success criterion for WGE is **not** that a frontier model can drive it. It is that a **3B-class
model can**. Opus building a map with WGE proves nothing. A small local model building a playable
map from concept art is a different thing entirely.

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

So the design rule throughout: a change that makes WGE more expressive but harder for a small model
to drive is a **regression**, not a feature.

## Architecture

Four languages, each doing the thing it is actually best at:

| Layer | Language | Responsibility |
|---|---|---|
| Authoring DSL | Python (parsed, never executed) | What the author — human or model — writes |
| Compiler & pipeline | Python | Concept intake, rasterization, asset planning, adapters |
| Contracts & identity | Rust | WorldSpec validation, provenance, stable world hashing |
| Solvers | Julia | Terrain analysis, hydrology, placement solving |
| Backends | Godot / UE5 / Unity / Bevy | Rendering and runtime only |

### The authoring DSL

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

## Status

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
- Unity and Unreal adapters with an honest preflight boundary

### What is not done

- **Art fidelity.** This is the live frontier. Settlement and objective kits still read as prototype,
  foliage density needs work, wetland rendering is weak, the ground palette is flat, and cliff faces
  still show heightmap-slab grammar.
- **Gameplay/system DSL.** The `wge.game` vocabulary does not exist yet. When it does, it shares the
  authoring kernel with `wge.world` — one sandbox, two vocabularies, never two dialects.
- **The critic→DSL loop.** Audit metrics are still numbers. Turning `foreground_edge_density: 0.044`
  into a concrete suggested patch is what closes the loop and is the next major piece of work.

## Layout

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
| Unity / Unreal adapters | `engine_adapters/` |
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

## Usage

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

## MVP vertical slice

The current game-development harness slice is driven by the Rust-owned project
ledger. The Python command is only orchestration: it copies and hashes pinned
inputs, then asks Rust to validate, certify, and emit the Unity handoff.

```sh
python3 pipeline/wge_mvp.py prepare --output <candidate>
python3 pipeline/wge_mvp.py verify-all \
  --spec <candidate>/project_spec.json \
  --evidence <candidate>/evidence.json \
  --snapshot-output <candidate>/project_snapshot.json \
  --unity-output <candidate>/wge_unity_mvp_import.json
```

`verify-all` exits nonzero and reports the blocked gates until target-runtime
evidence has actually been observed. It never upgrades an indeterminate Unity
import, playthrough, or visual result into a pass. A certified handoff is
imported through **Tools > Codeweald > WGE MVP > Import Certified Snapshot** in
the Unity adapter. The conventional-engine comparison is reported by
`pipeline/benchmark_mvp.py` only after both measured runs exist.

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
   makes a small model viable as a world author at all: a 3B can emit a paragraph of Python, and
   cannot emit a coherent mesh. Same thesis as the skill floor, one level up.
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
