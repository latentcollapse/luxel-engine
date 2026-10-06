#!/usr/bin/env python3
"""Build the CALIBRATION-1 material scene as one glTF binary (docs/world/converge/converge1-contracts.md §3).

Input: the sized maps written by tools/fetch_calibration_materials.py. Every
source digest is re-checked against the manifest first; a set that does not
match is refused, so no texture enters the scene without a manifest entry.

Output: artifacts/calibration/calibration1/calibration1.glb (ignored tree) and
a sidecar `calibration1.layout.json` that names every mesh, its material
column and its role, which `render-calibration` uses for placement and cameras.

Geometry, local frame (metres, +Y up, the camera side is +Z):
  * a 0.18 grey ground plane (34 x 7 m, top at y = 0) and one long plinth;
  * a calibration group: 18% grey, white (0.85) and black (0.03) cards, a
    chrome ball, a grey diffuse ball, and a 1 m checker strip (5 cm checks);
  * nine material columns, 3 m apart: a 1 m UV sphere, a 1 m cube with a
    2.5 cm chamfer, and a 2 x 1 m slab leaning back 15 deg; bark adds a
    cylinder, terrain is a slab only, emissive is a sphere only.

Foliage (CONVERGE-2 N-6, only when the manifest has a `foliage` section, i.e.
the `calibration2` set): pinned Poly Haven models (tools/fetch_models.py), one
node per specimen, standing on the ground in front of the plinth at z = 3 m.
The JPEG base colour and the separate alpha map are merged into one RGBA
texture; BLEND materials are conditioned to MASK at 0.5. Without the section
the output is calibration1, byte for byte.

UVs are metric (arc length on the sphere and cylinder, planar on boxes)
divided by the scan's physical size, so one texture repeat covers exactly the
area the scan measured. Node transforms are identity: conditioning does not
apply node transforms, so positions are baked.

Material derivations (contract §3 table):
  * rough metal = metal_plate albedo/normal/AO with roughness remapped to
    0.45 + 0.55 r (isolates roughness);
  * wet stone = rock_wall_08 with albedo x 0.6 in linear light and +10%
    saturation, normals flattened to 0.4 of their slope, roughness factor 0.25.
    Normal scale is BAKED into its own map because render conditioning does
    not carry glTF normalTexture.scale (recorded as a finding in the contract).
  * glTF metallicRoughness packs roughness in G and metal in B. The renderer
    reads G only; metal_plate's metallic factor is 1.0 and its B channel is
    255, so glTF semantics and the render agree until channel semantics land.

Usage:
    python3 tools/build_calibration_glb.py tools/calibration_materials/calibration1.json
"""

import hashlib
import io
import json
import math
import os
import struct
import sys

import numpy as np
from PIL import Image

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(REPO, "tools"))
from fetch_calibration_materials import sha256_of, sized_path, source_path  # noqa: E402

PLINTH_TOP_M = 0.15
COLUMN_PITCH_M = 3.0
SLAB_LEAN_DEG = 15.0
BEVEL_M = 0.025
GROUND_HALF_X_M = 17.0
FOLIAGE_Z_M = 3.0

COLUMNS = (
    # (column id, source asset, derivation, bodies)
    ("metal", "metal_plate", "metal", ("sphere", "cube", "slab")),
    ("rough_metal", "metal_plate", "rough_metal", ("sphere", "cube", "slab")),
    ("stone", "rock_wall_08", "dielectric", ("sphere", "cube", "slab")),
    ("wet_stone", "rock_wall_08", "wet", ("sphere", "cube", "slab")),
    ("bark", "bark_willow_02", "dielectric", ("sphere", "cube", "slab", "cylinder")),
    ("wood", "wood_planks_grey", "dielectric", ("sphere", "cube", "slab")),
    ("painted", "painted_plaster_wall", "dielectric", ("sphere", "cube", "slab")),
    ("terrain", "forrest_ground_01", "dielectric", ("slab",)),
    ("emissive", None, "emissive", ("sphere",)),
)


# ---------------------------------------------------------------- geometry

class Mesh:
    def __init__(self):
        self.p, self.n, self.uv, self.i = [], [], [], []

    def tri(self, a, b, c, normal=None):
        """Append one triangle, flipping winding so it faces along the normal."""
        pa, pb, pc = (np.asarray(v[0], float) for v in (a, b, c))
        face = np.cross(pb - pa, pc - pa)
        if np.linalg.norm(face) < 1e-12:
            return
        reference = normal if normal is not None else (np.asarray(a[1]) + np.asarray(b[1]) + np.asarray(c[1]))
        if np.dot(face, reference) < 0:
            b, c = c, b
        base = len(self.p)
        for position, vertex_normal, uv in (a, b, c):
            self.p.append(position)
            self.n.append(vertex_normal)
            self.uv.append(uv)
        self.i.extend([base, base + 1, base + 2])


def planar_uv(position, normal, scale):
    axis = int(np.argmax(np.abs(normal)))
    u_axis, v_axis = {0: (2, 1), 1: (0, 2), 2: (0, 1)}[axis]
    return (position[u_axis] / scale, -position[v_axis] / scale)


def flat_polygon(mesh, points, normal, scale, transform):
    points = [transform(np.asarray(p, float)) for p in points]
    normal = rotate(normal, transform)
    verts = [(p, normal, planar_uv(p, normal, scale)) for p in points]
    for k in range(1, len(verts) - 1):
        mesh.tri(verts[0], verts[k], verts[k + 1], normal)


def rotate(vector, transform):
    origin = transform(np.zeros(3))
    out = transform(np.asarray(vector, float)) - origin
    return out / np.linalg.norm(out)


def box(mesh, half, scale, transform, bevel=0.0):
    """Axis-aligned box with optional flat chamfer on every edge and corner."""
    h = np.asarray(half, float)
    inner = h - bevel
    for axis in range(3):
        for sign in (-1, 1):
            u, v = [a for a in range(3) if a != axis]
            corners = []
            for su, sv in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
                p = np.zeros(3)
                p[axis] = sign * h[axis]
                p[u] = su * inner[u]
                p[v] = sv * inner[v]
                corners.append(p)
            normal = np.zeros(3)
            normal[axis] = sign
            flat_polygon(mesh, corners, normal, scale, transform)
    if bevel <= 0:
        return
    for a in range(3):
        for b in range(a + 1, 3):
            c = 3 - a - b
            for sa in (-1, 1):
                for sb in (-1, 1):
                    quad = []
                    for sc in (-1, 1):
                        for which in (0, 1):
                            p = np.zeros(3)
                            p[c] = sc * inner[c]
                            p[a] = sa * (h[a] if which == 0 else inner[a])
                            p[b] = sb * (inner[b] if which == 0 else h[b])
                            quad.append(p)
                    normal = np.zeros(3)
                    normal[a], normal[b] = sa, sb
                    flat_polygon(mesh, [quad[0], quad[1], quad[3], quad[2]], normal / np.linalg.norm(normal), scale, transform)
    for sx in (-1, 1):
        for sy in (-1, 1):
            for sz in (-1, 1):
                s = np.array([sx, sy, sz], float)
                points = []
                for axis in range(3):
                    p = s * inner
                    p[axis] = s[axis] * h[axis]
                    points.append(p)
                flat_polygon(mesh, points, s / np.linalg.norm(s), scale, transform)


def sphere(mesh, radius, scale, center, segments=48, rings=24):
    grid = []
    for r in range(rings + 1):
        theta = math.pi * r / rings
        row = []
        for s in range(segments + 1):
            phi = 2 * math.pi * s / segments
            n = np.array([math.sin(theta) * math.cos(phi), math.cos(theta), -math.sin(theta) * math.sin(phi)])
            row.append((center + radius * n, n, (phi * radius / scale, theta * radius / scale)))
        grid.append(row)
    for r in range(rings):
        for s in range(segments):
            a, b, c, d = grid[r][s], grid[r + 1][s], grid[r + 1][s + 1], grid[r][s + 1]
            mesh.tri(a, b, c)
            mesh.tri(a, c, d)


def cylinder(mesh, radius, height, scale, base, segments=40):
    ring = []
    for s in range(segments + 1):
        phi = 2 * math.pi * s / segments
        n = np.array([math.cos(phi), 0.0, -math.sin(phi)])
        u = phi * radius / scale
        ring.append((base + radius * n, base + radius * n + [0, height, 0], n, u))
    for s in range(segments):
        p0, q0, n0, u0 = ring[s]
        p1, q1, n1, u1 = ring[s + 1]
        a, b = (p0, n0, (u0, 0.0)), (p1, n1, (u1, 0.0))
        c, d = (q1, n1, (u1, -height / scale)), (q0, n0, (u0, -height / scale))
        mesh.tri(a, b, c)
        mesh.tri(a, c, d)
    for y, normal in ((0.0, np.array([0, -1.0, 0])), (height, np.array([0, 1.0, 0]))):
        center = base + [0, y, 0]
        cap = (center, normal, planar_uv(center, normal, scale))
        for s in range(segments):
            p0 = ring[s][0] + [0, y, 0]
            p1 = ring[s + 1][0] + [0, y, 0]
            mesh.tri(cap, (p0, normal, planar_uv(p0, normal, scale)), (p1, normal, planar_uv(p1, normal, scale)), normal)


def translate(offset):
    offset = np.asarray(offset, float)
    return lambda p: p + offset


def lean_back(offset, degrees):
    """Rotate about +X so the top leans toward -Z (away from the camera), then translate."""
    a = math.radians(degrees)
    rot = np.array([[1, 0, 0], [0, math.cos(a), math.sin(a)], [0, -math.sin(a), math.cos(a)]])
    offset = np.asarray(offset, float)
    return lambda p: rot @ p + offset


# ---------------------------------------------------------------- textures

def load_rgb(path):
    return np.asarray(Image.open(path).convert("RGB"), dtype=np.float64) / 255.0


def png_bytes(rgb01):
    buffer = io.BytesIO()
    Image.fromarray(np.clip(np.rint(rgb01 * 255.0), 0, 255).astype(np.uint8), "RGB").save(buffer, format="PNG", compress_level=6)
    return buffer.getvalue()


def srgb_to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def linear_to_srgb(c):
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(np.clip(c, 0, None), 1 / 2.4) - 0.055)


def metallic_roughness(roughness, metal):
    g = roughness[..., 0]
    return np.stack([np.ones_like(g), g, np.full_like(g, 1.0 if metal else 0.0)], axis=-1)


def flatten_normals(normal01, factor):
    v = normal01 * 2.0 - 1.0
    v[..., :2] *= factor
    v /= np.linalg.norm(v, axis=-1, keepdims=True)
    return v * 0.5 + 0.5


def rgba_png_bytes(rgba01):
    buffer = io.BytesIO()
    Image.fromarray(np.clip(np.rint(rgba01 * 255.0), 0, 255).astype(np.uint8), "RGBA").save(buffer, format="PNG", compress_level=6)
    return buffer.getvalue()


def box_reduce(array, size):
    h, w = array.shape[:2]
    if h != w or h % size:
        raise SystemExit(f"{w}x{h} does not box-reduce to {size}")
    f = h // size
    return array.reshape(size, f, size, f, -1).mean(axis=(1, 3))


def foliage_material(gltf, models_manifest, model, document, material_index):
    """The source material, conditioned: RGBA base colour (diff + alpha map) at
    512 px averaged in linear light, normals at 512 px renormalised, the ARM map
    (R = AO, G = roughness, B = metal; glTF reads G and B) at 256 px. MASK keeps
    its cutoff; BLEND becomes MASK at 0.5 (WGE does not blend)."""
    from fetch_models import file_path

    source = document["materials"][material_index]
    pbr = source["pbrMetallicRoughness"]
    asset = model["asset"]

    def image(info):
        uri = document["images"][document["textures"][info["index"]]["source"]]["uri"]
        return np.asarray(Image.open(file_path(models_manifest, model, uri)).convert("RGB"), np.float64) / 255.0

    albedo = linear_to_srgb(box_reduce(srgb_to_linear(image(pbr["baseColorTexture"])), 512))
    alpha_map = np.asarray(Image.open(file_path(models_manifest, model, "maps/alpha.png")).convert("L"), np.float64) / 255.0
    alpha = box_reduce(alpha_map[..., None], 512)
    normal = box_reduce(image(source["normalTexture"]) * 2.0 - 1.0, 512)
    normal = normal / np.maximum(np.linalg.norm(normal, axis=-1, keepdims=True), 1e-8) * 0.5 + 0.5
    arm = box_reduce(image(pbr["metallicRoughnessTexture"]), 256)
    mode = source.get("alphaMode", "OPAQUE")
    if mode not in ("MASK", "BLEND"):
        raise SystemExit(f"{asset}: foliage material is {mode}, expected MASK or BLEND")
    return gltf.material({
        "name": f"foliage_{asset}",
        "alphaMode": "MASK",
        "alphaCutoff": source.get("alphaCutoff", 0.5) if mode == "MASK" else 0.5,
        "doubleSided": True,
        "pbrMetallicRoughness": {
            "baseColorTexture": {"index": gltf.texture(f"{asset}_albedo_alpha", rgba_png_bytes(np.concatenate([albedo, alpha], axis=-1)))},
            "metallicRoughnessTexture": {"index": gltf.texture(f"{asset}_arm", png_bytes(arm))},
            "metallicFactor": pbr.get("metallicFactor", 1.0),
            "roughnessFactor": pbr.get("roughnessFactor", 1.0),
        },
        "normalTexture": {"index": gltf.texture(f"{asset}_normal", png_bytes(normal))},
    })


def wet_albedo(albedo01):
    linear = srgb_to_linear(albedo01) * 0.6
    luminance = (linear @ np.array([0.2126, 0.7152, 0.0722]))[..., None]
    return linear_to_srgb(np.clip(luminance + 1.1 * (linear - luminance), 0, 1))


# ---------------------------------------------------------------- glTF writer

class Gltf:
    def __init__(self):
        self.bin = bytearray()
        self.doc = {"asset": {"version": "2.0", "generator": "wge build_calibration_glb.py"},
                    "scene": 0, "scenes": [{"nodes": []}], "nodes": [], "meshes": [], "materials": [],
                    "textures": [], "images": [], "samplers": [{"magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497}],
                    "accessors": [], "bufferViews": [], "buffers": []}
        self.image_index = {}

    def view(self, data, target=None):
        while len(self.bin) % 4:
            self.bin.append(0)
        entry = {"buffer": 0, "byteOffset": len(self.bin), "byteLength": len(data)}
        if target:
            entry["target"] = target
        self.bin.extend(data)
        self.doc["bufferViews"].append(entry)
        return len(self.doc["bufferViews"]) - 1

    def accessor(self, array, kind, component, target, bounds=False):
        view = self.view(array.tobytes(), target)
        entry = {"bufferView": view, "componentType": component, "count": int(array.shape[0]), "type": kind}
        if bounds:
            entry["min"] = [float(v) for v in array.min(axis=0)]
            entry["max"] = [float(v) for v in array.max(axis=0)]
        self.doc["accessors"].append(entry)
        return len(self.doc["accessors"]) - 1

    def texture(self, key, png):
        if key not in self.image_index:
            self.doc["images"].append({"bufferView": self.view(png), "mimeType": "image/png", "name": key})
            self.doc["textures"].append({"sampler": 0, "source": len(self.doc["images"]) - 1, "name": key})
            self.image_index[key] = len(self.doc["textures"]) - 1
        return self.image_index[key]

    def material(self, spec):
        self.doc["materials"].append(spec)
        return len(self.doc["materials"]) - 1

    def mesh(self, name, mesh, material):
        normals = np.asarray(mesh.n, np.float64)
        normals /= np.linalg.norm(normals, axis=1, keepdims=True)
        # Weld identical (position, normal, uv) vertices. Triangles are emitted
        # with fresh vertices, so without this the packet carries ~6x the data.
        rows = np.concatenate([np.asarray(mesh.p, np.float32), normals.astype(np.float32), np.asarray(mesh.uv, np.float32)], axis=1)
        unique, inverse = np.unique(rows, axis=0, return_inverse=True)
        positions = np.ascontiguousarray(unique[:, 0:3])
        normals = np.ascontiguousarray(unique[:, 3:6])
        uvs = np.ascontiguousarray(unique[:, 6:8])
        indices = inverse.reshape(-1)[np.asarray(mesh.i)].astype(np.uint32)
        primitive = {"attributes": {
            "POSITION": self.accessor(positions, "VEC3", 5126, 34962, bounds=True),
            "NORMAL": self.accessor(normals, "VEC3", 5126, 34962),
            "TEXCOORD_0": self.accessor(uvs, "VEC2", 5126, 34962),
        }, "indices": self.accessor(indices, "SCALAR", 5125, 34963), "material": material}
        self.doc["meshes"].append({"name": name, "primitives": [primitive]})
        self.doc["nodes"].append({"name": name, "mesh": len(self.doc["meshes"]) - 1})
        self.doc["scenes"][0]["nodes"].append(len(self.doc["nodes"]) - 1)

    def bytes(self):
        while len(self.bin) % 4:
            self.bin.append(0)
        self.doc["buffers"] = [{"byteLength": len(self.bin)}]
        text = json.dumps(self.doc, separators=(",", ":"), sort_keys=True).encode()
        text += b" " * ((4 - len(text) % 4) % 4)
        body = struct.pack("<II", len(text), 0x4E4F534A) + text + struct.pack("<II", len(self.bin), 0x004E4942) + bytes(self.bin)
        return struct.pack("<III", 0x46546C67, 2, 12 + len(body)) + body


# ---------------------------------------------------------------- scene

def main():
    manifest_path = sys.argv[1] if len(sys.argv) > 1 else os.path.join(REPO, "tools/calibration_materials/calibration1.json")
    manifest = json.load(open(manifest_path))
    materials = {m["source_asset"]: m for m in manifest["materials"]}
    for material in manifest["materials"]:
        for map_name, entry in material["maps"].items():
            if sha256_of(source_path(manifest, material, map_name)) != entry["sha256"]:
                raise SystemExit(f"{material['source_asset']}/{map_name} does not match the manifest; run the fetch tool")

    def sized(asset, map_name):
        return sized_path(manifest, materials[asset], map_name)

    def physical_m(asset):
        return materials[asset]["dimensions_mm"][0] / 1000.0

    gltf = Gltf()

    def flat_material(name, rgb, metallic, roughness, emissive=None):
        spec = {"name": name, "pbrMetallicRoughness": {"baseColorFactor": [*rgb, 1.0], "metallicFactor": metallic, "roughnessFactor": roughness}}
        if emissive is not None:
            spec["emissiveFactor"] = emissive
            spec["emissiveTexture"] = {"index": gltf.texture("emissive_white", png_bytes(np.ones((4, 4, 3))))}
        return gltf.material(spec)

    def scanned_material(name, derivation, asset):
        albedo = open(sized(asset, "albedo"), "rb").read()
        normal = open(sized(asset, "normal_gl"), "rb").read()
        ao = open(sized(asset, "ao"), "rb").read()
        roughness = load_rgb(sized(asset, "roughness"))
        roughness_factor = 1.0
        if derivation == "metal":
            mr_key, mr = f"{asset}_mr_metal", metallic_roughness(roughness, True)
        elif derivation == "rough_metal":
            mr_key, mr = f"{asset}_mr_rough_metal", metallic_roughness(0.45 + 0.55 * roughness, True)
        else:
            mr_key, mr = f"{asset}_mr", metallic_roughness(roughness, False)
        albedo_key, normal_key = f"{asset}_albedo", f"{asset}_normal"
        if derivation == "wet":
            albedo_key, albedo = f"{asset}_albedo_wet", png_bytes(wet_albedo(load_rgb(sized(asset, "albedo"))))
            normal_key, normal = f"{asset}_normal_wet", png_bytes(flatten_normals(load_rgb(sized(asset, "normal_gl")), 0.4))
            roughness_factor = 0.25
        return gltf.material({
            "name": name,
            "pbrMetallicRoughness": {
                "baseColorTexture": {"index": gltf.texture(albedo_key, albedo)},
                "metallicRoughnessTexture": {"index": gltf.texture(mr_key, png_bytes(mr))},
                "metallicFactor": 1.0 if derivation in ("metal", "rough_metal") else 0.0,
                "roughnessFactor": roughness_factor,
            },
            "normalTexture": {"index": gltf.texture(normal_key, normal)},
            "occlusionTexture": {"index": gltf.texture(f"{asset}_ao", ao)},
        })

    layout = {"schema_version": "wge.calibration-layout/v1", "plinth_top_m": PLINTH_TOP_M, "columns": [], "meshes": []}

    def emit(name, mesh, material, column, role):
        gltf.mesh(name, mesh, material)
        layout["meshes"].append({"mesh_name": name, "column": column, "role": role})

    neutral = flat_material("neutral_018", (0.18, 0.18, 0.18), 0.0, 1.0)
    first_x = -((len(COLUMNS) - 1) * COLUMN_PITCH_M) / 2.0 + 2.0
    row_min_x = first_x - 1.5 - 4.0
    row_max_x = first_x + (len(COLUMNS) - 1) * COLUMN_PITCH_M + 1.5

    ground = Mesh()
    # 34 x 7 m: the row plus 1.5 m each side, from behind the slabs to past the
    # checker strip. Sized to fit a footprint of the host world that is clear
    # of every authored instance (render-calibration places it).
    box(ground, (GROUND_HALF_X_M, 0.05, 3.5), 1.0, translate(((row_min_x + row_max_x) / 2, -0.05, 0.5)))
    emit("ground_plane", ground, neutral, "scene", "ground")
    plinth = Mesh()
    box(plinth, ((row_max_x - row_min_x) / 2, PLINTH_TOP_M / 2, 1.7), 1.0, translate(((row_min_x + row_max_x) / 2, PLINTH_TOP_M / 2, 0.0)))
    emit("plinth", plinth, neutral, "scene", "plinth")

    # Calibration group.
    gx = row_min_x + 2.0
    layout["calibration_group_center_x_m"] = gx
    for name, rgb, x in (("card_black_003", 0.03, gx - 1.0), ("card_grey_018", 0.18, gx), ("card_white_085", 0.85, gx + 1.0)):
        card = Mesh()
        box(card, (0.3, 0.45, 0.02), 1.0, lambda p, x=x: lean_back((x, PLINTH_TOP_M, -0.6), SLAB_LEAN_DEG)(p + [0, 0.45, 0]))
        emit(name, card, flat_material(name, (rgb, rgb, rgb), 0.0, 1.0), "calibration", "card")
    chrome = Mesh()
    sphere(chrome, 0.25, 1.0, np.array([gx - 0.6, PLINTH_TOP_M + 0.25, 0.8]))
    emit("ball_chrome", chrome, flat_material("chrome", (0.95, 0.95, 0.95), 1.0, 0.0), "calibration", "chrome_ball")
    grey_ball = Mesh()
    sphere(grey_ball, 0.25, 1.0, np.array([gx + 0.6, PLINTH_TOP_M + 0.25, 0.8]))
    emit("ball_grey_018", grey_ball, flat_material("grey_ball_018", (0.18, 0.18, 0.18), 0.0, 1.0), "calibration", "grey_ball")
    checker = np.indices((8, 8)).sum(axis=0) // 4 % 2
    checker_png = png_bytes(np.repeat(np.where(checker[..., None] == 0, 0.05, 0.8), 3, axis=-1))
    strip = Mesh()
    # 1 m x 0.2 m on the ground in front of the plinth; one texture repeat = 0.1 m = 2 x 2 checks of 5 cm.
    box(strip, (0.5, 0.0025, 0.1), 0.1, translate((gx, 0.0025, 2.4)))
    emit("checker_strip", strip, gltf.material({"name": "checker", "pbrMetallicRoughness": {
        "baseColorTexture": {"index": gltf.texture("checker_5cm", checker_png)}, "metallicFactor": 0.0, "roughnessFactor": 1.0}}),
        "calibration", "checker")

    for k, (column, asset, derivation, bodies) in enumerate(COLUMNS):
        cx = first_x + k * COLUMN_PITCH_M
        if derivation == "emissive":
            material = flat_material("emissive", (0.05, 0.05, 0.05), 0.0, 0.6, emissive=[1.0, 0.55, 0.2])
            scale = 1.0
        else:
            material = scanned_material(column, derivation, asset)
            scale = physical_m(asset)
        layout["columns"].append({"column": column, "source_asset": asset, "center_x_m": cx,
                                  "physical_size_m": scale if asset else None})
        for body in bodies:
            mesh = Mesh()
            if body == "sphere":
                x = cx - 0.7 if len(bodies) > 1 else cx
                sphere(mesh, 0.5, scale, np.array([x, PLINTH_TOP_M + 0.5, 0.7]))
            elif body == "cube":
                box(mesh, (0.5, 0.5, 0.5), scale, translate((cx + 0.7, PLINTH_TOP_M + 0.5, 0.7)), bevel=BEVEL_M)
            elif body == "slab":
                box(mesh, (1.0, 0.5, 0.025), scale, lambda p, cx=cx: lean_back((cx, PLINTH_TOP_M, -0.6), SLAB_LEAN_DEG)(p + [0, 0.5, 0]))
            elif body == "cylinder":
                cylinder(mesh, 0.2, 1.4, scale, np.array([cx, PLINTH_TOP_M, -0.15]))
            emit(f"{column}_{body}", mesh, material, column, body)

    if "foliage" in manifest:
        from gltf_model import node_triangles, verified

        foliage = manifest["foliage"]
        models_manifest = json.load(open(os.path.join(REPO, foliage["models_manifest"])))
        models = {model["asset"]: model for model in models_manifest["models"]}
        materials_by_asset = {}
        specimens = []
        for specimen in foliage["specimens"]:
            model = models[specimen["asset"]]
            document, buffers = verified(models_manifest, model)
            positions, normals, uvs, indices, material_index = node_triangles(document, buffers, specimen["node"])
            lo, hi = positions.min(axis=0), positions.max(axis=0)
            # Footprint centred on (x, FOLIAGE_Z_M), base on the ground (y = 0).
            positions = positions - [(lo[0] + hi[0]) / 2, lo[1], (lo[2] + hi[2]) / 2] + [specimen["x_m"], 0.0, FOLIAGE_Z_M]
            if specimen["asset"] not in materials_by_asset:
                materials_by_asset[specimen["asset"]] = foliage_material(gltf, models_manifest, model, document, material_index)
            mesh = Mesh()
            mesh.p, mesh.n, mesh.uv, mesh.i = list(positions), list(normals), list(uvs), list(indices)
            emit(f"foliage_{specimen['asset']}", mesh, materials_by_asset[specimen["asset"]], "foliage", "specimen")
            specimens.append({**specimen, "size_m": [round(float(v), 3) for v in hi - lo], "triangles": len(indices) // 3})
        layout["foliage"] = {"models_manifest": foliage["models_manifest"], "z_m": FOLIAGE_Z_M, "specimens": specimens}

    data = gltf.bytes()
    out_dir = os.path.join(REPO, manifest["cache_dir"])
    glb_path = os.path.join(out_dir, f"{manifest['set_id']}.glb")
    with open(glb_path, "wb") as handle:
        handle.write(data)
    layout["glb_sha256"] = "sha256:" + hashlib.sha256(data).hexdigest()
    layout["manifest_set_id"] = manifest["set_id"]
    with open(os.path.join(out_dir, f"{manifest['set_id']}.layout.json"), "w") as handle:
        json.dump(layout, handle, indent=2)
        handle.write("\n")
    print(f"wrote {glb_path} ({len(data) / 1e6:.1f} MB) {layout['glb_sha256']}")
    print(f"  {len(gltf.doc['meshes'])} meshes, {len(gltf.doc['materials'])} materials, {len(gltf.doc['images'])} images")


if __name__ == "__main__":
    main()
