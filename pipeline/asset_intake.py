#!/usr/bin/env python3
"""Deterministic structural intake for model-authored GLB assets.

This module is deliberately narrower than a renderer and more authoritative
than a filename or thumbnail.  It reads a GLB container, decodes the geometry
and material contracts that are actually present, and emits a report that
separates observed facts from claims the rigging/content system must still
construct or verify.

The report omits absolute paths, process data, timestamps, and renderer output.
Its canonical digest is therefore stable when the same bytes are inspected from
different working directories or by different processes.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import math
import struct
import sys
from pathlib import Path
from typing import Any, Iterable

import numpy as np
from PIL import Image


SCHEMA_VERSION = "wge.asset-intake/v1"

_COMPONENT_TYPES: dict[int, tuple[str, np.dtype[Any], int]] = {
    5120: ("BYTE", np.dtype("<i1"), 1),
    5121: ("UNSIGNED_BYTE", np.dtype("<u1"), 1),
    5122: ("SHORT", np.dtype("<i2"), 2),
    5123: ("UNSIGNED_SHORT", np.dtype("<u2"), 2),
    5125: ("UNSIGNED_INT", np.dtype("<u4"), 4),
    5126: ("FLOAT", np.dtype("<f4"), 4),
}
_TYPE_COMPONENTS = {
    "SCALAR": 1,
    "VEC2": 2,
    "VEC3": 3,
    "VEC4": 4,
    "MAT2": 4,
    "MAT3": 9,
    "MAT4": 16,
}
_PRIMITIVE_MODES = {
    0: "POINTS",
    1: "LINES",
    2: "LINE_LOOP",
    3: "LINE_STRIP",
    4: "TRIANGLES",
    5: "TRIANGLE_STRIP",
    6: "TRIANGLE_FAN",
}


class AssetIntakeError(ValueError):
    """The GLB cannot be safely interpreted by the intake contract."""


def _canonical_bytes(value: Any) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=True,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    ).encode("utf-8")


def canonical_sha256(value: Any) -> str:
    """Hash a JSON value using the report's canonical encoding."""

    return hashlib.sha256(_canonical_bytes(value)).hexdigest()


def _rounded(value: float) -> float:
    number = float(value)
    if abs(number) < 5e-12:
        return 0.0
    return float(round(number, 9))


def _vector(values: Iterable[float]) -> list[float]:
    return [_rounded(value) for value in values]


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _read_glb(path: Path) -> tuple[dict[str, Any], bytes, dict[str, Any]]:
    """Read and validate the GLB container, returning JSON and BIN data."""

    raw = path.read_bytes()
    if len(raw) < 20:
        raise AssetIntakeError("GLB is shorter than its mandatory header and JSON chunk")
    magic, version, declared_length = struct.unpack_from("<4sII", raw, 0)
    if magic != b"glTF":
        raise AssetIntakeError("input is not a glTF binary container")
    if version != 2:
        raise AssetIntakeError("unsupported glTF version %d; expected 2" % version)
    if declared_length != len(raw):
        raise AssetIntakeError(
            "GLB declared length %d does not match file length %d"
            % (declared_length, len(raw))
        )

    offset = 12
    json_bytes: bytes | None = None
    binary_chunks: list[bytes] = []
    extra_chunks: list[dict[str, Any]] = []
    chunk_index = 0
    while offset < len(raw):
        if offset + 8 > len(raw):
            raise AssetIntakeError("GLB ends inside a chunk header")
        chunk_length, chunk_type = struct.unpack_from("<II", raw, offset)
        start = offset + 8
        end = start + chunk_length
        if end > len(raw):
            raise AssetIntakeError("GLB chunk %d extends beyond the file" % chunk_index)
        chunk = raw[start:end]
        if chunk_type == 0x4E4F534A:  # ASCII JSON in little-endian uint32 form.
            if json_bytes is not None:
                raise AssetIntakeError("GLB contains more than one JSON chunk")
            json_bytes = chunk.rstrip(b"\\x00 \\t\\r\\n")
        elif chunk_type == 0x004E4942:  # BIN\\0.
            binary_chunks.append(chunk)
        else:
            label = chunk_type.to_bytes(4, "little").decode("ascii", errors="replace")
            extra_chunks.append({"type": label, "byte_length": chunk_length})
        offset = end
        chunk_index += 1

    if json_bytes is None:
        raise AssetIntakeError("GLB contains no JSON chunk")
    try:
        document = json.loads(json_bytes.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise AssetIntakeError("GLB JSON chunk is not valid UTF-8 JSON: %s" % exc) from exc
    if not isinstance(document, dict):
        raise AssetIntakeError("GLB JSON root must be an object")
    if len(binary_chunks) > 1:
        raise AssetIntakeError("multiple BIN chunks are not supported by glTF 2.0")

    binary = binary_chunks[0] if binary_chunks else b""
    for index, buffer in enumerate(document.get("buffers", [])):
        if not isinstance(buffer, dict):
            raise AssetIntakeError("buffer %d is not an object" % index)
        declared = int(buffer.get("byteLength", -1))
        if index == 0 and declared > len(binary):
            raise AssetIntakeError(
                "buffer 0 declares %d bytes but the BIN chunk has %d"
                % (declared, len(binary))
            )
        if buffer.get("uri") and index == 0:
            raise AssetIntakeError("external buffer URIs are not accepted in a GLB intake")

    container = {
        "magic": magic.decode("ascii"),
        "version": version,
        "declared_length": declared_length,
        "json_byte_length": len(json_bytes),
        "bin_byte_length": len(binary),
        "extra_chunks": extra_chunks,
    }
    return document, binary, container


def _accessor(
    document: dict[str, Any],
    binary: bytes,
    accessor_index: int,
) -> tuple[dict[str, Any], np.ndarray]:
    accessors = document.get("accessors", [])
    if not isinstance(accessors, list) or accessor_index < 0 or accessor_index >= len(accessors):
        raise AssetIntakeError("accessor %d is out of range" % accessor_index)
    accessor = accessors[accessor_index]
    if not isinstance(accessor, dict):
        raise AssetIntakeError("accessor %d is not an object" % accessor_index)
    if "sparse" in accessor:
        raise AssetIntakeError(
            "accessor %d uses sparse data, which the intake decoder does not silently approximate"
            % accessor_index
        )
    if "bufferView" not in accessor:
        raise AssetIntakeError("accessor %d has no bufferView" % accessor_index)
    views = document.get("bufferViews", [])
    view_index = int(accessor["bufferView"])
    if view_index < 0 or view_index >= len(views):
        raise AssetIntakeError("accessor %d references missing bufferView %d" % (accessor_index, view_index))
    view = views[view_index]
    if not isinstance(view, dict):
        raise AssetIntakeError("bufferView %d is not an object" % view_index)
    if int(view.get("buffer", 0)) != 0:
        raise AssetIntakeError("bufferView %d references a non-GLB buffer" % view_index)

    component_type = int(accessor.get("componentType", 0))
    component_name, dtype, component_size = _COMPONENT_TYPES.get(
        component_type, ("", np.dtype("<u1"), 0)
    )
    if not component_size:
        raise AssetIntakeError(
            "accessor %d uses unsupported componentType %d"
            % (accessor_index, component_type)
        )
    value_type = accessor.get("type")
    component_count = _TYPE_COMPONENTS.get(value_type)
    if component_count is None:
        raise AssetIntakeError("accessor %d uses unsupported type %r" % (accessor_index, value_type))

    count = int(accessor.get("count", -1))
    if count < 0:
        raise AssetIntakeError("accessor %d has an invalid count" % accessor_index)
    item_size = component_size * component_count
    stride = int(view.get("byteStride", item_size))
    if stride < item_size:
        raise AssetIntakeError("bufferView %d has a stride smaller than its accessor item" % view_index)
    byte_offset = int(view.get("byteOffset", 0)) + int(accessor.get("byteOffset", 0))
    required = byte_offset + (max(count - 1, 0) * stride) + item_size
    if required > len(binary):
        raise AssetIntakeError(
            "accessor %d reads through byte %d but the BIN chunk has %d bytes"
            % (accessor_index, required, len(binary))
        )

    if count == 0:
        values = np.empty((0, component_count), dtype=dtype)
    elif stride == item_size:
        values = np.frombuffer(
            binary,
            dtype=dtype,
            count=count * component_count,
            offset=byte_offset,
        ).reshape((count, component_count))
    else:
        values = np.ndarray(
            shape=(count, component_count),
            dtype=dtype,
            buffer=binary,
            offset=byte_offset,
            strides=(stride, component_size),
        )
    values = np.array(values, copy=True)

    if accessor.get("normalized") and component_type != 5126:
        if component_type in {5121, 5123, 5125}:
            values = values.astype(np.float64) / float(np.iinfo(dtype).max)
        else:
            info = np.iinfo(dtype)
            values = np.maximum(values.astype(np.float64) / max(abs(info.min), info.max), -1.0)

    metadata = {
        "index": accessor_index,
        "buffer_view": view_index,
        "count": count,
        "type": value_type,
        "component_type": component_name,
        "component_type_code": component_type,
        "component_count": component_count,
        "normalized": bool(accessor.get("normalized", False)),
        "byte_stride": stride,
        "byte_offset": byte_offset,
    }
    return metadata, values


def _accessor_summary(metadata: dict[str, Any], values: np.ndarray) -> dict[str, Any]:
    result = dict(metadata)
    if values.size:
        result["min"] = _vector(np.min(values, axis=0))
        result["max"] = _vector(np.max(values, axis=0))
    else:
        result["min"] = []
        result["max"] = []
    return result


def _matrix_for_node(node: dict[str, Any]) -> np.ndarray:
    if "matrix" in node:
        values = np.asarray(node["matrix"], dtype=np.float64)
        if values.size != 16:
            raise AssetIntakeError("node matrix must contain 16 values")
        return values.reshape((4, 4), order="F")

    translation = np.asarray(node.get("translation", [0.0, 0.0, 0.0]), dtype=np.float64)
    rotation = np.asarray(node.get("rotation", [0.0, 0.0, 0.0, 1.0]), dtype=np.float64)
    scale = np.asarray(node.get("scale", [1.0, 1.0, 1.0]), dtype=np.float64)
    if translation.size != 3 or rotation.size != 4 or scale.size != 3:
        raise AssetIntakeError("node TRS has an invalid component count")
    qx, qy, qz, qw = rotation
    norm = math.sqrt(float(np.dot(rotation, rotation)))
    if norm == 0.0:
        raise AssetIntakeError("node rotation quaternion has zero length")
    qx, qy, qz, qw = (rotation / norm).tolist()
    rotation_matrix = np.array(
        [
            [1 - 2 * (qy * qy + qz * qz), 2 * (qx * qy - qz * qw), 2 * (qx * qz + qy * qw), 0],
            [2 * (qx * qy + qz * qw), 1 - 2 * (qx * qx + qz * qz), 2 * (qy * qz - qx * qw), 0],
            [2 * (qx * qz - qy * qw), 2 * (qy * qz + qx * qw), 1 - 2 * (qx * qx + qy * qy), 0],
            [0, 0, 0, 1],
        ],
        dtype=np.float64,
    )
    transform = np.eye(4, dtype=np.float64)
    transform[:3, :3] = rotation_matrix[:3, :3] @ np.diag(scale)
    transform[:3, 3] = translation
    return transform


def _transform_bounds(bounds: tuple[np.ndarray, np.ndarray], matrix: np.ndarray) -> dict[str, list[float]]:
    minimum, maximum = bounds
    corners = np.array(
        [
            [x, y, z, 1.0]
            for x in (minimum[0], maximum[0])
            for y in (minimum[1], maximum[1])
            for z in (minimum[2], maximum[2])
        ],
        dtype=np.float64,
    )
    transformed = (matrix @ corners.T).T[:, :3]
    return {"min": _vector(np.min(transformed, axis=0)), "max": _vector(np.max(transformed, axis=0))}


def _topology_report(
    positions: np.ndarray,
    indices: np.ndarray,
    normals: np.ndarray | None,
) -> dict[str, Any]:
    if len(indices) % 3:
        raise AssetIntakeError("triangle primitive index count is not divisible by three")
    triangles = indices.reshape((-1, 3)).astype(np.int64, copy=False)
    if len(triangles) == 0:
        return {
            "triangle_count": 0,
            "degenerate_index_triangles": 0,
            "zero_area_triangles": 0,
            "edge_count": 0,
            "boundary_edge_count": 0,
            "manifold_edge_count": 0,
            "non_manifold_edge_count": 0,
            "max_edge_uses": 0,
            "connected_component_count": 0,
            "largest_component_vertices": 0,
            "component_vertex_counts_top": [],
        }
    if int(triangles.min()) < 0 or int(triangles.max()) >= len(positions):
        raise AssetIntakeError("triangle index falls outside the position accessor")

    v0, v1, v2 = positions[triangles[:, 0]], positions[triangles[:, 1]], positions[triangles[:, 2]]
    cross = np.cross(v1 - v0, v2 - v0)
    area = np.linalg.norm(cross, axis=1) * 0.5
    repeated = (
        (triangles[:, 0] == triangles[:, 1])
        | (triangles[:, 1] == triangles[:, 2])
        | (triangles[:, 0] == triangles[:, 2])
    )

    edges = np.concatenate(
        (triangles[:, [0, 1]], triangles[:, [1, 2]], triangles[:, [2, 0]]),
        axis=0,
    )
    edges.sort(axis=1)
    unique_edges, edge_counts = np.unique(edges, axis=0, return_counts=True)

    parent = np.arange(len(positions), dtype=np.int64)
    rank = np.zeros(len(positions), dtype=np.int8)

    def find(value: int) -> int:
        while parent[value] != value:
            parent[value] = parent[parent[value]]
            value = int(parent[value])
        return value

    def union(left: int, right: int) -> None:
        root_left, root_right = find(left), find(right)
        if root_left == root_right:
            return
        if rank[root_left] < rank[root_right]:
            root_left, root_right = root_right, root_left
        parent[root_right] = root_left
        if rank[root_left] == rank[root_right]:
            rank[root_left] += 1

    for left, right in unique_edges.tolist():
        union(int(left), int(right))
    roots = np.asarray([find(index) for index in range(len(positions))], dtype=np.int64)
    component_roots, component_sizes = np.unique(roots, return_counts=True)
    used = np.unique(triangles)
    isolated_vertex_count = int(len(positions) - len(used))

    result: dict[str, Any] = {
        "triangle_count": int(len(triangles)),
        "degenerate_index_triangles": int(np.count_nonzero(repeated)),
        "zero_area_triangles": int(np.count_nonzero(area <= 1e-12)),
        "triangle_area_m2": {
            "min": _rounded(float(np.min(area))),
            "max": _rounded(float(np.max(area))),
            "mean": _rounded(float(np.mean(area))),
        },
        "edge_count": int(len(unique_edges)),
        "boundary_edge_count": int(np.count_nonzero(edge_counts == 1)),
        "manifold_edge_count": int(np.count_nonzero(edge_counts == 2)),
        "non_manifold_edge_count": int(np.count_nonzero(edge_counts > 2)),
        "max_edge_uses": int(np.max(edge_counts)),
        "isolated_vertex_count": isolated_vertex_count,
        "connected_component_count": int(len(component_roots)),
        "largest_component_vertices": int(np.max(component_sizes)),
        "component_vertex_counts_top": [
            int(value) for value in sorted(component_sizes.tolist(), reverse=True)[:10]
        ],
    }

    if normals is not None and len(normals) == len(positions):
        valid = area > 1e-12
        face_normals = np.zeros_like(cross, dtype=np.float64)
        face_normals[valid] = cross[valid] / area[valid, None] / 2.0
        averaged = normals[triangles[:, 0]] + normals[triangles[:, 1]] + normals[triangles[:, 2]]
        averaged_length = np.linalg.norm(averaged, axis=1)
        valid &= averaged_length > 1e-12
        averaged[valid] /= averaged_length[valid, None]
        dots = np.sum(face_normals[valid] * averaged[valid], axis=1)
        result["normal_orientation_disagreement_count"] = int(np.count_nonzero(dots < 0.0))
    return result


def _image_report(document: dict[str, Any], binary: bytes, index: int) -> dict[str, Any]:
    images = document.get("images", [])
    image = images[index]
    if not isinstance(image, dict):
        raise AssetIntakeError("image %d is not an object" % index)
    entry: dict[str, Any] = {
        "index": index,
        "mime_type": image.get("mimeType"),
        "embedded": False,
    }
    if "bufferView" in image:
        views = document.get("bufferViews", [])
        view_index = int(image["bufferView"])
        if view_index < 0 or view_index >= len(views):
            raise AssetIntakeError("image %d references missing bufferView %d" % (index, view_index))
        view = views[view_index]
        start = int(view.get("byteOffset", 0))
        end = start + int(view["byteLength"])
        raw = binary[start:end]
        entry.update(
            {
                "embedded": True,
                "buffer_view": view_index,
                "encoded_byte_length": len(raw),
                "encoded_sha256": _sha256_bytes(raw),
            }
        )
        try:
            with Image.open(io.BytesIO(raw)) as decoded:
                rgba = decoded.convert("RGBA")
                alpha = np.asarray(rgba.getchannel("A"), dtype=np.uint8)
                entry.update(
                    {
                        "decoded_format": decoded.format,
                        "width_px": int(decoded.width),
                        "height_px": int(decoded.height),
                        "mode": decoded.mode,
                        "alpha_non_opaque_pixel_count": int(np.count_nonzero(alpha < 255)),
                        "alpha_zero_pixel_count": int(np.count_nonzero(alpha == 0)),
                    }
                )
                if alpha.size:
                    entry["alpha_non_opaque_fraction"] = _rounded(
                        float(np.count_nonzero(alpha < 255) / alpha.size)
                    )
        except (OSError, ValueError) as exc:
            entry["decode_error"] = str(exc)
    elif "uri" in image:
        entry["uri"] = image["uri"]
    else:
        raise AssetIntakeError("image %d has neither bufferView nor uri" % index)
    return entry


def _texture_report(document: dict[str, Any], index: int) -> dict[str, Any]:
    texture = document.get("textures", [])[index]
    if not isinstance(texture, dict):
        raise AssetIntakeError("texture %d is not an object" % index)
    entry = {
        "index": index,
        "source": texture.get("source"),
        "sampler": texture.get("sampler"),
        "extensions": texture.get("extensions", {}),
    }
    if "extensions" in texture and "EXT_texture_webp" in texture["extensions"]:
        entry["source"] = texture["extensions"]["EXT_texture_webp"].get("source")
    return entry


def _material_report(
    document: dict[str, Any],
    image_reports: list[dict[str, Any]],
    texture_reports: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    result = []
    for index, material in enumerate(document.get("materials", [])):
        if not isinstance(material, dict):
            raise AssetIntakeError("material %d is not an object" % index)
        pbr = material.get("pbrMetallicRoughness", {})
        if not isinstance(pbr, dict):
            raise AssetIntakeError("material %d has invalid PBR data" % index)
        bindings: dict[str, Any] = {}
        for semantic, key in (
            ("base_color", "baseColorTexture"),
            ("metallic_roughness", "metallicRoughnessTexture"),
            ("normal", "normalTexture"),
            ("occlusion", "occlusionTexture"),
            ("emissive", "emissiveTexture"),
        ):
            source = pbr.get(key) if key in pbr else material.get(key)
            if isinstance(source, dict) and "index" in source:
                texture_index = int(source["index"])
                texture = texture_reports[texture_index]
                image_index = texture.get("source")
                bindings[semantic] = {
                    "texture_index": texture_index,
                    "image_index": image_index,
                    "image": image_reports[image_index] if isinstance(image_index, int) else None,
                    "texcoord": int(source.get("texCoord", 0)),
                }
        result.append(
            {
                "index": index,
                "name": material.get("name"),
                "alpha_mode": material.get("alphaMode", "OPAQUE"),
                "double_sided": bool(material.get("doubleSided", False)),
                "pbr_factors": {
                    "base_color_factor": pbr.get("baseColorFactor", [1, 1, 1, 1]),
                    "metallic_factor": pbr.get("metallicFactor", 1),
                    "roughness_factor": pbr.get("roughnessFactor", 1),
                },
                "bindings": bindings,
                "extensions": material.get("extensions", {}),
            }
        )
    return result


def _node_report(document: dict[str, Any], index: int, matrix: np.ndarray) -> dict[str, Any]:
    node = document["nodes"][index]
    return {
        "index": index,
        "name": node.get("name"),
        "mesh": node.get("mesh"),
        "skin": node.get("skin"),
        "children": [int(child) for child in node.get("children", [])],
        "local_matrix": [_rounded(value) for value in matrix.reshape(-1).tolist()],
        "has_explicit_matrix": "matrix" in node,
        "has_translation": "translation" in node,
        "has_rotation": "rotation" in node,
        "has_scale": "scale" in node,
    }


def _active_node_indices(document: dict[str, Any]) -> list[int]:
    nodes = document.get("nodes", [])
    if not nodes:
        return []
    scenes = document.get("scenes", [])
    scene_index = int(document.get("scene", 0)) if scenes else None
    roots = scenes[scene_index].get("nodes", []) if scene_index is not None else list(range(len(nodes)))
    seen: set[int] = set()
    ordered: list[int] = []

    def visit(index: int) -> None:
        if index in seen:
            return
        if index < 0 or index >= len(nodes):
            raise AssetIntakeError("scene graph references missing node %d" % index)
        seen.add(index)
        ordered.append(index)
        for child in nodes[index].get("children", []):
            visit(int(child))

    for root in roots:
        visit(int(root))
    return ordered


def _primitive_report(
    document: dict[str, Any],
    binary: bytes,
    mesh_index: int,
    primitive_index: int,
    primitive: dict[str, Any],
) -> tuple[dict[str, Any], dict[str, Any]]:
    attributes = primitive.get("attributes", {})
    if not isinstance(attributes, dict) or "POSITION" not in attributes:
        raise AssetIntakeError("mesh %d primitive %d has no POSITION attribute" % (mesh_index, primitive_index))
    position_meta, positions = _accessor(document, binary, int(attributes["POSITION"]))
    if positions.shape[1] != 3:
        raise AssetIntakeError("POSITION accessor must be VEC3")
    normal_meta = None
    normals = None
    if "NORMAL" in attributes:
        normal_meta, normals = _accessor(document, binary, int(attributes["NORMAL"]))
    uv_meta = None
    uvs = None
    if "TEXCOORD_0" in attributes:
        uv_meta, uvs = _accessor(document, binary, int(attributes["TEXCOORD_0"]))

    index_meta = None
    indices = None
    mode = int(primitive.get("mode", 4))
    if "indices" in primitive:
        index_meta, indices = _accessor(document, binary, int(primitive["indices"]))
        if indices.shape[1] != 1:
            raise AssetIntakeError("indices accessor must be SCALAR")
        indices = indices[:, 0]
    else:
        indices = np.arange(len(positions), dtype=np.uint32)

    attribute_reports = {}
    for semantic, accessor_index in sorted(attributes.items()):
        metadata, values = _accessor(document, binary, int(accessor_index))
        attribute_reports[semantic] = _accessor_summary(metadata, values)

    report: dict[str, Any] = {
        "index": primitive_index,
        "mode": _PRIMITIVE_MODES.get(mode, "UNKNOWN_%d" % mode),
        "mode_code": mode,
        "material": primitive.get("material"),
        "attributes": attribute_reports,
        "missing_common_attributes": [
            semantic
            for semantic in ("TANGENT", "COLOR_0", "JOINTS_0", "WEIGHTS_0")
            if semantic not in attributes
        ],
        "morph_target_count": len(primitive.get("targets", [])),
        "vertex_count": int(len(positions)),
        "index_count": int(len(indices)),
        "position_bounds": {
            "min": _vector(np.min(positions, axis=0)),
            "max": _vector(np.max(positions, axis=0)),
            "size": _vector(np.max(positions, axis=0) - np.min(positions, axis=0)),
        },
    }
    if mode == 4:
        topology = _topology_report(positions, indices, normals)
        report["topology"] = topology
    else:
        report["topology"] = {
            "triangle_count": 0,
            "not_evaluated_for_mode": report["mode"],
        }

    if normals is not None:
        normal_lengths = np.linalg.norm(normals, axis=1)
        report["normal_quality"] = {
            "min_length": _rounded(float(np.min(normal_lengths))),
            "max_length": _rounded(float(np.max(normal_lengths))),
            "non_unit_count": int(np.count_nonzero(np.abs(normal_lengths - 1.0) > 1e-3)),
        }
    if uvs is not None:
        report["uv0_quality"] = {
            "min": _vector(np.min(uvs, axis=0)),
            "max": _vector(np.max(uvs, axis=0)),
            "outside_0_1_count": int(np.count_nonzero((uvs < 0.0).any(axis=1) | (uvs > 1.0).any(axis=1))),
        }
    decoded = {
        "positions": positions,
        "normals": normals,
        "uvs": uvs,
        "indices": indices,
        "position_meta": position_meta,
        "normal_meta": normal_meta,
        "uv_meta": uv_meta,
        "index_meta": index_meta,
    }
    return report, decoded


def _rigging_assessment(
    document: dict[str, Any],
    primitive_reports: list[dict[str, Any]],
    materials: list[dict[str, Any]],
) -> dict[str, Any]:
    skins = document.get("skins", [])
    animations = document.get("animations", [])
    morph_targets = sum(int(report.get("morph_target_count", 0)) for report in primitive_reports)
    has_joints = any("JOINTS_0" not in report["missing_common_attributes"] for report in primitive_reports)
    has_weights = any("WEIGHTS_0" not in report["missing_common_attributes"] for report in primitive_reports)
    has_tangents = all("TANGENT" not in report["missing_common_attributes"] for report in primitive_reports)
    boundary_edges = sum(
        int(report.get("topology", {}).get("boundary_edge_count", 0))
        for report in primitive_reports
    )
    components = sum(
        int(report.get("topology", {}).get("connected_component_count", 0))
        for report in primitive_reports
    )
    zero_area_triangles = sum(
        int(report.get("topology", {}).get("zero_area_triangles", 0))
        for report in primitive_reports
    )
    non_manifold_edges = sum(
        int(report.get("topology", {}).get("non_manifold_edge_count", 0))
        for report in primitive_reports
    )
    normal_disagreements = sum(
        int(report.get("topology", {}).get("normal_orientation_disagreement_count", 0))
        for report in primitive_reports
    )

    pre_rigging_repairs: list[dict[str, Any]] = []
    if zero_area_triangles:
        pre_rigging_repairs.append(
            {
                "id": "GEO-001",
                "action": "Remove or repair zero-area triangles before binding",
                "observed_count": zero_area_triangles,
                "status": "required",
            }
        )
    if non_manifold_edges:
        pre_rigging_repairs.append(
            {
                "id": "GEO-002",
                "action": "Inspect and resolve non-manifold edges",
                "observed_count": non_manifold_edges,
                "status": "required",
            }
        )
    if boundary_edges:
        pre_rigging_repairs.append(
            {
                "id": "GEO-003",
                "action": "Classify open boundaries as intentional shells or repair targets",
                "observed_count": boundary_edges,
                "status": "required",
            }
        )
    if components > len(primitive_reports):
        pre_rigging_repairs.append(
            {
                "id": "GEO-004",
                "action": "Segment disconnected shells into semantic parts or document why they remain separate",
                "observed_count": components,
                "status": "required",
            }
        )
    if normal_disagreements:
        pre_rigging_repairs.append(
            {
                "id": "GEO-005",
                "action": "Validate winding and recalculate only the normal regions proven incorrect",
                "observed_count": normal_disagreements,
                "status": "required",
            }
        )
    for material in materials:
        binding = material.get("bindings", {}).get("base_color")
        image = binding.get("image") if isinstance(binding, dict) else None
        if (
            material.get("alpha_mode") == "OPAQUE"
            and isinstance(image, dict)
            and image.get("alpha_non_opaque_pixel_count", 0)
        ):
            pre_rigging_repairs.append(
                {
                    "id": "MAT-001",
                    "action": "Decide whether base-color alpha is intentional, then preserve it or discard it explicitly",
                    "observed_count": int(image["alpha_non_opaque_pixel_count"]),
                    "status": "target-dependent",
                }
            )

    required = [
        {
            "id": "RIG-001",
            "action": "Construct or author a semantic skeleton and rest pose",
            "reason": "The GLB contains no skins, joints, or joint hierarchy.",
            "status": "required",
        },
        {
            "id": "RIG-002",
            "action": "Assign and validate skin weights",
            "reason": "No JOINTS_0 or WEIGHTS_0 attributes are present.",
            "status": "required",
        },
        {
            "id": "RIG-003",
            "action": "Annotate semantic parts and rigging landmarks before binding",
            "reason": "The asset is one primitive with no semantic part labels or sockets.",
            "status": "required",
        },
        {
            "id": "RIG-004",
            "action": "Calibrate real-world scale and rest orientation",
            "reason": "The file supplies coordinates but no verified gameplay scale or authored pose.",
            "status": "required",
        },
        {
            "id": "RIG-005",
            "action": "Construct a minimum animation set and validate contacts",
            "reason": "The GLB contains no animations.",
            "status": "required",
        },
        {
            "id": "RIG-006",
            "action": "Construct collision, LOD, and gameplay socket metadata",
            "reason": "These are not represented in the GLB contract.",
            "status": "required",
        },
    ]
    if not has_tangents:
        required.append(
            {
                "id": "RIG-007",
                "action": "Generate or bake tangents for the target material pipeline",
                "reason": "No TANGENT vertex attribute is present.",
                "status": "target-dependent",
            }
        )
    if boundary_edges or components > 1:
        required.append(
            {
                "id": "RIG-008",
                "action": "Classify disconnected shells and decide whether to weld, preserve, or reject them",
                "reason": "Topology is fragmented or open; automatic skinning must not assume one continuous surface.",
                "status": "required",
            }
        )

    return {
        "status": "unrigged_static_mesh" if not skins and not animations else "partial_rigging",
        "observed": {
            "skin_count": len(skins),
            "animation_count": len(animations),
            "morph_target_count": morph_targets,
            "has_joint_attributes": has_joints,
            "has_weight_attributes": has_weights,
            "has_tangent_attributes": has_tangents,
            "boundary_edge_count": boundary_edges,
            "connected_component_count": components,
            "zero_area_triangle_count": zero_area_triangles,
            "non_manifold_edge_count": non_manifold_edges,
            "normal_orientation_disagreement_count": normal_disagreements,
        },
        "pre_rigging_repairs": pre_rigging_repairs,
        "required_repairs_or_construction": required,
    }


def inspect_asset(path: Path) -> dict[str, Any]:
    """Inspect one GLB and return a deterministic, JSON-serializable report."""

    source = path.expanduser().resolve()
    if not source.is_file():
        raise AssetIntakeError("asset does not exist: %s" % source)
    document, binary, container = _read_glb(source)

    images = [
        _image_report(document, binary, index)
        for index in range(len(document.get("images", [])))
    ]
    textures = [
        _texture_report(document, index)
        for index in range(len(document.get("textures", [])))
    ]
    materials = _material_report(document, images, textures)

    mesh_reports: list[dict[str, Any]] = []
    primitive_reports: list[dict[str, Any]] = []
    decoded_primitives: dict[tuple[int, int], dict[str, Any]] = {}
    for mesh_index, mesh in enumerate(document.get("meshes", [])):
        if not isinstance(mesh, dict):
            raise AssetIntakeError("mesh %d is not an object" % mesh_index)
        primitive_entries = []
        for primitive_index, primitive in enumerate(mesh.get("primitives", [])):
            entry, decoded = _primitive_report(
                document, binary, mesh_index, primitive_index, primitive
            )
            entry["mesh_index"] = mesh_index
            primitive_entries.append(entry)
            primitive_reports.append(entry)
            decoded_primitives[(mesh_index, primitive_index)] = decoded
        mesh_reports.append(
            {
                "index": mesh_index,
                "name": mesh.get("name"),
                "primitive_count": len(primitive_entries),
                "primitives": primitive_entries,
                "extras": mesh.get("extras", {}),
            }
        )

    node_matrices = {
        index: _matrix_for_node(node)
        for index, node in enumerate(document.get("nodes", []))
    }
    active_nodes = _active_node_indices(document)
    node_reports = [
        _node_report(document, index, node_matrices[index])
        for index in range(len(document.get("nodes", [])))
    ]
    instances: list[dict[str, Any]] = []
    for node_index in active_nodes:
        node = document["nodes"][node_index]
        mesh_index = node.get("mesh")
        if mesh_index is None:
            continue
        for primitive in mesh_reports[int(mesh_index)]["primitives"]:
            bounds = primitive["position_bounds"]
            local_bounds = (
                np.asarray(bounds["min"], dtype=np.float64),
                np.asarray(bounds["max"], dtype=np.float64),
            )
            world_bounds = _transform_bounds(local_bounds, node_matrices[node_index])
            instances.append(
                {
                    "node_index": node_index,
                    "node_name": node.get("name"),
                    "mesh_index": int(mesh_index),
                    "primitive_index": int(primitive["index"]),
                    "world_bounds": world_bounds,
                }
            )

    used_accessors = set()
    for mesh in document.get("meshes", []):
        for primitive in mesh.get("primitives", []):
            if "indices" in primitive:
                used_accessors.add(int(primitive["indices"]))
            used_accessors.update(int(index) for index in primitive.get("attributes", {}).values())
    unused_accessors = sorted(set(range(len(document.get("accessors", [])))) - used_accessors)

    extensions_used = sorted(str(value) for value in document.get("extensionsUsed", []))
    extensions_required = sorted(str(value) for value in document.get("extensionsRequired", []))
    root_scene = document.get("scene")
    input_report = {
        "file_name": source.name,
        "sha256": _sha256_bytes(source.read_bytes()),
        "byte_length": source.stat().st_size,
        "container": container,
        "asset": document.get("asset", {}),
    }
    scene_report = {
        "active_scene": root_scene,
        "scene_count": len(document.get("scenes", [])),
        "node_count": len(document.get("nodes", [])),
        "active_node_count": len(active_nodes),
        "mesh_count": len(document.get("meshes", [])),
        "camera_count": len(document.get("cameras", [])),
        "skin_count": len(document.get("skins", [])),
        "animation_count": len(document.get("animations", [])),
        "nodes": node_reports,
        "mesh_instances": instances,
    }

    primitive_attributes = {
        semantic
        for primitive in primitive_reports
        for semantic in primitive.get("attributes", {})
    }
    geometry_report = {
        "mesh_count": len(mesh_reports),
        "primitive_count": len(primitive_reports),
        "total_vertex_count": sum(int(entry["vertex_count"]) for entry in primitive_reports),
        "total_index_count": sum(int(entry["index_count"]) for entry in primitive_reports),
        "total_triangle_count": sum(
            int(entry.get("topology", {}).get("triangle_count", 0))
            for entry in primitive_reports
        ),
        "attributes_present": sorted(primitive_attributes),
        "attributes_absent_from_all_primitives": [
            semantic
            for semantic in ("TANGENT", "COLOR_0", "JOINTS_0", "WEIGHTS_0")
            if semantic not in primitive_attributes
        ],
        "unused_accessor_indices": unused_accessors,
        "meshes": mesh_reports,
    }

    report: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "input": input_report,
        "extensions": {
            "used": extensions_used,
            "required": extensions_required,
        },
        "scene": scene_report,
        "geometry": geometry_report,
        "materials": {
            "material_count": len(materials),
            "texture_count": len(textures),
            "image_count": len(images),
            "materials": materials,
            "textures": textures,
            "images": images,
        },
        "inference_boundary": {
            "safe_inferences": [
                {
                    "claim": "The file contains renderable triangle geometry.",
                    "evidence": "A TRIANGLES primitive has POSITION and indices.",
                },
                {
                    "claim": "The mesh has vertex normals and a TEXCOORD_0 UV set.",
                    "evidence": "NORMAL and TEXCOORD_0 accessors are bound to the primitive.",
                },
                {
                    "claim": "The material uses embedded PBR base-color and metallic-roughness textures.",
                    "evidence": "The material binds both textures and the image bytes are embedded in the GLB.",
                },
                {
                    "claim": "The asset is not rigged or animated in this file.",
                    "evidence": "There are no skins, no animations, and no JOINTS_0 or WEIGHTS_0 attributes.",
                },
                {
                    "claim": "The glTF coordinate convention is Y-up, but gameplay scale is not verified.",
                    "evidence": "No authored scale or unit calibration is present in the scene graph.",
                },
            ],
            "not_safe_to_infer": [
                "A semantic identity such as humanoid, creature, weapon, armor, or prop.",
                "Anatomical joints, rest pose, bone hierarchy, or animation intent.",
                "Real-world scale, gameplay affordances, collision, sockets, or LOD policy.",
                "Whether transparent pixels in the base-color image are meaningful, because alphaMode is OPAQUE.",
                "Whether disconnected shells are intentional parts or defective fragmentation.",
            ],
        },
        "rigging_assessment": _rigging_assessment(document, primitive_reports, materials),
        "benchmark_status": "intake_complete_rigging_required",
    }
    report["canonical_report_sha256"] = canonical_sha256(report)
    return report


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Produce a deterministic structural intake report for one GLB"
    )
    parser.add_argument("asset", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    try:
        report = inspect_asset(args.asset)
    except (AssetIntakeError, OSError, ValueError) as exc:
        parser.error(str(exc))
    payload = json.dumps(report, indent=2, sort_keys=True, ensure_ascii=True) + "\n"
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(payload, encoding="utf-8")
    else:
        print(payload, end="")
    print(
        "Asset intake %s: %s"
        % (report["benchmark_status"], report["canonical_report_sha256"]),
        file=sys.stderr,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
