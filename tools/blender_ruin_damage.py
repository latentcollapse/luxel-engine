"""Headless Blender: fracture a modular wall piece (CONVERGE-3 R-1).

Usage (driven by tools/ruin_damage.py):
    blender -b --factory-startup --python tools/blender_ruin_damage.py -- SPEC.json

SPEC = {
  "input": "piece.obj",          # one object per source material (usemtl)
  "output": "damaged.obj",       # the piece with the broken-off cells removed
  "debris": "debris.obj",        # one object per broken-off cell and source
                                 # material ("cell_<k>__<material>")
  "seeds": [[x, y, z], ...],     # Voronoi sites, piece coordinates
  "removed": [k, ...],           # indices of the sites whose cells break off
  "squash": [1, 1.6, 1],         # cells are built in coordinates scaled by this,
                                 # so they come out wider than tall, like courses
  "bounds": [[x0, y0, z0], [x1, y1, z1]],
  "neighbor_radius": r,          # sites farther apart than r (squashed) are
                                 # not neighbours; must exceed twice the spacing
  "close_shell": true            # fill the piece's open boundaries first
}

The removed cells are subtracted from the piece; each one intersected with the
piece is a piece of debris whose outer faces are the wall's own, so what lies on
the ground is what broke off. Faces the cells create carry the material "CUT"
(the caller gives them stone and box-projected UVs); the OBJ files name each
face's material with `usemtl`. Deterministic: the caller seeds the sites.
"""

import json
import sys

import bmesh
import bpy
from mathutils import Vector


def load(path):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    # Raw coordinates (no Y-up to Z-up conversion): height stays on Y.
    bpy.ops.wm.obj_import(filepath=path, forward_axis="Y", up_axis="Z")
    return [ob for ob in bpy.context.scene.objects if ob.type == "MESH"]


def close_shell(ob):
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-4)
    bmesh.ops.holes_fill(bm, edges=bm.edges, sides=0)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(ob.data)
    bm.free()


def voronoi_cell(index, sites, bounds, radius):
    """The convex Voronoi cell of `sites[index]` within `bounds` (all in the
    squashed space), as a closed bmesh: the bounds box cut by every bisector."""
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    lo, hi = Vector(bounds[0]), Vector(bounds[1])
    for v in bm.verts:
        v.co = Vector(lo[i] + (hi[i] - lo[i]) * (v.co[i] + 0.5) for i in range(3))
    site = sites[index]
    for j, other in enumerate(sites):
        # Only near sites bound a cell when sites are dense; far ones cannot.
        if j == index or (other - site).length > radius:
            continue
        normal = other - site
        middle = (site + other) / 2
        # Keep the half-space nearer `site`; fill the new opening (convex).
        result = bmesh.ops.bisect_plane(bm, geom=bm.verts[:] + bm.edges[:] + bm.faces[:], plane_co=middle,
                                        plane_no=normal, clear_outer=True)
        cut_edges = [e for e in result["geom_cut"] if isinstance(e, bmesh.types.BMEdge)]
        if cut_edges:
            bmesh.ops.holes_fill(bm, edges=cut_edges, sides=0)
        if not bm.faces:
            break
    return bm


def cell_object(index, sites, bounds, squash, material, radius):
    bm = voronoi_cell(index, sites, bounds, radius)
    for v in bm.verts:
        v.co = Vector(v.co[i] / squash[i] for i in range(3))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    mesh = bpy.data.meshes.new(f"cell_{index}")
    bm.to_mesh(mesh)
    bm.free()
    ob = bpy.data.objects.new(f"cell_{index}", mesh)
    ob.data.materials.append(material)
    bpy.context.scene.collection.objects.link(ob)
    return ob


def apply_boolean(target, tool, operation):
    modifier = target.modifiers.new("cut", "BOOLEAN")
    modifier.operation = operation
    modifier.solver = "EXACT"
    modifier.use_hole_tolerant = True
    # Faces from the tool keep its material ("CUT"); the default (index based)
    # would give them the target's first slot.
    modifier.material_mode = "TRANSFER"
    modifier.object = tool
    bpy.context.view_layer.objects.active = target
    bpy.ops.object.modifier_apply(modifier=modifier.name)


def duplicate(ob, name):
    copy = ob.copy()
    copy.data = ob.data.copy()
    copy.name = name
    bpy.context.scene.collection.objects.link(copy)
    return copy


def merge_coplanar(ob):
    """Booleans split every face they touch; dissolving coplanar neighbours
    of one material and one UV island (0.5 degrees) restores large faces.
    The caller transfers UVs and normals from the source by position, so
    nothing it needs is lost here."""
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
    # UV islands are a boundary too: trim maps neighbouring quads to different
    # atlas regions, and fusing them streaked the texture across the atlas.
    bmesh.ops.dissolve_limit(bm, angle_limit=0.0087, verts=bm.verts, edges=bm.edges, delimit={"MATERIAL", "UV"})
    bmesh.ops.triangulate(bm, faces=bm.faces)
    bm.to_mesh(ob.data)
    bm.free()


def export(objects, path, merge=True):
    for ob in objects:
        if merge:
            merge_coplanar(ob)
    bpy.ops.object.select_all(action="DESELECT")
    for ob in objects:
        ob.select_set(True)
    bpy.ops.wm.obj_export(filepath=path, export_selected_objects=True, export_materials=True,
                          export_uv=False, export_normals=True, export_triangulated_mesh=True,
                          apply_modifiers=True, forward_axis="Y", up_axis="Z")


def main():
    spec = json.load(open(sys.argv[sys.argv.index("--") + 1]))
    pieces = load(spec["input"])
    if spec.get("close_shell", True):
        for ob in pieces:
            close_shell(ob)
        debris_sources = pieces
    else:
        # The standing piece stays open (closing it would seal openings such
        # as an arch passage); debris is cut from a closed copy, so what
        # breaks off is solid. The two agree wherever the cells reach.
        debris_sources = []
        for ob in pieces:
            closed_copy = duplicate(ob, ob.name + "_closed")
            close_shell(closed_copy)
            debris_sources.append(closed_copy)
    squash = spec["squash"]
    sites = [Vector(p[i] * squash[i] for i in range(3)) for p in spec["seeds"]]
    lo, hi = spec["bounds"]
    bounds = ([lo[i] * squash[i] for i in range(3)], [hi[i] * squash[i] for i in range(3)])
    cut = bpy.data.materials.get("CUT") or bpy.data.materials.new("CUT")
    debris = []
    cutter = None
    for k in spec["removed"]:
        cell = cell_object(k, sites, bounds, squash, cut, spec["neighbor_radius"])
        if not cell.data.polygons:
            bpy.data.objects.remove(cell)
            continue
        # Debris: the stone inside this exact cell. (An open shell can give an
        # open piece; the caller keeps only closed ones.)
        # Every material's shell: a gate's merlons are trim, not wall stone.
        for ob in debris_sources:
            if ob.data.polygons:
                part = duplicate(ob, f"cell_{k}__{ob.name.removesuffix('_closed')}")
                apply_boolean(part, cell, "INTERSECT")
                if part.data.polygons:
                    debris.append(part)
                else:
                    bpy.data.objects.remove(part)
        # The cutter: removed cells grown by ~3 mm and united. Neighbouring
        # cells compute their shared face separately and disagree by float
        # error; subtracting them one by one left zero-thickness fins on
        # every seam. Grown and united, they leave none.
        centre = sum((v.co for v in cell.data.vertices), Vector()) / len(cell.data.vertices)
        for v in cell.data.vertices:
            offset = v.co - centre
            v.co = centre + offset * (1.0 + 0.003 / max(offset.length, 1e-6))
        if cutter is None:
            cutter = cell
        else:
            apply_boolean(cutter, cell, "UNION")
            bpy.data.objects.remove(cell)
    if cutter is not None:
        for ob in pieces:
            if ob.data.polygons:
                apply_boolean(ob, cutter, "DIFFERENCE")
        bpy.data.objects.remove(cutter)
    merge = spec.get("merge_coplanar", True)
    export(pieces, spec["output"], merge)
    if debris:
        export(debris, spec["debris"], merge)
    else:
        open(spec["debris"], "w").close()
    print("RUIN_DAMAGE_OK", len(pieces), "pieces", len(debris), "debris")


main()
