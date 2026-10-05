"""Headless Blender step of tools/build_kit.py: decimate one triangle mesh.

Run as:
    blender -b --factory-startup --python tools/blender_decimate.py -- IN.obj OUT.obj TARGET_TRIANGLES

Reads an OBJ written by build_kit.py (positions, normals, one UV set),
collapses it to at most TARGET_TRIANGLES with UV seams delimited so texture
islands survive, recomputes smooth normals, and writes a triangulated OBJ
with normals and UVs. Blender is deterministic for identical input and
version; build_kit.py records the version and pins the output digests.
"""

import sys

import bpy

source, target, triangles = sys.argv[sys.argv.index("--") + 1:]
triangles = int(triangles)

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.wm.obj_import(filepath=source)
(obj,) = [o for o in bpy.context.scene.objects if o.type == "MESH"]
bpy.context.view_layer.objects.active = obj
obj.select_set(True)

current = sum(len(polygon.vertices) - 2 for polygon in obj.data.polygons)
if current > triangles:
    modifier = obj.modifiers.new("decimate", "DECIMATE")
    modifier.decimate_type = "COLLAPSE"
    modifier.ratio = triangles / current
    modifier.use_collapse_triangulate = True
    modifier.delimit = {"SEAM", "UV"}
    bpy.ops.object.modifier_apply(modifier="decimate")
bpy.ops.object.shade_smooth()

bpy.ops.wm.obj_export(
    filepath=target,
    export_selected_objects=True,
    export_uv=True,
    export_normals=True,
    export_materials=False,
    export_triangulated_mesh=True,
    apply_modifiers=True,
    forward_axis="NEGATIVE_Z",
    up_axis="Y",
)
