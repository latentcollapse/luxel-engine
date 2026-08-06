# Codeweald World Viewer

This is the lightweight Bevy inspection backend for a **compiled** Codeweald
world. It does not author topology or rerun solvers. It validates and consumes
the same ZoneSpec, terrain manifest, placement plan, and asset digest map used
by the Godot backend.

The first vertical slice renders:

- a one-metre inspection mesh built from the certified float32 heightfield;
- the registered low-frequency concept colour field;
- every Julia-solved, Rust-certified landform placement through its glTF/GLB;
- corridor and hydrology overlays from ZoneSpec;
- a lightweight procedural inspection sky and ambient lighting;
- a free camera and an automatically refreshing status HUD.

The viewer polls canonical compiled artifacts twice per second. A successful
change atomically replaces the displayed world; an invalid intermediate write
leaves the prior world visible and reports the error. Bevy separately hot
reloads referenced scenes, meshes, and textures.

```bash
cd world_core
cargo run -p codeweald-world-viewer -- \
  --batch ../godot_renderer/concept_batches/codeweald_alpine_arena_v1
```

Controls:

- Right mouse: hold to look.
- `W` / `S`: move forward and backward.
- `A` / `D`: strafe left and right.
- `Space` / `Z`: move straight up and down.
- Shift: accelerate.
- Mouse wheel: change movement speed.
- `M`: toggle captured mouse.
- `F`: frame the compiled world bounds.
- `1`: corridor overlay.
- `2`: hydrology overlay.
- `R`: force a compiled-world reload.

Godot remains an acceptance oracle while this backend acquires terrain
materials, all dressing layers, semantic picking, corrections, navigation,
physics, and deterministic capture parity.

Deterministic visual-audit capture:

```bash
cargo run -p codeweald-world-viewer -- \
  --batch ../godot_renderer/concept_batches/codeweald_alpine_arena_v1 \
  --capture /tmp/codeweald-overview.png \
  --view overview

python ../godot_renderer/pipeline/bevy_visual_acceptance.py \
  ../godot_renderer/concept_batches/codeweald_alpine_arena_v1/zone_spec.json \
  /tmp/codeweald-overview.png \
  --output ../godot_renderer/concept_batches/codeweald_alpine_arena_v1/bevy_visual_acceptance_report.json
```

Capture mode frames the certified world, waits for asynchronous glTF and
texture loads to settle, saves the image, and exits. The acceptance report
fails closed on empty, dark, low-contrast, clipped, roadless, foliage-free, or
waterless renders.

`--view` also accepts `west-wall`, `east-wall`, and `player` for canonical
low-angle silhouette and player-scale audits. These views consume the same
certified world and use deterministic camera contracts.

The model-facing one-command form runs both steps:

```bash
python ../godot_renderer/pipeline/capture_bevy.py \
  ../godot_renderer/concept_batches/codeweald_alpine_arena_v1

# Canonical visual audit: overview, west/east walls, and player-height view.
python ../godot_renderer/pipeline/capture_bevy.py \
  ../godot_renderer/concept_batches/codeweald_alpine_arena_v1 --suite
```
