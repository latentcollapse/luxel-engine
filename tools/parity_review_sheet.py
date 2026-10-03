#!/usr/bin/env python3
"""Build the human visual-review bundle (sprint §8).

WHY THIS EXISTS
---------------
The sprint closed every measurement gate but NOT the human gate: nobody has
looked at the `full` captures and judged whether they look like a commercial
game screenshot. Metrics can say "the sky stopped banding" and "the shadows got
darker"; they cannot say "this looks expensive". Only a human can close that.

This script does the mechanical part of that review so the human can do the
judgement part: it converts the fixed-camera PPM captures to PNG (so they open
in a normal image viewer), and lays baseline beside candidate with a measured
difference strip, per view.

It deliberately does NOT score anything. A number produced by this script
would be mistaken for the human judgement it is standing in for.

Usage:
    python3 tools/parity_review_sheet.py \
        --baseline artifacts/parity/baseline/run-a \
        --candidate artifacts/parity/ab-full \
        --out artifacts/parity/review-full
"""

import os
import sys
import json
import argparse

from PIL import Image, ImageDraw

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
VIEWS = ("close", "medium", "wide")

# Absolute magnitude → white, signed direction → red (darker) / blue (lighter).
# This is a DIFFERENCE map, not an image: its purpose is to let a reviewer see
# at a glance which parts of the frame moved, before making any judgement about
# whether the movement is good.
DIFF_SCALE = 6


def load(root, view):
    path = os.path.join(root, view, "native_capture.ppm")
    if not os.path.exists(path):
        return None
    return Image.open(path).convert("RGB")


def diff_map(base, cand):
    b = base.load()
    c = cand.load()
    w, h = base.size
    out = Image.new("RGB", (w, h))
    px = out.load()
    for y in range(h):
        for x in range(w):
            br, bg, bb = b[x, y]
            cr, cg, cb = c[x, y]
            dr = max(-255, min(255, cr - br))
            dg = max(-255, min(255, cg - bg))
            db = max(-255, min(255, cb - bb))
            lum = (dr + dg + db) / 3
            amp = min(255, int(abs(lum) * DIFF_SCALE))
            if lum < 0:
                px[x, y] = (amp, 0, 0)
            elif lum > 0:
                px[x, y] = (0, 0, amp)
            else:
                px[x, y] = (0, 0, 0)
    return out


def label(img, text):
    """Draw a caption bar above an image without mutating its pixels."""
    bar = 26
    canvas = Image.new("RGB", (img.width, img.height + bar), (16, 16, 20))
    canvas.paste(img, (0, bar))
    d = ImageDraw.Draw(canvas)
    d.text((8, 7), text, fill=(235, 235, 235))
    return canvas


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--baseline", required=True)
    ap.add_argument("--candidate", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--label", default="candidate")
    # The left panel is only the FROZEN baseline when --baseline points at it.
    # Comparing against a retained arm (e.g. ab-full-ctl) under the frozen label
    # would mislabel the evidence a human is asked to judge.
    ap.add_argument("--baseline-label", default="BASELINE (frozen)")
    args = ap.parse_args()

    os.makedirs(args.out, exist_ok=True)
    manifest = {"baseline": args.baseline, "baseline_label": args.baseline_label,
                "candidate": args.candidate, "label": args.label, "sheets": []}

    for view in VIEWS:
        base = load(args.baseline, view)
        cand = load(args.candidate, view)
        if base is None or cand is None:
            print("skip %s: missing capture" % view)
            continue

        base.save(os.path.join(args.out, "%s-baseline.png" % view))
        cand.save(os.path.join(args.out, "%s-%s.png" % (view, args.label)))

        diff = diff_map(base, cand)
        diff.save(os.path.join(args.out, "%s-diff.png" % view))

        panels = [
            label(base, "%s / %s" % (view.upper(), args.baseline_label.upper())),
            label(cand, "%s / %s" % (view.upper(), args.label.upper())),
            label(diff, "%s / DIFFERENCE (red=darker, blue=lighter)" % view.upper()),
        ]
        gap = 8
        width = sum(p.width for p in panels) + gap * (len(panels) - 1)
        height = max(p.height for p in panels)
        sheet = Image.new("RGB", (width, height), (16, 16, 20))
        x = 0
        for p in panels:
            sheet.paste(p, (x, 0))
            x += p.width + gap
        sheet_path = os.path.join(args.out, "%s-sheet.png" % view)
        sheet.save(sheet_path)
        manifest["sheets"].append(sheet_path)
        print("wrote %s" % sheet_path)

    with open(os.path.join(args.out, "manifest.json"), "w") as fh:
        json.dump(manifest, fh, indent=2, sort_keys=True)
        fh.write("\n")
    print("bundle at %s" % args.out)
    return 0


if __name__ == "__main__":
    sys.exit(main())