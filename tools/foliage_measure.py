#!/usr/bin/env python3
"""N-6 acceptance (WGE_CONVERGE2_CONTRACTS.md §1): foliage silhouette vs distance.

Input: two `render-calibration` runs of the foliage views under the overcast
rig (no sun, so nothing casts a shadow): one of `calibration2` (with the
foliage specimens) and one of `calibration1` (the same scene without them).
The views share one fixed field of view and one ray, so the pixels that differ
between the two runs are exactly the specimens' silhouette, and its area
should scale as 1/d².

Raw scaling, normalised to the close view:
    ratio(d) = area(d) · d² / (area(d0) · d0²)
is reported but is NOT the acceptance number: a pixel only partly covered by
a leaf after the supersample resolve still differs by more than the threshold,
so the far views, whose silhouettes are mostly edge pixels, read LARGER than
1/d² even with perfect coverage (measured 2026-10-04: 1.09 at 10 m, 1.27 at
40 m).

Acceptance compares like with like. Box-filter the close mask by k = d / d0
(the views share a ray and the frame centre is the foliage centre, so k x k
blocks from the origin stay centred) and count blocks whose mean coverage
exceeds the threshold's coverage equivalent: that is the far silhouette a
renderer that preserves coverage would produce. Alpha-tested foliage that
thins at distance shows as actual / predicted < 1. Acceptance: every
actual / predicted within ±15%.

Also writes a contact sheet (with and without foliage, per distance, plus the
silhouette mask) for the human review.

Usage:
    python3 tools/foliage_measure.py RUN_WITH RUN_WITHOUT [--rig overcast] [--out DIR]
"""

import argparse
import json
import os
import re

import numpy as np
from PIL import Image

DIFF_THRESHOLD = 3  # sRGB8 levels on any channel; the two runs share every other pixel exactly
# Coverage a far pixel needs before its difference passes DIFF_THRESHOLD: the
# threshold over the median close-view difference inside the silhouette.


def read_ppm(path):
    with open(path, "rb") as handle:
        data = handle.read()
    match = re.match(rb"P6\s+(\d+)\s+(\d+)\s+255\s", data)
    width, height = int(match.group(1)), int(match.group(2))
    return np.frombuffer(data[match.end():], np.uint8).reshape(height, width, 3)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("with_run")
    parser.add_argument("without_run")
    parser.add_argument("--rig", default="overcast")
    parser.add_argument("--out")
    args = parser.parse_args()
    views = sorted(
        (int(name.split("-")[1].rstrip("m")), name)
        for name in os.listdir(os.path.join(args.with_run, args.rig))
        if name.startswith("foliage-")
    )
    if len(views) < 2:
        raise SystemExit("need at least two foliage views")
    rows, results, masks = [], [], []
    for distance, name in views:
        with_image = read_ppm(os.path.join(args.with_run, args.rig, name, "native_capture.ppm"))
        without_image = read_ppm(os.path.join(args.without_run, args.rig, name, "native_capture.ppm"))
        mask = (np.abs(with_image.astype(int) - without_image.astype(int)).max(axis=-1) > DIFF_THRESHOLD)
        masks.append(mask)
        if not results:
            difference = np.abs(with_image.astype(int) - without_image.astype(int)).max(axis=-1)
            block_threshold = DIFF_THRESHOLD / float(np.median(difference[mask]))
        results.append({"view": name, "distance_m": distance, "silhouette_px": int(mask.sum())})
        rows.append(np.concatenate([with_image, without_image, np.repeat(mask[..., None] * 255, 3, axis=-1).astype(np.uint8)], axis=1))
    d0, a0 = results[0]["distance_m"], results[0]["silhouette_px"]
    for result in results:
        result["ratio_to_close"] = round(result["silhouette_px"] * result["distance_m"] ** 2 / (a0 * d0 ** 2), 4)
        k = result["distance_m"] // d0
        if result["distance_m"] % d0:
            raise SystemExit(f"distance {result['distance_m']} is not a multiple of {d0}")
        h, w = masks[0].shape
        blocks = masks[0][: h // k * k, : w // k * k].reshape(h // k, k, w // k, k).mean(axis=(1, 3))
        predicted = int((blocks > block_threshold).sum())
        result["predicted_px"] = predicted
        result["actual_over_predicted"] = round(result["silhouette_px"] / predicted, 4)
    verdict = all(abs(result["actual_over_predicted"] - 1.0) <= 0.15 for result in results)
    report = {"rig": args.rig, "diff_threshold_srgb8": DIFF_THRESHOLD, "block_coverage_threshold": round(block_threshold, 4),
              "views": results, "within_15_percent": verdict}
    print(json.dumps(report, indent=2))
    if args.out:
        os.makedirs(args.out, exist_ok=True)
        with open(os.path.join(args.out, "foliage_silhouette.json"), "w") as handle:
            json.dump(report, handle, indent=2)
            handle.write("\n")
        sheet = np.concatenate(rows, axis=0)
        Image.fromarray(sheet).resize((sheet.shape[1] // 2, sheet.shape[0] // 2), Image.LANCZOS).save(
            os.path.join(args.out, "foliage_silhouette_sheet.png"))


if __name__ == "__main__":
    main()
