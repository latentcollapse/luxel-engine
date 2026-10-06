# Luxel Engine Roadmap

2026-10-06 · Matt · the layered maturation path. Each layer stands on the one above it.

```text
WORLD          terrain → atmosphere → materials → lighting → vegetation → architecture
CHARACTERS     import → skeleton → weights → skinning → animation → equipment sockets
DEFORMATION    conventional LBS first; TetCage where measured advantageous
GAMEPLAY       attributes → abilities → effects → tags → cooldowns → targeting
               → damage → resources → status → progression
CONTENT SDK    semantic authoring contracts
ADD-ONS        specialised creative surfaces over Luxel truth (Missions, Armory, Studio, ...)
```

Under every layer sits the **platform**: Rust authority, the native graphics
contract, Julia/Lava on the GPU, receipts and identity. Tools Luxel drives from
outside (Gaea, Blender, ComfyUI, the Unity/Unreal/Godot adapters) are
**integrations**, not layers.

Principles carried through every layer:

- Prove one layer before leaning on the next. The world must be somewhere
  worth being before characters are worth building; characters must work
  conventionally before TetCage is asked to improve them.
- Add-ons are views and authoring surfaces over Luxel truth; they never own it.
- Every new axis is absent by default and byte-identical when absent
  (`tools/verify_identity.py`).

## World — ACTIVE

Goal: Luxel can depict a world worth building things inside. The bar is
Witcher-3-class completeness, and the stress test after CONVERGE-4 is a giant
redwood/sequoia forest settlement (the "Sidhe" references: Knothole Glade,
Grizzly Hills, the Pacific Northwest).

| Stage | State |
|---|---|
| Terrain | Gaea-driven landform (`docs/integrations/gaea-programme.md`), scanned terrain layers (N-4), km-scale backdrop (CONVERGE-4 N-3) |
| Atmosphere | analytic sky + aerial perspective (N-1); km-scale haze (N-3) |
| Materials | scanned PBR sets, IBL (N-2); wear/wetness layers not yet |
| Lighting | one view-fitted shadow map; cascades next (CONVERGE-4 L-1); contact AO (N-8); local lights, light shafts not yet |
| Vegetation | alpha-to-coverage foliage, scatter + forest (N-7); one tree species, regular spacing, wrong biome (B-1) |
| Architecture | one imported ruin kit (N-5, R-1) |

Contracts: `docs/world/converge/` (CONVERGE-1..4). Plan: `docs/world/landscape-parity-plan.md`.

What the Sidhe scene will need that does not exist yet: hero-scale trees
(trunks as landform), volumetric light shafts (L-9), many local lights (L-4),
flowing water (L-5), moss/wet material layers, a settlement kit with paths and
bridges, deep forest fog, and output resolution / anti-aliasing past 960×640.

## Characters — NOT STARTED

Import → skeleton → weights → skinning → animation → equipment sockets. The
schema has no joints, weights or animation channels yet (parity audit GP-09).
Existing seed: `pipeline/rigging_provider.py` (structural control only).

## Deformation — RESEARCH

Conventional linear-blend skinning is the baseline. TetCage
(`docs/deformation/tetcage-seam-design.md`, `graphics_lab/tet-lab/`) joins
where it is measured to buy something: crowds, creatures, secondary motion,
foliage. It does not rescue the character pipeline.

## Gameplay — FOUNDATION ONLY

A Luxel-native capability model (preconditions, costs, targeting, phases,
effects, tags, interruptibility, animation/VFX/audio intent, authority policy,
telemetry), not a clone of Unreal GAS. Existing: the gameplay contract crate,
GCS capability resolution, the deterministic reference runtime
(`docs/gameplay/`).

## Content SDK — FOUNDATION ONLY

Semantic authoring contracts that Cyan and add-ons speak. Existing: the
semantic facade, construction plans, the semantic kernel, and the frozen
model-native language spec (`docs/DSL docs/LUXEL_LANGUAGE_SPEC.md`, hash-pinned
by `tests/dsl_conformance/freeze_v06.json`; do not move or edit it without
re-freezing).

## Add-ons — SPECIFIED ONLY

Small semantic applications over one world model: Missions (quests),
Armory (gear, with Grindstone as its weapon-design lab), Studio (characters
for any purpose: game, MMO NPC, VTuber, film), and third-party add-ons. Specs
so far: `docs/add-ons/grindstone-spec.md`, `docs/add-ons/reforge-spec.md`.
