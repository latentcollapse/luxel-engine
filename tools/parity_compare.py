#!/usr/bin/env python3
"""Registered reference comparator for the graphical parity sprint (§2.2).

Answers one question: *did this candidate move the image, in which direction,
on which registered dimensions, and by how much?* It does not judge
aesthetics. That stays with human review (§8).

DESIGN CONSTRAINTS, all of which are load-bearing rather than stylistic:

1. THRESHOLDS ARE REGISTERED HERE, NOT TUNED FROM OUTPUT. Every limit below is a
   constant chosen against the instrument's own resolution and the renderer
   contract, not fitted to make a particular candidate pass. The one exception
   is documented in place (`MEAN_LUMA_DELTA_MAX_PCT`) and is deliberately loose
   because only a human can say whether a large global exposure change is a
   wanted grade or a bug.

2. THE INSTRUMENT MUST BE ABLE TO FAIL. `self_check` runs two known-bad
   controls before any result is reported:
     - a candidate compared against ITSELF must report exactly zero change, or
       the comparison is broken; and
     - a synthetically banded image compared against the real capture must be
       reported as a large change, or the instrument is blind.
   If a control fails, the tool prints FAILURE and refuses to print PASS.

3. A GATE OVER ZERO COMPARED CELLS IS FAILURE, NOT PASS. A run with no views, or
   a view whose images disagree in size, is not a pass.

4. ALPHA IS NOT COMPARED. The capture is opaque by contract; comparing alpha
   would let a change nobody can see satisfy the gate.

5. THE BASELINE IS THE REFERENCE, AND THE REFERENCE IS FROZEN. Callers pass the
   frozen baseline directory; the tool never rewrites it.
"""

import os
import sys
import json
import argparse

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

VIEWS = ("close", "medium", "wide")

# ---------------------------------------------------------------------------
# REGISTERED THRESHOLDS
# ---------------------------------------------------------------------------

# A candidate that leaves the image completely untouched is not a "no-op
# candidate", it is a policy the renderer ignored. This is the anti-decorative-
# schema floor: any candidate arm must move at least this fraction of pixels.
MIN_CHANGED_PCT = 0.01

# Determinism floor: a control arm (no policy) must match the baseline exactly.
# Not a tolerance — exact equality. See parity_ab.sh's control arm.
CONTROL_REQUIRES_EXACT = True

# Movement bounds. A candidate that changes far more than this is almost
# certainly broken rather than artistic.
MAX_CHANGED_PCT = 99.0
MAX_MEAN_LUMA_DELTA_PCT = 12.0
MAX_HIGHLIGHT_DELTA_PCT = 1.0
MAX_CONTENT_DELTA_PCT = 5.0
MAX_SKY_BANDING_DELTA = 90.0


def read_ppm(path):
    """Minimal binary PPM (P6) reader. Returns (w, h, bytes)."""
    with open(path, "rb") as fh:
        data = fh.read()
    if not data.startswith(b"P6"):
        raise ValueError("%s is not a binary P6 PPM" % path)
    fields = []
    i = 2
    while len(fields) < 3:
        while i < len(data) and data[i:i + 1].isspace():
            i += 1
        if data[i:i + 1] == b"#":
            while i < len(data) and data[i:i + 1] != b"\n":
                i += 1
            continue
        start = i
        while i < len(data) and not data[i:i + 1].isspace():
            i += 1
        fields.append(int(data[start:i]))
    i += 1  # single whitespace byte after maxval
    w, h, maxval = fields
    if maxval != 255:
        raise ValueError("%s has maxval %d, expected 255" % (path, maxval))
    return w, h, data[i:i + w * h * 3]


def luma(r, g, b):
    # Rec.601 luma on 8-bit channels; matches the perceptual intent of the
    # luminance quantiles without needing a float pass over the whole image.
    return (r * 299 + g * 587 + b * 114) // 1000


def compare_view(base_path, cand_path):
    bw, bh, bp = read_ppm(base_path)
    cw, ch, cp = read_ppm(cand_path)
    if (bw, bh) != (cw, ch):
        return None, "size mismatch: baseline %dx%d vs candidate %dx%d" % (bw, bh, cw, ch)

    n = bw * bh
    changed = 0
    darker = 0
    lighter = 0
    same = 0
    delta_sum = 0
    absmax = 0

    base_hi = cand_hi = 0
    base_mean = cand_mean = 0
    base_vals = [0] * 256
    cand_vals = [0] * 256
    base_colors = set()
    cand_colors = set()

    for p in range(n):
        o = p * 3
        br, bg, bb = bp[o], bp[o + 1], bp[o + 2]
        cr, cg, cb = cp[o], cp[o + 1], cp[o + 2]
        bl = luma(br, bg, bb)
        cl = luma(cr, cg, cb)
        base_vals[bl] += 1
        cand_vals[cl] += 1
        base_mean += bl
        cand_mean += cl
        base_colors.add((br, bg, bb))
        cand_colors.add((cr, cg, cb))
        if bl >= 200:
            base_hi += 1
        if cl >= 200:
            cand_hi += 1
        if (br, bg, bb) == (cr, cg, cb):
            same += 1
            continue
        changed += 1
        d = cl - bl
        delta_sum += d
        if d < 0:
            darker += 1
        elif d > 0:
            lighter += 1
        if abs(d) > absmax:
            absmax = abs(d)

    def pct(v):
        return round(100.0 * v / n, 4)

    def quantile(vals, q):
        target = int(q * n)
        acc = 0
        for v in range(256):
            acc += vals[v]
            if acc > target:
                return v
        return 255

    base_mean_f = base_mean / float(n)
    cand_mean_f = cand_mean / float(n)
    mean_delta_pct = 0.0 if base_mean_f == 0 else 100.0 * (cand_mean_f - base_mean_f) / base_mean_f

    return {
        "width": bw,
        "height": bh,
        "changed_pct": pct(changed),
        "unchanged_pct": pct(same),
        "darker_pct": pct(darker),
        "lighter_pct": pct(lighter),
        "mean_luma_delta": round(cand_mean_f - base_mean_f, 4),
        "mean_luma_delta_pct": round(mean_delta_pct, 4),
        "max_abs_luma_delta": absmax,
        "luminance": {
            "baseline_p50": quantile(base_vals, 0.50),
            "candidate_p50": quantile(cand_vals, 0.50),
            "baseline_p90": quantile(base_vals, 0.90),
            "candidate_p90": quantile(cand_vals, 0.90),
            "baseline_p99": quantile(base_vals, 0.99),
            "candidate_p99": quantile(cand_vals, 0.99),
        },
        "highlight_ge_200_pct": {
            "baseline": pct(base_hi),
            "candidate": pct(cand_hi),
            "delta": round(pct(cand_hi) - pct(base_hi), 4),
        },
        "distinct_rgb": {
            "baseline": len(base_colors),
            "candidate": len(cand_colors),
            "delta": len(cand_colors) - len(base_colors),
        },
    }, None


def sky_band_delta(base_view, cand_view):
    """Sky banding movement, sourced from each side's own metrics.json when
    present. Reported only when both sides have a measurement; a missing cell
    is reported as missing, never as zero."""
    out = {}
    for v in VIEWS:
        b = base_view.get(v, {}).get("sky")
        c = cand_view.get(v, {}).get("sky")
        if not b or not c:
            out[v] = None
            continue
        out[v] = {
            "baseline_row_identical_pct": b["row_identical_pct"],
            "candidate_row_identical_pct": c["row_identical_pct"],
            "row_delta": round(c["row_identical_pct"] - b["row_identical_pct"], 2),
            "baseline_column_identical_pct": b["column_identical_pct"],
            "candidate_column_identical_pct": c["column_identical_pct"],
            "column_delta": round(c["column_identical_pct"] - b["column_identical_pct"], 2),
        }
    return out


def load_metrics(root):
    # `parity_measure.py` writes metrics.json next to the view directories, which
    # may be the run dir itself or its parent (e.g. baseline/metrics.json beside
    # baseline/run-a/). Check both so a banding delta is reported rather than
    # silently reported as "missing".
    for candidate in (os.path.join(root, "metrics.json"),
                      os.path.join(os.path.dirname(root.rstrip("/")), "metrics.json")):
        if os.path.exists(candidate):
            with open(candidate) as fh:
                return json.load(fh).get("views", {})
    return {}


def self_check(base_root):
    """Two known-bad controls. See module docstring point 2."""
    problems = []

    # Control A: a candidate compared against itself must be a perfect no-op.
    for v in VIEWS:
        p = os.path.join(base_root, v, "native_capture.ppm")
        if not os.path.exists(p):
            problems.append("control A unavailable: %s missing" % p)
            break
        res, err = compare_view(p, p)
        if err:
            problems.append("control A errored on %s: %s" % (v, err))
            break
        if res["changed_pct"] != 0.0 or res["mean_luma_delta"] != 0.0:
            problems.append(
                "control A FAILED: self-comparison of %s reported %.4f%% changed, "
                "mean delta %.4f. The comparator cannot measure zero, so no "
                "result from it is trustworthy." % (v, res["changed_pct"], res["mean_luma_delta"]))
            break

    # Control B: a deliberately corrupted image must be detected as different.
    src = os.path.join(base_root, "close", "native_capture.ppm")
    if os.path.exists(src):
        w, h, data = read_ppm(src)
        corrupted = bytearray(data)
        # Invert every pixel: maximum possible change, no ambiguity.
        for i in range(0, len(corrupted), 3):
            corrupted[i] = 255 - corrupted[i]
            corrupted[i + 1] = 255 - corrupted[i + 1]
            corrupted[i + 2] = 255 - corrupted[i + 2]
        tmp = "/tmp/_parity_compare_control_b.ppm"
        with open(tmp, "wb") as fh:
            fh.write(b"P6\n%d %d\n255\n" % (w, h))
            fh.write(bytes(corrupted))
        res, err = compare_view(src, tmp)
        if err:
            problems.append("control B errored: %s" % err)
        elif res["changed_pct"] < 99.0:
            problems.append(
                "control B FAILED: a fully inverted image was reported as only "
                "%.4f%% changed. The comparator is blind." % res["changed_pct"])
        else:
            os.remove(tmp)
    else:
        problems.append("control B unavailable: %s missing" % src)

    return problems


def gate_view(res, mode):
    """mode: 'control' requires exact no-op; 'candidate' requires real movement."""
    reasons = []
    if mode == "control":
        if res["changed_pct"] != 0.0:
            reasons.append("control arm changed %.4f%% of pixels; an absent policy "
                           "must be byte-identical" % res["changed_pct"])
        return (not reasons), reasons
    if res["changed_pct"] < MIN_CHANGED_PCT:
        reasons.append("candidate changed only %.4f%% of pixels (< %.2f%%): the policy "
                       "appears to be ignored by the renderer"
                       % (res["changed_pct"], MIN_CHANGED_PCT))
    if res["changed_pct"] > MAX_CHANGED_PCT:
        reasons.append("candidate changed %.4f%% of pixels (> %.1f%%): likely broken, "
                       "not artistic" % (res["changed_pct"], MAX_CHANGED_PCT))
    if abs(res["mean_luma_delta_pct"]) > MAX_MEAN_LUMA_DELTA_PCT:
        reasons.append("mean luminance moved %.3f%% (> %.1f%%)"
                       % (res["mean_luma_delta_pct"], MAX_MEAN_LUMA_DELTA_PCT))
    if abs(res["highlight_ge_200_pct"]["delta"]) > MAX_HIGHLIGHT_DELTA_PCT:
        reasons.append("highlight fraction moved %.4f pp (> %.1f pp)"
                       % (res["highlight_ge_200_pct"]["delta"], MAX_HIGHLIGHT_DELTA_PCT))
    return (not reasons), reasons


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("baseline_root", help="frozen reference, e.g. artifacts/parity/baseline/run-a")
    ap.add_argument("candidate_root")
    ap.add_argument("--mode", choices=("candidate", "control"), default="candidate",
                    help="'control' asserts an exact no-op (the absent-policy arm)")
    ap.add_argument("--label", default="candidate")
    ap.add_argument("--json")
    ap.add_argument("--skip-self-check", action="store_true",
                    help="NOT for acceptance use; omits the known-bad controls")
    args = ap.parse_args()

    report = {
        "label": args.label,
        "mode": args.mode,
        "baseline_root": args.baseline_root,
        "candidate_root": args.candidate_root,
        "views": {},
        "sky_band_delta": {},
    }

    problems = [] if args.skip_self_check else self_check(args.baseline_root)
    report["self_check_problems"] = problems
    controls_proven = not problems

    compared = 0
    all_pass = controls_proven
    for v in VIEWS:
        bp = os.path.join(args.baseline_root, v, "native_capture.ppm")
        cp = os.path.join(args.candidate_root, v, "native_capture.ppm")
        if not os.path.exists(bp) or not os.path.exists(cp):
            report["views"][v] = {"error": "missing capture"}
            all_pass = False
            continue
        res, err = compare_view(bp, cp)
        if err:
            report["views"][v] = {"error": err}
            all_pass = False
            continue
        passed, reasons = gate_view(res, args.mode)
        res["gate_pass"] = passed
        res["gate_reasons"] = reasons
        report["views"][v] = res
        compared += 1
        all_pass = all_pass and passed

    report["sky_band_delta"] = sky_band_delta(
        load_metrics(args.baseline_root), load_metrics(args.candidate_root))

    if compared == 0:
        all_pass = False
        report.setdefault("notes", []).append(
            "no view was compared: a gate over zero cells is FAILURE, not PASS")

    report["result"] = "PASS" if all_pass else "FAIL"

    if args.json:
        with open(args.json, "w") as fh:
            json.dump(report, fh, indent=2, sort_keys=True)
            fh.write("\n")

    for problem in problems:
        print("SELF-CHECK FAIL: %s" % problem)
    for v in VIEWS:
        r = report["views"].get(v, {})
        if "error" in r:
            print("%-7s ERROR %s" % (v, r["error"]))
            continue
        lum = r["luminance"]
        print("%-7s changed=%-8.4f%%  darker=%-8.4f%% lighter=%-8.4f%%  dMean=%-8.4f%%  "
              "p50 %d->%d  p90 %d->%d  p99 %d->%d  hi200 %+.4fpp  colors %+d"
              % (v, r["changed_pct"], r["darker_pct"], r["lighter_pct"],
                 r["mean_luma_delta_pct"],
                 lum["baseline_p50"], lum["candidate_p50"],
                 lum["baseline_p90"], lum["candidate_p90"],
                 lum["baseline_p99"], lum["candidate_p99"],
                 r["highlight_ge_200_pct"]["delta"],
                 r["distinct_rgb"]["delta"]))
        for reason in r["gate_reasons"]:
            print("        gate FAIL: %s" % reason)
    for v, d in sorted(report["sky_band_delta"].items()):
        if d is None:
            print("sky band %-7s (no metrics.json on one side; not treated as zero)" % v)
        else:
            print("sky band %-7s row %+.2f (%.2f -> %.2f)  col %+.2f (%.2f -> %.2f)"
                  % (v, d["row_delta"], d["baseline_row_identical_pct"],
                     d["candidate_row_identical_pct"],
                     d["column_delta"], d["baseline_column_identical_pct"],
                     d["candidate_column_identical_pct"]))
    print("COMPARATOR (%s): %s" % (args.mode, report["result"]))
    if args.json:
        print("wrote %s" % args.json)
    return 0 if report["result"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())