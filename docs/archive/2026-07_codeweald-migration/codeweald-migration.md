# Migrating Luxel out of `Codeweald/godot_renderer/`

> **Historical migration record.** This document describes an earlier
> multi-target layout and is retained for archaeology and salvage only. It is
> not the current product doctrine or execution plan. Use
> [`../ACTIVE_ARCHITECTURE.md`](../../platform/active-architecture.md) and
> [`../LUXEL_DEMO_READY_NATIVE_ENGINE_MEGA_SPRINT.md`](../2026-09_roadmaps-and-audits/demo-ready-mega-sprint.md)
> for the native Rust/Julia/Lava path. Godot, Unity, and Unreal references
> below must not be interpreted as current runtime gates.

## Why

`godot_renderer/` is a historical name that no longer describes its contents. Of the code in there,
45 modules are engine-agnostic Python and 16 are Godot `.gd` scripts. Unity and Unreal adapters
already live alongside them. The directory name asserts a dependency that mostly does not exist, and
it makes Luxel look like a Godot subsystem rather than a general-purpose world compiler.

## Target layout

Luxel now lives at the workspace root (`Code Projects/luxel-engine/`), deliberately outside `Game Projects/` —
it is a tool that games consume, not a game.

```
Luxel/
  README.md
  docs/
  luxel/                  compiler, DSL, pipeline  (from godot_renderer/pipeline/*.py)
  world_core/           Rust WorldSpec  (moved wholesale)
  terrain_lab/          Julia solvers  (moved wholesale)
  viewer/               Bevy reference renderer -- first-party, NOT a backend
  backends/
    godot/              the 16 .gd scripts, project.godot, addons, FBX
    unity/              from engine_adapters/unity
    unreal/             from engine_adapters/unreal
  tests/
  worlds/               from concept_batches/
```

## Three layers, not two

The earlier draft of this document treated Bevy as one backend among several. That is wrong, and it
produces the wrong layout. There are three distinct roles:

1. **Core** — produces and certifies world data. Would still be needed if the world were never
   rendered at all, only inspected as data.
2. **Reference renderer** — Bevy. This is Luxel's *own instrument*: the eye it uses to check its own
   work, and the surface for lightweight inspection and editing commands. It ships **with** Luxel and
   is not optional or swappable. Audit captures route through it precisely so that no target
   engine's quirks contaminate the measurement.
3. **Target backends** — Godot, Unreal, Unity. Consumers of finished world data, reached through
   adapters, fully interchangeable, each individually droppable.

The distinction matters practically: if the reference renderer were "just another backend", then
every visual gate would be measuring a backend rather than measuring the world, and swapping
backends would silently move the acceptance thresholds.

## Classification rule

Ask: **would this module still be needed if the world were never rendered by any target engine —
only compiled, certified, and inspected?**

- Yes → `luxel/` (or `world_core/`, `terrain_lab/` by language)
- Yes, but only to *look at* the world for verification → `viewer/`
- No, it exists to hand the world to a named third-party engine → `backends/<engine>/`

Note that 18 pipeline modules mention "godot", but most only do so to *emit* a Godot adapter
manifest — that is Luxel producing backend output, not Luxel depending on Godot. Classify by what the
module would do if Godot were deleted, not by grep hits.

## Order of operations

Do these as separate, individually verifiable steps. Do not batch them.

1. **`terrain_lab/` → `Luxel/terrain_lab/`.** Lowest risk: self-contained Julia project, referenced by
   path from the pipeline in a small number of places. Verify with `julia --project=. test/runtests.jl`.
2. **`world_core/` → `Luxel/world_core/`.** Self-contained Cargo workspace. Verify with
   `cargo test --workspace`.
3. **`concept_batches/` → `Luxel/worlds/`.** Data, but heavily path-referenced by reports and tests.
   The production zone spec is referenced by an absolute-ish path in the scaffold round-trip test.
4. **`pipeline/*.py` → `Luxel/luxel/`.** The big one. Leave the `.gd` files behind.
5. **`tests/` → `Luxel/tests/`.** Update the `parents[1] / "pipeline"` sys.path insert.
6. **Godot leftovers stay** in Codeweald and become `backends/godot/` by reference, or move if the
   Godot project can tolerate it.

## Known coupling to fix

- `tests/*.py` insert `Path(__file__).resolve().parents[1] / "pipeline"` on `sys.path`.
- `test_worldbuilder_dsl.py` resolves the production zone spec through
  `parents[1] / "concept_batches" / ...` and skips if absent — it will silently skip rather than
  fail if the path breaks, so **check for skips, not just passes**, after moving.
- `build_zone.py` imports pipeline modules directly and names stage order including
  `worldbuilder_dsl` and `godot_adapter`.
- `codeweald` launcher hardcodes `CANONICAL_PROJECT_DIR = /mnt/d/Language Projects/...` — already
  stale (wrong mount *and* wrong workspace name) and needs fixing regardless of this migration.
- Reports under `concept_batches/` embed absolute paths in `bevy_visual_acceptance_report.json` and
  similar. These are provenance records; regenerate rather than rewrite them.

## Verification gate

The migration is done when, from the new locations:

- Python 83/83 with **zero skips**
- Rust 15/15
- Julia 8/8
- `worldbuilder_dsl.py <zone_spec.json> --output map.py --scaffold` produces 8 landforms
- scaffold → compile → apply round-trips the production zone with zero composition drift

## Caution

Codex (the pipeline's primary author) is rate-limited until **2026-08-06** and returns with muscle
memory for the old paths. Whatever the final layout, record it on the board so the move is not
rediscovered as breakage.
