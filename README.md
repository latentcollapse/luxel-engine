# WGE — WorldGen Engine

**Concept art in. A playable, faithful 3D map out.**

WGE is a world compiler. You give it concept art and annotations describing a place; it produces a
certified, deterministic world — terrain, hydrology, traversal, roads, settlements, materials,
placements — and then hands that world to whichever renderer you want to look at it in.

## What WGE is not

**WGE is not a game engine, and it is not a Godot project.** It has no runtime, no scene graph, no
input system, no physics loop. It never draws anything.

Godot, Unreal 5, and Unity are **backends**. They are consumers of WGE output, interchangeable by
design and individually droppable. If a change to WGE can only be expressed in one of them, that
change is in the wrong layer.

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

The vertical slice — `codeweald_alpine_arena_v1`, a 256×256m alpine arena — compiles end to end and
passes every gate.

| Suite | Result |
|---|---|
| Python | 83/83 |
| Rust | 15/15 |
| Julia | 8/8 |
| Bevy four-view visual acceptance | passed, zero failures |

Current world: 66,049 terrain vertices, 337 render-plan instances, 0 prop placements (intentional —
every landform is terrain-native), 3 compiled roads, certified traversal.

Pipeline stages, all green: `compile → worldbuilder_dsl → evidence_overlay → style_reference →
runtime_effects → terrain → terrain_analysis → terrain_contract → traversal_probe →
terrain_materials → asset_catalog → asset_plan → asset_visual_preflight →
asset_physical_acceptance → julia_placement_solver → rust_placement_contract → rust_render_plan →
godot_adapter`.

### What is done

- Deterministic terrain compilation with a stable world identity hash
- Rust-certified provenance: analysis is bound to exact raster bytes, stale inputs are rejected
- Julia terrain analysis, hydrology with uphill-reversal rejection, placement solving
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

> **Migration in progress.** WGE currently lives inside `Codeweald/godot_renderer/` for historical
> reasons — that path predates the realisation that this is a general-purpose tool. The name is
> actively misleading: most of what is in there is engine-agnostic, and the Godot-specific part is a
> minority of it. See `docs/MIGRATION.md` for the file-level plan.

Today:

WGE lives at the **workspace root** (`Code Projects/WGE/`), not under `Game Projects/`. It is a
general-purpose tool that games consume, not a game.

Paths below are relative to the workspace root:

| What | Where |
|---|---|
| Compiler, DSL, pipeline (45 Python modules) | `Game Projects/Codeweald/godot_renderer/pipeline/` |
| Rust WorldSpec core | `Game Projects/Codeweald/world_core/` |
| Bevy reference renderer | `Game Projects/Codeweald/world_core/apps/world_viewer/` |
| Julia solvers | `Game Projects/Codeweald/terrain_lab/` |
| Unity / Unreal adapters | `Game Projects/Codeweald/godot_renderer/engine_adapters/` |
| Godot backend (16 `.gd` scripts, project files) | `Game Projects/Codeweald/godot_renderer/` |
| Worlds and their evidence | `Game Projects/Codeweald/godot_renderer/concept_batches/` |

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
