#!/usr/bin/env python3
"""Re-render every frozen reference and prove each capture is byte-identical.

WGE's regression discipline is "absent axis = same bytes": a change that adds
an axis must leave every existing frame exactly as it was. This renders each
reference set with the current build and compares capture digests, view by
view, against the reference summaries. Any mismatch, missing view or failed
render is a failure; there is no tolerance.

References (see the memory note on reference renders):
  parity  ab-null-tfix, ab-full-tfix, ab-converge0-tfix, ab-converge1-tfix (12)
          ab-converge2-n5 (3, needs the built kit1)
          ab-converge3-final (3, needs the built kit2)
  calib   run3 (88), run3-grazing (16), n6-foliage (9)

Usage:
    python3 tools/verify_identity.py OUT_DIR [--only parity|calibration] [--bin PATH]
Exit status 0 only when every compared frame is identical.
"""

import argparse
import json
import os
import subprocess
import sys

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
JULIA = "/home/mattc/.juliaup/bin/julia"
LAYOUT = "world_core/crates/reference_runtime/examples/riverwatch.layout.json"
WORKER = "graphics_lab/bin/wge_graphics_worker.jl"

PARITY = [
    # (reference dir, policy)
    ("artifacts/parity/ab-null-tfix", ""),
    ("artifacts/parity/ab-full-tfix", "full"),
    ("artifacts/parity/ab-converge0-tfix", "converge0"),
    ("artifacts/parity/ab-converge1-tfix", "converge1"),
    ("artifacts/parity/ab-converge2-n5", "converge2"),
    ("artifacts/parity/ab-converge3-final", "converge3"),
]
CALIBRATION = [
    # (reference dir, glb, rigs, views)
    ("artifacts/calibration/run3", "artifacts/calibration/calibration1/calibration1.glb", "all", "row,close"),
    ("artifacts/calibration/run3-grazing", "artifacts/calibration/calibration1/calibration1.glb", "sun,sun-ibl", "grazing"),
    ("artifacts/calibration/n6-foliage", "artifacts/calibration/calibration2/calibration2.glb", "sun,sun-ibl,overcast", "foliage"),
]
# Variables that select arms or content; authorization reads them from the
# environment, so they are cleared before every render and set explicitly.
SELECTORS = ("WGE_PARITY_RENDER_POLICY", "WGE_TERRAIN_LAYER_SET", "WGE_PARITY_CONTENT", "WGE_KIT_SET", "WGE_BACKDROP_SET")


def clean_env(**extra):
    env = {k: v for k, v in os.environ.items() if k not in SELECTORS}
    env.update(extra)
    return env


def parity_digests(path):
    summary = json.load(open(os.path.join(path, "campaign2_summary.json")))
    return {view["view"]: view["capture_sha256"] for view in summary["views"]}


def calibration_digests(path):
    summary = json.load(open(os.path.join(path, "calibration_summary.json")))
    return {(r["rig"], r["view"]): r["capture_sha256"] for r in summary["renders"]}


def compare(label, reference, candidate):
    rows, failures = [], 0
    for key in sorted(reference, key=str):
        got = candidate.get(key)
        ok = got == reference[key]
        failures += not ok
        rows.append((key, "identical" if ok else ("MISSING" if got is None else "DIFFERS")))
    extra = sorted(set(candidate) - set(reference), key=str)
    for key in extra:
        rows.append((key, "EXTRA (not in reference)"))
        failures += 1
    print(f"{label}: {len(reference) - failures if failures <= len(reference) else 0}/{len(reference)} identical")
    for key, status in rows:
        if status != "identical":
            print(f"    {key}: {status}")
    return failures


def run(command, env, log):
    with open(log, "w") as handle:
        return subprocess.run(command, cwd=REPO, env=env, stdout=handle, stderr=subprocess.STDOUT).returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("out")
    parser.add_argument("--only", choices=("parity", "calibration"))
    parser.add_argument("--bin", default="world_core/target/release/wge-native-graphics-contract")
    args = parser.parse_args()
    os.makedirs(os.path.join(REPO, args.out), exist_ok=True)
    failures, frames = 0, 0
    if args.only in (None, "parity"):
        for reference, policy in PARITY:
            name = os.path.basename(reference)
            out = os.path.join(args.out, name)
            extra = {"WGE_PARITY_RENDER_POLICY": policy} if policy else {}
            if policy in ("converge0", "converge1", "converge2", "converge3"):
                extra["WGE_TERRAIN_LAYER_SET"] = "tools/terrain_layers/converge0.json"
            if policy == "converge2":
                extra["WGE_KIT_SET"] = "tools/kit/kit1.lock.json"
            if policy == "converge3":
                extra["WGE_KIT_SET"] = "tools/kit/kit2.lock.json"
            command = [args.bin, "render-campaign2-layout", LAYOUT, JULIA, "terrain_lab", "graphics_lab", WORKER, out]
            code = run(command, clean_env(**extra), os.path.join(REPO, out + ".log"))
            reference_digests = parity_digests(os.path.join(REPO, reference))
            frames += len(reference_digests)
            if code != 0:
                print(f"{name}: render FAILED (exit {code}); see {out}.log")
                failures += len(reference_digests)
                continue
            failures += compare(name, reference_digests, parity_digests(os.path.join(REPO, out)))
    if args.only in (None, "calibration"):
        for reference, glb, rigs, views in CALIBRATION:
            name = os.path.basename(reference)
            out = os.path.join(args.out, name)
            command = [args.bin, "render-calibration", LAYOUT, JULIA, "terrain_lab", "graphics_lab", WORKER, glb, out,
                       "--rigs", rigs, "--views", views]
            code = run(command, clean_env(), os.path.join(REPO, out + ".log"))
            reference_digests = calibration_digests(os.path.join(REPO, reference))
            frames += len(reference_digests)
            if code != 0:
                print(f"{name}: render FAILED (exit {code}); see {out}.log")
                failures += len(reference_digests)
                continue
            failures += compare(name, reference_digests, calibration_digests(os.path.join(REPO, out)))
    print(f"TOTAL: {frames - failures}/{frames} frames identical")
    sys.exit(0 if failures == 0 and frames > 0 else 1)


if __name__ == "__main__":
    main()
