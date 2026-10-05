"""Read one node of a pinned glTF model as baked triangle arrays.

Shared by the calibration builder (N-6 foliage row) and the kit builder (N-5).
Render conditioning ignores node transforms, so a builder bakes them here:
positions get the node's translation, rotation and scale; normals get the
rotation and the inverse scale, renormalised. Only flat (root) nodes are
supported; a child node is refused rather than placed wrong.
"""

import json
import os

import numpy as np

from fetch_models import file_path, sha256_of

COMPONENTS = {5126: np.float32, 5125: np.uint32, 5123: np.uint16, 5121: np.uint8}
WIDTHS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}


def verified(manifest, model):
    """Refuse a model whose files do not match the manifest; return its glTF document and buffers."""
    for relative, entry in model["files"].items():
        if sha256_of(file_path(manifest, model, relative)) != entry["sha256"]:
            raise SystemExit(f"{model['asset']}/{relative} does not match the manifest; run tools/fetch_models.py")
    document_path = file_path(manifest, model, f"{model['asset']}.gltf")
    with open(document_path) as handle:
        document = json.load(handle)
    buffers = []
    for buffer in document["buffers"]:
        with open(os.path.join(os.path.dirname(document_path), buffer["uri"]), "rb") as handle:
            buffers.append(handle.read())
    return document, buffers


def accessor(document, buffers, index):
    entry = document["accessors"][index]
    view = document["bufferViews"][entry["bufferView"]]
    dtype = np.dtype(COMPONENTS[entry["componentType"]])
    width = WIDTHS[entry["type"]]
    stride = view.get("byteStride", dtype.itemsize * width)
    start = view.get("byteOffset", 0) + entry.get("byteOffset", 0)
    raw = np.frombuffer(buffers[view["buffer"]], dtype=np.uint8, count=stride * (entry["count"] - 1) + dtype.itemsize * width, offset=start)
    rows = np.lib.stride_tricks.as_strided(raw, shape=(entry["count"], dtype.itemsize * width), strides=(stride, 1))
    return np.ascontiguousarray(rows).view(dtype).reshape(entry["count"], width)


def rotation_matrix(xyzw):
    x, y, z, w = xyzw
    return np.array([
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ])


def node_triangles(document, buffers, node_name):
    """Baked (positions, normals, uv0, indices, material index) of one root node's mesh."""
    nodes = [node for node in document["nodes"] if node.get("name") == node_name]
    if len(nodes) != 1:
        raise SystemExit(f"node {node_name!r} matches {len(nodes)} nodes")
    node = nodes[0]
    roots = {index for scene in document.get("scenes", []) for index in scene["nodes"]}
    if document["nodes"].index(node) not in roots or "matrix" in node:
        raise SystemExit(f"node {node_name!r} is not a flat TRS root node")
    rotation = rotation_matrix(node.get("rotation", [0, 0, 0, 1]))
    scale = np.asarray(node.get("scale", [1, 1, 1]), np.float64)
    translation = np.asarray(node.get("translation", [0, 0, 0]), np.float64)
    primitives = document["meshes"][node["mesh"]]["primitives"]
    if len(primitives) != 1 or primitives[0].get("mode", 4) != 4:
        raise SystemExit(f"node {node_name!r} must be one triangle-list primitive")
    primitive = primitives[0]
    attributes = primitive["attributes"]
    positions = accessor(document, buffers, attributes["POSITION"]).astype(np.float64)
    normals = accessor(document, buffers, attributes["NORMAL"]).astype(np.float64)
    uvs = accessor(document, buffers, attributes["TEXCOORD_0"]).astype(np.float64)
    indices = accessor(document, buffers, primitive["indices"]).reshape(-1).astype(np.int64)
    positions = (positions * scale) @ rotation.T + translation
    normals = (normals / scale) @ rotation.T
    normals /= np.linalg.norm(normals, axis=1, keepdims=True)
    return positions, normals, uvs, indices, primitive["material"]
