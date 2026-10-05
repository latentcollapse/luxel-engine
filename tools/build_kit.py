#!/usr/bin/env python3
"""Condition the CONVERGE-2 N-5 hero kit into one GLB per asset (WGE_CONVERGE2_CONTRACTS.md §2).

Input: the pinned Poly Haven sources of tools/models/kit1.json (fetched with
tools/fetch_models.py; every file is re-verified here).

Output: `artifacts/kit/kit1/<asset>.glb` and `kit1.build.json` (what each
asset became: triangles, textures, decimation, leaf-card report). The GLB
digests are pinned in the committed `tools/kit/kit1.lock.json`; a build whose
digests differ from the lock is refused unless run with `--pin`.

Assets:
  * ruin    — three `modular_fort_01` pieces composed as a broken gateway
              (authored low-poly; no decimation). Node transforms are baked:
              render conditioning ignores them.
  * rock_a / rock_b — one rock from each mossy rock set, decimated in Blender.
  * tree    — `tree_small_02`: trunk and branches decimated in Blender; the
              1.94 M leaf triangles rebuilt as cluster cards (tools/leaf_cards.py)
              in one RGBA atlas, MASK at 0.5.
  * fern    — `fern_02_b` as authored (ground cover).

Textures are box-reduced exactly (albedo in linear light, normals renormalised,
ARM as data) to the per-asset sizes below, chosen against the measured frame
budget (kit <= 24 MB of packet). JPEG base colours with a separate alpha map
are merged into RGBA; BLEND becomes MASK at 0.5.

Usage:
    python3 tools/build_kit.py            # build and verify against the lock
    python3 tools/build_kit.py --pin      # build and (re)write the lock
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from build_calibration_glb import Gltf, Mesh, box_reduce, linear_to_srgb, png_bytes, rgba_png_bytes, srgb_to_linear  # noqa: E402
from fetch_models import file_path  # noqa: E402
from gltf_model import node_primitives, verified  # noqa: E402
import leaf_cards  # noqa: E402

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MODELS_MANIFEST = "tools/models/kit1.json"
LOCK = "tools/kit/kit1.lock.json"
OUT_DIR = "artifacts/kit/kit1"

# Ruin layout: (fort node, yaw degrees, (x, z) of the footprint centre in metres).
# A gateway with the wall broken off beside it: the wall end stands 2 m clear of
# the gate and a few degrees off true, so it reads as what is left of a wall,
# not as an intact block (the first layout, with a corner piece, did).
RUIN_PIECES = (
    ("modular_fort_01_wall_thin_gate_01", 0.0, (0.0, 0.0)),
    ("modular_fort_01_wall_thick_end_01", 7.0, (0.3, 8.3)),
)
ROCKS = (("rock_a", "rock_moss_set_01", "rock_moss_set_01_rock02"),
         ("rock_b", "rock_moss_set_02", "rock_moss_set_02_rock11"))
ROCK_TRIANGLES = 2500
TRUNK_TRIANGLES = 3000
BRANCH_TRIANGLES = 6000
LEAF_TILE_PX, LEAF_MAX_CLUSTERS, LEAF_ATLAS_PX = 24, 220, 512
# (albedo, normal, arm) edge lengths in px, per source material name suffix.
TEXTURE_SIZES = {
    "wall_1": (512, 256, 256), "trim_1": (256, 128, 128), "plaster_1": (256, 128, 128),
    "rock_moss_set_01": (256, 256, 128), "rock_moss_set_02": (256, 256, 128),
    "tree_small_02_trunk": (256, 256, 128), "tree_small_02_branches": (256, 128, 128),
    "fern_02": (256, 128, 128),
}


def sha256_bytes(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def blender_version():
    out = subprocess.run(["blender", "--version"], capture_output=True, text=True, check=True).stdout
    return out.splitlines()[0].strip()


def write_obj(path, positions, normals, uvs, indices):
    with open(path, "w") as handle:
        for p in positions:
            handle.write(f"v {p[0]:.6f} {p[1]:.6f} {p[2]:.6f}\n")
        for t in uvs:
            handle.write(f"vt {t[0]:.6f} {1.0 - t[1]:.6f}\n")
        for n in normals:
            handle.write(f"vn {n[0]:.6f} {n[1]:.6f} {n[2]:.6f}\n")
        for a, b, c in indices.reshape(-1, 3) + 1:
            handle.write(f"f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}\n")


def read_obj(path):
    v, vt, vn, rows = [], [], [], []
    with open(path) as handle:
        for line in handle:
            parts = line.split()
            if not parts:
                continue
            if parts[0] == "v":
                v.append([float(x) for x in parts[1:4]])
            elif parts[0] == "vt":
                vt.append([float(parts[1]), 1.0 - float(parts[2])])
            elif parts[0] == "vn":
                vn.append([float(x) for x in parts[1:4]])
            elif parts[0] == "f":
                for corner in parts[1:4]:
                    a, b, n = corner.split("/")
                    rows.append((int(a) - 1, int(b) - 1, int(n) - 1))
    rows = np.asarray(rows)
    return np.asarray(v)[rows[:, 0]], np.asarray(vn)[rows[:, 2]], np.asarray(vt)[rows[:, 1]], np.arange(len(rows))


def decimate(primitive, triangles, workdir, name):
    """Blender collapse to at most `triangles`; returns a primitive dict."""
    source, target = os.path.join(workdir, f"{name}.obj"), os.path.join(workdir, f"{name}.dec.obj")
    write_obj(source, primitive["positions"], primitive["normals"], primitive["uvs"], primitive["indices"])
    subprocess.run(["blender", "-b", "--factory-startup", "--python",
                    os.path.join(REPO, "tools/blender_decimate.py"), "--", source, target, str(triangles)],
                   check=True, capture_output=True)
    positions, normals, uvs, indices = read_obj(target)
    # Collapse can leave zero-area triangles whose exported normals are zero
    # (seen on the tree branches). Drop them, and give any other zero normal
    # its face normal, so no NaN reaches the packet.
    tri = positions[indices.reshape(-1, 3)]
    face = np.cross(tri[:, 1] - tri[:, 0], tri[:, 2] - tri[:, 0])
    area = np.linalg.norm(face, axis=1)
    keep = area > 1e-12
    corners = indices.reshape(-1, 3)[keep].reshape(-1)
    face = np.repeat(face[keep] / area[keep, None], 3, axis=0)
    positions, normals, uvs = positions[corners], normals[corners], uvs[corners]
    zero = np.linalg.norm(normals, axis=1) < 1e-6
    normals[zero] = face[zero]
    indices = np.arange(len(corners))
    return {**primitive, "positions": positions, "normals": normals, "uvs": uvs, "indices": indices}


def mesh_of(primitive):
    mesh = Mesh()
    mesh.p, mesh.n, mesh.uv, mesh.i = (list(primitive["positions"]), list(primitive["normals"]),
                                       list(primitive["uvs"]), list(primitive["indices"]))
    return mesh


class Materials:
    """Conditioned glTF materials of one source model, one per source material."""

    def __init__(self, gltf, manifest, model, document):
        self.gltf, self.manifest, self.model, self.document = gltf, manifest, model, document
        self.cache = {}
        self.texture_px = {}

    def _image(self, info):
        uri = self.document["images"][self.document["textures"][info["index"]]["source"]]["uri"]
        path = file_path(self.manifest, self.model, uri)
        return np.asarray(Image.open(path).convert("RGB"), np.float64) / 255.0

    def _texture(self, key, data, px):
        self.texture_px[key] = px
        return {"index": self.gltf.texture(key, data)}

    def get(self, index, alpha_map=None):
        if index in self.cache:
            return self.cache[index]
        source = self.document["materials"][index]
        name = source["name"]
        suffix = next(s for s in sorted(TEXTURE_SIZES, key=len, reverse=True) if name.endswith(s))
        albedo_px, normal_px, arm_px = TEXTURE_SIZES[suffix]
        pbr = source["pbrMetallicRoughness"]
        albedo = linear_to_srgb(box_reduce(srgb_to_linear(self._image(pbr["baseColorTexture"])), albedo_px))
        mode = source.get("alphaMode", "OPAQUE")
        spec = {"name": name, "doubleSided": bool(source.get("doubleSided", False))}
        if mode in ("MASK", "BLEND"):
            if alpha_map is None:
                raise SystemExit(f"{name}: {mode} material needs an alpha map")
            alpha = np.asarray(Image.open(file_path(self.manifest, self.model, alpha_map)).convert("L"), np.float64) / 255.0
            rgba = np.concatenate([albedo, box_reduce(alpha[..., None], albedo_px)], axis=-1)
            base = self._texture(f"{name}_albedo", rgba_png_bytes(rgba), albedo_px)
            spec["alphaMode"] = "MASK"
            spec["alphaCutoff"] = source.get("alphaCutoff", 0.5) if mode == "MASK" else 0.5
        else:
            base = self._texture(f"{name}_albedo", png_bytes(albedo), albedo_px)
        normal = box_reduce(self._image(source["normalTexture"]) * 2.0 - 1.0, normal_px)
        normal = normal / np.maximum(np.linalg.norm(normal, axis=-1, keepdims=True), 1e-8) * 0.5 + 0.5
        arm = box_reduce(self._image(pbr["metallicRoughnessTexture"]), arm_px)
        spec["pbrMetallicRoughness"] = {
            "baseColorTexture": base,
            "metallicRoughnessTexture": self._texture(f"{name}_arm", png_bytes(arm), arm_px),
            "metallicFactor": pbr.get("metallicFactor", 1.0),
            "roughnessFactor": pbr.get("roughnessFactor", 1.0),
        }
        spec["normalTexture"] = self._texture(f"{name}_normal", png_bytes(normal), normal_px)
        self.cache[index] = self.gltf.material(spec)
        return self.cache[index]


def footprint_placed(primitives, yaw_degrees, center_xz):
    """Recentre a piece on its own footprint (base on y = 0), then yaw and move it."""
    allp = np.concatenate([p["positions"] for p in primitives])
    lo, hi = allp.min(axis=0), allp.max(axis=0)
    shift = np.array([(lo[0] + hi[0]) / 2, lo[1], (lo[2] + hi[2]) / 2])
    yaw = np.radians(yaw_degrees)
    rotation = np.array([[np.cos(yaw), 0, np.sin(yaw)], [0, 1, 0], [-np.sin(yaw), 0, np.cos(yaw)]])
    offset = np.array([center_xz[0], 0.0, center_xz[1]])
    return [{**p, "positions": (p["positions"] - shift) @ rotation.T + offset, "normals": p["normals"] @ rotation.T}
            for p in primitives]


def grounded(primitives):
    """Footprint centred on the origin, base on y = 0."""
    return footprint_placed(primitives, 0.0, (0.0, 0.0))


def triangles(primitives):
    return int(sum(len(p["indices"]) // 3 for p in primitives))


def build_ruin(manifest, models):
    model = models["modular_fort_01"]
    document, buffers = verified(manifest, model)
    gltf = Gltf()
    materials = Materials(gltf, manifest, model, document)
    pieces = []
    for node, yaw, center in RUIN_PIECES:
        placed = footprint_placed(node_primitives(document, buffers, node), yaw, center)
        for k, primitive in enumerate(placed):
            gltf.mesh(f"ruin_{node.removeprefix('modular_fort_01_')}_{k}", mesh_of(primitive), materials.get(primitive["material"]))
        pieces.append({"node": node, "yaw_deg": yaw, "center_xz_m": list(center), "triangles": triangles(placed)})
    return gltf, {"pieces": pieces, "textures_px": materials.texture_px}


def build_rock(manifest, models, model_name, node, workdir, name):
    model = models[model_name]
    document, buffers = verified(manifest, model)
    gltf = Gltf()
    materials = Materials(gltf, manifest, model, document)
    (primitive,) = grounded(node_primitives(document, buffers, node))
    source_triangles = triangles([primitive])
    primitive = decimate(primitive, ROCK_TRIANGLES, workdir, name)
    gltf.mesh(name, mesh_of(primitive), materials.get(primitive["material"]))
    return gltf, {"node": node, "source_triangles": source_triangles, "triangles": triangles([primitive]),
                  "textures_px": materials.texture_px}


def build_tree(manifest, models, workdir):
    model = models["tree_small_02"]
    document, buffers = verified(manifest, model)
    gltf = Gltf()
    materials = Materials(gltf, manifest, model, document)
    by_name = {document["materials"][p["material"]]["name"]: p for p in
               grounded(node_primitives(document, buffers, document["nodes"][0]["name"]))}
    report = {}
    for part, budget in (("trunk", TRUNK_TRIANGLES), ("branches", BRANCH_TRIANGLES)):
        source = by_name[f"tree_small_02_{part}"]
        decimated = decimate(source, budget, workdir, f"tree_{part}")
        gltf.mesh(f"tree_{part}", mesh_of(decimated), materials.get(source["material"]))
        report[part] = {"source_triangles": triangles([source]), "triangles": triangles([decimated])}
    leaves = by_name["tree_small_02_leaves"]
    albedo = srgb_to_linear(np.asarray(Image.open(file_path(manifest, model, "textures/tree_small_02_leaves_diff_1k.jpg")).convert("RGB"), np.float64) / 255.0)
    alpha = np.asarray(Image.open(file_path(manifest, model, "maps/leaves_alpha.png")).convert("L"), np.float64) / 255.0
    lo, hi = leaves["positions"].min(axis=0), leaves["positions"].max(axis=0)
    center, radius = (lo + hi) / 2, float(np.linalg.norm(hi - lo) / 2)
    cp, cn, cu, ci, atlas, cards = leaf_cards.bake(leaves["positions"], leaves["uvs"], leaves["indices"], albedo, alpha,
                                                   center, radius, max_clusters=LEAF_MAX_CLUSTERS, tile=LEAF_TILE_PX,
                                                   atlas_size=LEAF_ATLAS_PX)
    rgba = np.concatenate([linear_to_srgb(atlas[..., :3]), atlas[..., 3:]], axis=-1)
    leaf_material = gltf.material({
        "name": "tree_small_02_leaf_cards",
        "alphaMode": "MASK", "alphaCutoff": leaf_cards.ALPHA_CUTOFF, "doubleSided": True,
        "pbrMetallicRoughness": {"baseColorTexture": {"index": gltf.texture("tree_leaf_card_atlas", rgba_png_bytes(rgba))},
                                 "metallicFactor": 0.0, "roughnessFactor": 0.7},
    })
    gltf.mesh("tree_leaf_cards", mesh_of({"positions": cp, "normals": cn, "uvs": cu, "indices": ci}), leaf_material)
    # Canopy fidelity (contract §2, restated): outline and mass at 10 cm.
    leaf_xyz = leaves["positions"][leaves["indices"].reshape(-1, 3)]
    leaf_uv = leaves["uvs"][leaves["indices"].reshape(-1, 3)]
    card_xyz, card_uv = cp[ci.reshape(-1, 3)], cu[ci.reshape(-1, 3)]
    half = float(np.max(hi - lo) / 2 * 1.05)
    views = {}
    for view, normal in (("front", (0, 0, 1)), ("side", (1, 0, 0)), ("diagonal", (0.7, 0.3, 0.65))):
        views[view] = canopy_fidelity(leaf_xyz, leaf_uv, alpha, card_xyz, card_uv, atlas[..., 3], center, half, np.array(normal, float))
    cards["fidelity"] = views
    report["leaf_cards"] = cards
    report["textures_px"] = {**materials.texture_px, "tree_leaf_card_atlas": LEAF_ATLAS_PX}
    return gltf, report


def canopy_fidelity(leaf_xyz, leaf_uv, leaf_alpha, card_xyz, card_uv, card_alpha, center, half, normal, resolution=250):
    u, v, n = leaf_cards._frame(normal)
    original, _, _ = leaf_cards.splat(leaf_xyz, leaf_uv, center, u, v, n, half, resolution, leaf_alpha)
    cards, _, _ = leaf_cards.splat(card_xyz, card_uv, center, u, v, n, half, resolution, card_alpha)
    k = max(1, round(0.10 / (2 * half / resolution)))
    m = resolution // k * k
    oc = original[:m, :m].reshape(m // k, k, m // k, k).mean(axis=(1, 3))
    cc = cards[:m, :m].reshape(m // k, k, m // k, k).mean(axis=(1, 3))
    oo, co = oc > 0.15, cc > 0.15
    return {
        "outline_iou_10cm": round(float((oo & co).sum() / max((oo | co).sum(), 1)), 4),
        "mass_ratio": round(float(cc.sum() / max(oc.sum(), 1e-9)), 4),
        "pixel_exact_coverage": round(float((original & cards).sum() / max(original.sum(), 1)), 4),
    }


def build_fern(manifest, models):
    model = models["fern_02"]
    document, buffers = verified(manifest, model)
    gltf = Gltf()
    materials = Materials(gltf, manifest, model, document)
    (primitive,) = grounded(node_primitives(document, buffers, "fern_02_b"))
    gltf.mesh("fern", mesh_of(primitive), materials.get(primitive["material"], alpha_map="maps/alpha.png"))
    return gltf, {"node": "fern_02_b", "triangles": triangles([primitive]), "textures_px": materials.texture_px}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--pin", action="store_true")
    args = parser.parse_args()
    manifest = json.load(open(os.path.join(REPO, MODELS_MANIFEST)))
    models = {model["asset"]: model for model in manifest["models"]}
    out_dir = os.path.join(REPO, OUT_DIR)
    os.makedirs(out_dir, exist_ok=True)
    build = {"schema_version": "wge.kit-build/v1", "set_id": "kit1", "models_manifest": MODELS_MANIFEST,
             "blender": blender_version(), "assets": {}}
    with tempfile.TemporaryDirectory() as workdir:
        builders = {
            "ruin": lambda: build_ruin(manifest, models),
            "rock_a": lambda: build_rock(manifest, models, ROCKS[0][1], ROCKS[0][2], workdir, "rock_a"),
            "rock_b": lambda: build_rock(manifest, models, ROCKS[1][1], ROCKS[1][2], workdir, "rock_b"),
            "tree": lambda: build_tree(manifest, models, workdir),
            "fern": lambda: build_fern(manifest, models),
        }
        for name, builder in builders.items():
            gltf, report = builder()
            data = gltf.bytes()
            with open(os.path.join(out_dir, f"{name}.glb"), "wb") as handle:
                handle.write(data)
            report.update({"glb": f"{OUT_DIR}/{name}.glb", "glb_bytes": len(data), "glb_sha256": sha256_bytes(data)})
            build["assets"][name] = report
            print(f"{name:7s} {len(data) / 1e6:5.2f} MB {report['glb_sha256'][:23]}")
    with open(os.path.join(out_dir, "kit1.build.json"), "w") as handle:
        json.dump(build, handle, indent=2)
        handle.write("\n")

    lock_path = os.path.join(REPO, LOCK)
    lock = {"schema_version": "wge.kit-lock/v1", "set_id": "kit1", "blender": build["blender"],
            "assets": {name: {"glb": a["glb"], "bytes": a["glb_bytes"], "sha256": a["glb_sha256"]}
                       for name, a in build["assets"].items()}}
    if args.pin:
        os.makedirs(os.path.dirname(lock_path), exist_ok=True)
        with open(lock_path, "w") as handle:
            json.dump(lock, handle, indent=2)
            handle.write("\n")
        print("pinned", LOCK)
    else:
        pinned = json.load(open(lock_path))
        if pinned != lock:
            raise SystemExit(f"kit build does not match {LOCK} (different Blender or inputs?); "
                             "inspect, then rebuild with --pin if the change is intended")
        print("matches", LOCK)


if __name__ == "__main__":
    main()
