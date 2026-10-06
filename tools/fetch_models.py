#!/usr/bin/env python3
"""Fetch scanned CC0 models (glTF + buffers + textures) against a pinned manifest.

The model counterpart of tools/fetch_calibration_materials.py, for CONVERGE-2
(docs/world/converge/converge2-contracts.md): Luxel is source only, so a model set is committed
as a MANIFEST (URL, byte size, sha256, provider md5 per file, licence,
authors, physical size) and the files land in the ignored artifacts tree under
`<cache_dir>/<asset>/`, keeping the glTF's relative paths. Builders read only
what this tool wrote and refuse files whose digests do not match.

Every model entry pins the glTF document, every file it includes (buffers and
textures), and any extra maps the manifest names (`extra_maps`, e.g. `Alpha`:
Poly Haven's glTF base colour is a JPEG, so the coverage lives in a separate
map that the builder merges into an RGBA base colour).

Usage:
    python3 tools/fetch_models.py tools/models/foliage1.json
    python3 tools/fetch_models.py MANIFEST --pin   # (re)write sizes + digests

`--pin` queries the Poly Haven API, downloads, cross-checks the provider's md5,
and records byte size and sha256. Use it only when deliberately adding or
changing a source; a pinned manifest must never be silently re-pinned.
"""

import argparse
import hashlib
import json
import os
import sys
import urllib.request

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
API = "https://api.polyhaven.com"
USER_AGENT = "luxel-model-fetch"


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
    request = urllib.request.Request(f"{API}/{path}", headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request) as response:
        return json.load(response)


def model_dir(manifest, model):
    return os.path.join(REPO, manifest["cache_dir"], model["asset"])


def file_path(manifest, model, relative):
    """Local path of one pinned file. `relative` is the manifest key: the
    glTF's own relative path, `<asset>.gltf` for the document, or
    `maps/<name>.<ext>` for an extra map."""
    return os.path.join(model_dir(manifest, model), relative)


def pin(manifest):
    resolution = manifest["resolution"]
    for model in manifest["models"]:
        asset = model["asset"]
        info = api_json(f"info/{asset}")
        files = api_json(f"files/{asset}")
        model["authors"] = sorted(info["authors"])
        model["dimensions_mm"] = [round(value) for value in info["dimensions"]]
        gltf = files["gltf"][resolution]["gltf"]
        pinned = {f"{asset}.gltf": {"url": gltf["url"], "provider_md5": gltf["md5"]}}
        for relative, entry in sorted(gltf["include"].items()):
            pinned[relative] = {"url": entry["url"], "provider_md5": entry["md5"]}
        for name in model.get("extra_maps", []):
            entry = files[name][resolution]["png"]
            pinned[f"maps/{name.lower()}.png"] = {"url": entry["url"], "provider_md5": entry["md5"]}
        model["files"] = pinned


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
    for model in manifest["models"]:
        for relative, entry in model["files"].items():
            path = file_path(manifest, model, relative)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            if not os.path.exists(path) or (not args.pin and sha256_of(path) != entry.get("sha256")):
                print("fetch", entry["url"])
                request = urllib.request.Request(entry["url"], headers={"User-Agent": USER_AGENT})
                with urllib.request.urlopen(request) as response, open(path, "wb") as out:
                    out.write(response.read())
            size, digest, md5 = os.path.getsize(path), sha256_of(path), md5_of(path)
            if md5 != entry["provider_md5"]:
                failures.append(f"{model['asset']}/{relative}: md5 {md5} != provider {entry['provider_md5']}")
            if args.pin:
                entry["bytes"], entry["sha256"] = size, digest
            elif size != entry["bytes"] or digest != entry["sha256"]:
                failures.append(f"{model['asset']}/{relative}: {size} bytes {digest}, "
                                f"manifest pins {entry['bytes']} bytes {entry['sha256']}")
                continue
            print(f"  {model['asset']:18s} {relative:44s} {size:9d} {digest[:23]}")

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
