#!/usr/bin/env python3
"""Fetch the scanned CC0 texture files a terrain layer-set manifest pins.

Luxel is source only (see .gitignore): it does not carry art. A terrain layer
set is therefore committed as a MANIFEST (source URL, byte size, sha256,
licence, physical size) and the files land in the ignored artifacts tree. The
Rust loader re-verifies every digest before a byte reaches a packet, so this
tool is a convenience, not an authority.

Usage:
    python3 tools/fetch_terrain_layers.py tools/terrain_layers/converge0.json
    python3 tools/fetch_terrain_layers.py MANIFEST --pin   # (re)write sizes + sha256

`--pin` downloads and records the digests. Use it only when deliberately
adding or changing a source; a pinned manifest must never be silently re-pinned.
"""

import argparse
import hashlib
import json
import os
import sys
import urllib.request

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def cache_path(manifest, layer, map_name):
    entry = layer["maps"][map_name]
    return os.path.join(REPO, manifest["cache_dir"], layer["layer_id"], os.path.basename(entry["url"]))


def sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for block in iter(lambda: handle.read(1 << 20), b""):
            digest.update(block)
    return "sha256:" + digest.hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("manifest")
    parser.add_argument("--pin", action="store_true")
    args = parser.parse_args()
    with open(args.manifest) as handle:
        manifest = json.load(handle)

    failures = []
    for layer in manifest["layers"]:
        for map_name, entry in layer["maps"].items():
            path = cache_path(manifest, layer, map_name)
            os.makedirs(os.path.dirname(path), exist_ok=True)
            if not os.path.exists(path) or (not args.pin and sha256_of(path) != entry.get("sha256")):
                print("fetch", entry["url"])
                urllib.request.urlretrieve(entry["url"], path)
            size = os.path.getsize(path)
            digest = sha256_of(path)
            if args.pin:
                entry["bytes"] = size
                entry["sha256"] = digest
            elif size != entry["bytes"] or digest != entry["sha256"]:
                failures.append(f"{layer['layer_id']}/{map_name}: {size} bytes {digest}, "
                                f"manifest pins {entry['bytes']} bytes {entry['sha256']}")
            print(f"  {layer['layer_id']:8s} {map_name:10s} {size:9d} {digest[:23]}")

    if args.pin:
        with open(args.manifest, "w") as handle:
            json.dump(manifest, handle, indent=2)
            handle.write("\n")
        print("pinned", args.manifest)
    if failures:
        print("DIGEST MISMATCH — refusing:", *failures, sep="\n  ")
        sys.exit(1)


if __name__ == "__main__":
    main()
