#!/usr/bin/env python3
"""Judge foliage by the instances the plan placed, not by green pixels.

**Why this module exists (D28).** `bevy_visual_acceptance` measured foliage as
a green-pixel share of the whole capture. That share moves when the *frame's
composition* moves, and composition is a function of terrain height: two builds
of the alpine arena went 275 -> 301 render-plan instances while
`foliage_fraction` fell 0.006820 -> 0.005207. More trees, lower metric.

Normalising by the foreground was the fix direction originally recorded under
D24, and it is not sufficient here. It removes the *sky's* share of the effect
and leaves the *rock's*: a world that gains a taller massif has proportionally
more rock and shadow among its own foreground pixels, so green-share-of-world
still falls without a single tree being removed. The denominator has to stop
being an area at all.

So the denominator is the instance count. The viewer projects each spawned
foliage entity through the real capture camera and writes
`<capture>_foliage_projection.json`; this gate reads that artifact. Adding a
tree can now only ever raise the numerator or leave it unchanged, which is the
property the pixel metric could not have at any threshold.

**What this does not prove.** Frustum containment is not occlusion: an instance
standing behind a ridge is reported in frame. The pixel probe below is what
distinguishes drawn from merely-framed, and it is reported rather than gated
until it has been measured against a real build — setting a floor from a number
nobody has observed is the mistake D25 and D28 both record.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

import numpy as np

PROJECTION_VERSION = "codeweald.bevy-foliage-projection/v1"
ACCEPTANCE_VERSION = "codeweald.foliage-projection-acceptance/v1"

# How far above an instance's base to look for canopy, in world metres.
#
# `position_m` carries the grounding offset already subtracted, so a projected
# instance point sits at the foot of the trunk; sampling it reads ground. Two
# metres is not a tuned value — it is a floor on what could be called foliage
# at all, chosen so the probe reaches canopy on the shortest plausible asset
# rather than to make any particular world pass.
PROBE_HEIGHT_M = 2.0

# Half-width of the probe column, as a fraction of its height. A canopy is
# wider than it is tall far more often than the reverse, so this is deliberately
# not square.
PROBE_HALF_WIDTH_RATIO = 0.6


def foliage_mask(analysis: np.ndarray) -> np.ndarray:
    """The foliage hue test, shared with `bevy_visual_acceptance`.

    Kept identical on purpose: the point of this module is to change the
    *denominator*, not to quietly change what counts as a green pixel at the
    same time. A different hue test here would make the two metrics
    incomparable and hide which change moved the number.
    """
    red, green, blue = analysis[:, :, 0], analysis[:, :, 1], analysis[:, :, 2]
    return (green >= 0.20) & (green >= red * 1.08) & (green >= blue * 1.05)


def _probe_hit(mask: np.ndarray, x: float, y: float, pixels_per_metre: float) -> bool:
    """Is there a foliage-hued pixel in the canopy column above this base?"""
    height, width = mask.shape
    span = max(1.0, pixels_per_metre * PROBE_HEIGHT_M)
    half_width = max(1.0, span * PROBE_HALF_WIDTH_RATIO)
    # Screen y decreases upward, so the canopy is at smaller y than the base.
    top = int(round(y - span))
    bottom = int(round(y))
    left = int(round(x - half_width))
    right = int(round(x + half_width))
    top = max(0, min(height, top))
    bottom = max(0, min(height, bottom + 1))
    left = max(0, min(width, left))
    right = max(0, min(width, right + 1))
    if top >= bottom or left >= right:
        return False
    return bool(mask[top:bottom, left:right].any())


def evaluate(
    projection: dict[str, Any],
    analysis: np.ndarray | None = None,
) -> dict[str, Any]:
    """Score a foliage projection artifact, optionally probing the capture.

    `analysis` is the HxWx3 float RGB capture. Without it the pixel probe is
    skipped and only the framing metrics are reported.
    """
    failures: list[str] = []
    version = projection.get("schema_version")
    if version != PROJECTION_VERSION:
        failures.append(
            f"Foliage projection schema is {version!r}, expected {PROJECTION_VERSION!r}"
        )
        return {
            "schema_version": ACCEPTANCE_VERSION,
            "status": "failed",
            "failures": failures,
            "metrics": {},
        }

    instances = projection.get("instances") or []
    instance_count = len(instances)
    in_frame = [
        instance
        for instance in instances
        if instance.get("in_frame") and instance.get("viewport_px")
    ]
    in_frame_count = len(in_frame)
    in_frame_fraction = in_frame_count / instance_count if instance_count else 0.0

    mask = foliage_mask(analysis) if analysis is not None else None
    probed = 0
    hits = 0
    if mask is not None:
        for instance in in_frame:
            scale = instance.get("pixels_per_metre")
            point = instance.get("viewport_px")
            if not scale or scale <= 0 or not point:
                continue
            probed += 1
            if _probe_hit(mask, float(point[0]), float(point[1]), float(scale)):
                hits += 1
    hit_fraction = hits / probed if probed else 0.0

    # The plan placing no foliage at all is a different failure from the
    # foliage not being visible, and conflating them would send anyone
    # debugging it to the wrong file.
    if instance_count == 0:
        failures.append("Compiled world placed no foliage instances")
    elif in_frame_count == 0:
        failures.append("Compiled foliage is entirely outside the capture frame")

    return {
        "schema_version": ACCEPTANCE_VERSION,
        "status": "failed" if failures else "passed",
        "failures": failures,
        "metrics": {
            "instance_count": instance_count,
            "in_frame_count": in_frame_count,
            "in_frame_fraction": round(in_frame_fraction, 6),
            # Reported, deliberately not gated. This is the number that
            # distinguishes drawn from merely-framed, and it has never been
            # measured on a real build; a floor set before that would be the
            # invented threshold D28 exists to complain about.
            "probed_count": probed,
            "probe_hit_count": hits,
            "probe_hit_fraction": round(hit_fraction, 6),
            "probe_height_m": PROBE_HEIGHT_M,
            "viewport_px": projection.get("viewport_px"),
        },
        "measures": {
            "in_frame_fraction": (
                "Share of spawned foliage instances inside the capture frustum. "
                "Composition-independent: the denominator is a count of trees, "
                "so no reframing can lower it. Does not test occlusion."
            ),
            "probe_hit_fraction": (
                "Share of in-frame instances with a foliage-hued pixel in the "
                "canopy column above their base. Distinguishes drawn from "
                "framed. Occluded instances read as misses, which is honest for "
                "a visibility metric but means this is not a render-failure "
                "detector on its own."
            ),
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("projection", type=Path)
    parser.add_argument("--capture", type=Path, default=None)
    parser.add_argument("--output", type=Path, default=None)
    args = parser.parse_args()

    projection = json.loads(args.projection.read_text(encoding="utf-8"))
    analysis = None
    if args.capture is not None:
        from PIL import Image

        image = Image.open(args.capture).convert("RGB")
        analysis = np.asarray(image, dtype=np.float32) / 255.0

    report = evaluate(projection, analysis)
    text = json.dumps(report, indent=2)
    if args.output is not None:
        args.output.write_text(text + "\n", encoding="utf-8")
    print(text)
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
