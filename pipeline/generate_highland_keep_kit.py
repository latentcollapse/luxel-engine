"""Generate a portable, textured Highland war-keep landmark kit.

The kit is deliberately engine-neutral: Blender emits the authored GLB and a
Unity FBX sidecar, while the ZoneSpec only asks for a `fortification` asset.
Realm colour remains a runtime banner effect, so the same verified source can
serve Albion, Hibernia, Midgard, or a future neutral faction without forking
geometry in every adapter.

**The layout lives in `highland_keep_geometry`, not here.** This file is the
Blender shell: it turns declared parts into meshes and exports them. Everything
that decides whether the keep is a building rather than a wall -- where the gate
is, which parts would close it, how much courtyard survives -- is pure
arithmetic in that module, so it is unit-testable without Blender (D3).

Alongside each GLB it writes an **affordance sidecar**: the measured claim that
this asset can be entered, and the agent it was measured against. That claim is
what `asset_physical_acceptance` re-verifies before the asset is allowed into a
world, so a keep that stops being enterable cannot quietly ship again (D17).
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import bpy

sys.path.insert(0, str(Path(__file__).resolve().parent))

from highland_keep_geometry import (  # noqa: E402
    VARIANTS,
    KeepSpec,
    Part,
    affordances,
    keep_parts,
    verify,
)

# Defaults, not truths. `agent_*` mirror `zone_compiler.DEFAULT_TRAVERSAL_POLICY`
# and `placed_scale` mirrors the `highland_fortified_keep` asset profile's
# `scale_m`. They are arguments because "does this keep work" is a question
# about a *world*, not about the kit: the same geometry that passes for a 2.5 m
# agent seals shut for a 5 m one, and baking either number in here is how a kit
# silently stops being valid for the zone that uses it.
DEFAULT_AGENT_RADIUS_M = 2.5
DEFAULT_AGENT_MAX_CLIMB_M = 4.0
DEFAULT_PLACED_SCALE = 0.1306

MATERIALS = {
    # Values are authored as linear Blender material values. Their low-key
    # palette deliberately sits in the same overcast family as the source art,
    # while battlements, roofs, and timber retain readable silhouette contrast.
    "stone": ("Highland granite", (0.085, 0.105, 0.100, 1.0), 0.93, 0.0),
    "dark_stone": ("Weathered wall shadow", (0.022, 0.030, 0.028, 1.0), 0.97, 0.0),
    "slate": ("Slate roof", (0.015, 0.027, 0.038, 1.0), 0.72, 0.0),
    "timber": ("Gatehouse timber", (0.052, 0.022, 0.009, 1.0), 0.88, 0.0),
    "iron": ("Gate iron", (0.022, 0.027, 0.029, 1.0), 0.48, 0.55),
}


def _arguments() -> tuple[Path, float, float, float]:
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if not argv:
        raise SystemExit(
            "Usage: blender --background --python generate_highland_keep_kit.py -- "
            "OUTPUT_DIRECTORY [--agent-radius-m R] [--agent-max-climb-m C] "
            "[--placed-scale S]"
        )
    output = Path(argv[0]).resolve()
    values = {
        "--agent-radius-m": DEFAULT_AGENT_RADIUS_M,
        "--agent-max-climb-m": DEFAULT_AGENT_MAX_CLIMB_M,
        "--placed-scale": DEFAULT_PLACED_SCALE,
    }
    rest = argv[1:]
    while rest:
        flag = rest.pop(0)
        if flag not in values:
            raise SystemExit("unknown option %r" % flag)
        if not rest:
            raise SystemExit("%s needs a value" % flag)
        values[flag] = float(rest.pop(0))
    return output, values["--agent-radius-m"], values["--agent-max-climb-m"], values["--placed-scale"]


def _materials() -> dict[str, bpy.types.Material]:
    built: dict[str, bpy.types.Material] = {}
    for key, (name, color, roughness, metallic) in MATERIALS.items():
        material = bpy.data.materials.new(name)
        material.use_nodes = True
        principled = material.node_tree.nodes.get("Principled BSDF")
        principled.inputs["Base Color"].default_value = color
        principled.inputs["Roughness"].default_value = roughness
        principled.inputs["Metallic"].default_value = metallic
        built[key] = material
    return built


def _emit(part: Part, materials: dict[str, bpy.types.Material], root: bpy.types.Object) -> bpy.types.Object:
    # Kit coordinates are declared in the shared engine convention (X/Z ground,
    # Y up). Blender is Z-up, so both location and box dimensions must cross
    # that boundary here. Keeping it in one helper prevents a visually plausible
    # but sideways GLB from escaping into a Godot/Unity/UE asset plan.
    location = (part.centre[0], part.centre[2], part.centre[1])
    if part.shape == "box":
        bpy.ops.mesh.primitive_cube_add(location=location)
        obj = bpy.context.object
        obj.scale = (part.size[0] * 0.5, part.size[2] * 0.5, part.size[1] * 0.5)
        bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    elif part.shape == "cylinder":
        bpy.ops.mesh.primitive_cylinder_add(
            vertices=part.vertices, radius=part.size[0] * 0.5, depth=part.size[1], location=location
        )
        obj = bpy.context.object
    elif part.shape == "cone":
        bpy.ops.mesh.primitive_cone_add(
            vertices=part.vertices, radius1=part.size[0] * 0.5, radius2=0.0,
            depth=part.size[1], location=location,
        )
        obj = bpy.context.object
    else:
        raise SystemExit("unknown part shape %r on %s" % (part.shape, part.name))
    obj.name = part.name
    if part.bevel:
        modifier = obj.modifiers.new("Weathered_edges", "BEVEL")
        modifier.width = part.bevel
        modifier.segments = 2
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.modifier_apply(modifier=modifier.name)
    obj.data.materials.append(materials[part.material])
    # Rotation is applied to the object, never baked into the mesh, so the part
    # keeps its own origin as its pivot -- the convention D3's roof-pivot bug
    # came from breaking.
    if part.yaw_radians:
        obj.rotation_euler[2] = part.yaw_radians
    obj.parent = root
    return obj


def _build(spec: KeepSpec) -> bpy.types.Object:
    root = bpy.data.objects.new("HighlandFortifiedKeep_%s" % spec.key.upper(), None)
    bpy.context.collection.objects.link(root)
    materials = _materials()
    for part in keep_parts(spec):
        _emit(part, materials, root)
    return root


def main() -> None:
    output, agent_radius_m, max_climb_m, scale = _arguments()
    output.mkdir(parents=True, exist_ok=True)

    # Refuse before building, not after exporting: a sealed keep should never
    # exist on disk to be picked up by a later run.
    for spec in VARIANTS:
        problems = verify(spec, scale=scale, agent_radius_m=agent_radius_m, max_climb_m=max_climb_m)
        if problems:
            raise SystemExit("\n".join(problems))

    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete(use_global=False)
    for spec in VARIANTS:
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete(use_global=False)
        root = _build(spec)
        destination = output / ("highland_fortified_keep_" + spec.key)
        bpy.ops.wm.save_as_mainfile(filepath=str(destination.with_suffix(".blend")))
        bpy.ops.object.select_all(action="DESELECT")
        root.select_set(True)
        for child in root.children_recursive:
            child.select_set(True)
        bpy.context.view_layer.objects.active = root
        bpy.ops.export_scene.gltf(
            filepath=str(destination.with_suffix(".glb")), export_format="GLB",
            use_selection=True, export_materials="EXPORT", export_normals=True, export_apply=True,
        )
        unity = output / "unity"
        unity.mkdir(parents=True, exist_ok=True)
        bpy.ops.export_scene.fbx(
            filepath=str(unity / ("highland_fortified_keep_" + spec.key + ".fbx")),
            use_selection=True, apply_unit_scale=True, bake_space_transform=False,
        )
        contract = affordances(spec, scale=scale, agent_radius_m=agent_radius_m, max_climb_m=max_climb_m)
        destination.with_suffix(".affordances.json").write_text(
            json.dumps(contract, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(
            "Generated Highland fortified keep variant %s at %s (threshold %.2f m, "
            "courtyard ring %.2f m, agent radius %.1f m)"
            % (
                spec.key,
                destination.with_suffix(".glb"),
                contract["enterable"]["threshold_clear_m"],
                contract["enterable"]["interior_ring_m"],
                agent_radius_m,
            )
        )


if __name__ == "__main__":
    main()
