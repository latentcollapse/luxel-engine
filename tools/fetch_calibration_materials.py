#!/usr/bin/env python3
"""Fetch and size the scanned CC0 material sets of the CALIBRATION-1 scene.

WGE is source only (see .gitignore), so the material sets are committed as a
MANIFEST (source URL, byte size, sha256, provider md5, licence, physical size)
and the files land in the ignored artifacts tree, exactly like the N-4 terrain
layers (tools/fetch_terrain_layers.py). No file enters the scene without a
manifest entry: tools/build_calibration_glb.py reads only what this tool wrote,
and refuses a set whose source digests do not match the manifest.

Sizing (docs/world/converge/converge1-contracts.md §3 budget): albedo and normal at 512 px,
roughness / AO / metal at 256 px, from the 1K sources by EXACT box filters
(2x2 and 4x4), in numpy, so the output is deterministic on any machine:
  * albedo is averaged in linear light, then re-encoded sRGB;
  * normals are averaged as vectors and renormalised;
  * roughness / AO / metal are averaged as data.

Usage:
    python3 tools/fetch_calibration_materials.py tools/calibration_materials/calibration1.json
    python3 tools/fetch_calibration_materials.py MANIFEST --pin   # (re)write sizes + digests

`--pin` queries the Poly Haven API, downloads, cross-checks the provider's md5,
and records byte size and sha256. Use it only when deliberately adding or
changing a source; a pinned manifest must never be silently re-pinned.
"""

import argparse
import hashlib
import io
import json
import os
import sys
import urllib.request

import numpy as np
from PIL import Image

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
API = "https://api.polyhaven.com"
# Manifest map name -> Poly Haven API key.
API_KEYS = {"albedo": "Diffuse", "normal_gl": "nor_gl", "roughness": "Rough", "ao": "AO", "metal": "Metal"}


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return "sha256:" + digest.hexdigest()


def md5_of(path):
    digest = hashlib.md5()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def api_json(path):
    request = urllib.request.Request(f"{API}/{path}", headers={"User-Agent": "wge-calibration-fetch"})
    with urllib.request.urlopen(request) as response:
        return json.load(response)


def source_path(manifest, material, map_name):
    url = material["maps"][map_name]["url"]
    return os.path.join(REPO, manifest["cache_dir"], "source", material["source_asset"], os.path.basename(url))


def sized_path(manifest, material, map_name):
    px = manifest["texture_sizes_px"][map_name]
    return os.path.join(REPO, manifest["cache_dir"], "sized", material["source_asset"], f"{map_name}_{px}.png")


def srgb_to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def linear_to_srgb(c):
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(np.clip(c, 0, None), 1 / 2.4) - 0.055)


def box(array, factor):
    h, w = array.shape[:2]
    return array.reshape(h // factor, factor, w // factor, factor, -1).mean(axis=(1, 3))


def downsample(path, map_name, size):
    image = Image.open(path)
    rgb = np.asarray(image.convert("RGB"), dtype=np.float64) / 255.0
    h, w = rgb.shape[:2]
    if h != w or h % size:
        raise SystemExit(f"{path}: {w}x{h} does not box-reduce to {size}")
    factor = h // size
    if map_name == "albedo":
        out = linear_to_srgb(box(srgb_to_linear(rgb), factor))
    elif map_name == "normal_gl":
        v = box(rgb * 2.0 - 1.0, factor)
        v /= np.maximum(np.linalg.norm(v, axis=-1, keepdims=True), 1e-8)
        out = v * 0.5 + 0.5
    else:
        out = box(rgb[..., :1], factor).repeat(3, axis=-1)
    return np.clip(np.rint(out * 255.0), 0, 255).astype(np.uint8)


def pin(manifest):
    for material in manifest["materials"]:
        asset = material["source_asset"]
        info = api_json(f"info/{asset}")
        files = api_json(f"files/{asset}")
        material["authors"] = sorted(info["authors"])
        material["dimensions_mm"] = [round(value) for value in info["dimensions"]]
        for map_name in material["maps"]:
            entry = files[API_KEYS[map_name]]["1k"]["jpg"]
            material["maps"][map_name] = {"url": entry["url"], "provider_md5": entry["md5"]}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("manifest")
    parser.add_argument("--pin", action="store_true")
    args = parser.parse_args()
    with open(args.manifest) as handle:
        manifest = json.load(handle)
    if args.pin:
        pin(manifest)

    failures = []
    for material in manifest["materials"]:
        for map_name, entry in material["maps"].items():
            path = source_path(manifest, material, map_name)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            if not os.path.exists(path) or (not args.pin and sha256_of(path) != entry.get("sha256")):
                print("fetch", entry["url"])
                request = urllib.request.Request(entry["url"], headers={"User-Agent": "wge-calibration-fetch"})
                with urllib.request.urlopen(request) as response, open(path, "wb") as out:
                    out.write(response.read())
            size, digest, md5 = os.path.getsize(path), sha256_of(path), md5_of(path)
            if md5 != entry["provider_md5"]:
                failures.append(f"{material['source_asset']}/{map_name}: md5 {md5} != provider {entry['provider_md5']}")
            if args.pin:
                entry["bytes"], entry["sha256"] = size, digest
            elif size != entry["bytes"] or digest != entry["sha256"]:
                failures.append(f"{material['source_asset']}/{map_name}: {size} bytes {digest}, "
                                f"manifest pins {entry['bytes']} bytes {entry['sha256']}")
                continue
            out_path = sized_path(manifest, material, map_name)
            os.makedirs(os.path.dirname(out_path), exist_ok=True)
            pixels = downsample(path, map_name, manifest["texture_sizes_px"][map_name])
            buffer = io.BytesIO()
            Image.fromarray(pixels, "RGB").save(buffer, format="PNG", optimize=False, compress_level=6)
            with open(out_path, "wb") as out:
                out.write(buffer.getvalue())
            print(f"  {material['source_asset']:22s} {map_name:10s} {size:9d} {digest[:23]} -> {pixels.shape[1]}px")

    if failures:
        print("DIGEST MISMATCH — refusing:", *failures, sep="\n  ")
        sys.exit(1)
    if args.pin:
        with open(args.manifest, "w") as handle:
            json.dump(manifest, handle, indent=2)
            handle.write("\n")
        print("pinned", args.manifest)


if __name__ == "__main__":
    main()
