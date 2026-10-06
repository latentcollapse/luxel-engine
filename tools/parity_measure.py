#!/usr/bin/env python3
"""Luxel graphical-parity measurement instrument.

Reads a campaign2 capture tree and emits a deterministic metric set used for
before/after A/B comparison. Every number here is computed from the promoted
PPM/RGBA bytes only -- no packet or receipt claims are trusted.

Usage:
    parity_measure.py CAPTURE_ROOT [--json OUT.json] [--label NAME]

CAPTURE_ROOT contains close/ medium/ wide/ subdirectories each holding
native_capture.ppm.
"""
import sys, os, json, hashlib, argparse

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def read_ppm(path):
    with open(path, 'rb') as f:
        assert f.readline().strip() == b'P6', "not a P6 ppm: %s" % path
        line = f.readline()
        while line.startswith(b'#'):
            line = f.readline()
        w, h = map(int, line.split())
        f.readline()  # maxval
        data = f.read()
    assert len(data) == w * h * 3, "ppm payload %d != %d" % (len(data), w * h * 3)
    return w, h, data


def luma(r, g, b):
    return (r * 299 + g * 587 + b * 114) // 1000


def measure_view(path):
    w, h, d = read_ppm(path)
    n = w * h
    lum = [0] * n
    for i in range(n):
        o = i * 3
        lum[i] = luma(d[o], d[o + 1], d[o + 2])
    sl = sorted(lum)

    def q(f):
        return sl[min(n - 1, int(n * f))]

    colors = set()
    sat = 0
    sat_low = 0
    for i in range(n):
        o = i * 3
        r, g, b = d[o], d[o + 1], d[o + 2]
        colors.add((r, g, b))
        s = max(r, g, b) - min(r, g, b)
        sat += s
        if s <= 4:
            sat_low += 1

    # --- sky banding: the defect that motivated the dither slice -------------
    top_rows = max(1, h // 4)
    col_identical = 0
    col_pairs = 0
    x = w // 2
    prev = None
    for y in range(top_rows):
        o = (y * w + x) * 3
        c = (d[o], d[o + 1], d[o + 2])
        if prev is not None:
            col_pairs += 1
            if c == prev:
                col_identical += 1
        prev = c
    row_identical = 0
    row_pairs = 0
    for yy in (0, max(0, top_rows // 2)):
        prev = None
        for xx in range(w):
            o = (yy * w + xx) * 3
            c = (d[o], d[o + 1], d[o + 2])
            if prev is not None:
                row_pairs += 1
                if c == prev:
                    row_identical += 1
            prev = c

    # --- sky coverage (top quarter, bluish) ---------------------------------
    sky = 0
    tot = 0
    for y in range(top_rows):
        for xx in range(0, w, 2):
            o = (y * w + xx) * 3
            r, g, b = d[o], d[o + 1], d[o + 2]
            tot += 1
            if b > r + 6 and b >= g - 2:
                sky += 1

    # --- content coverage: pixels differing from the sky gradient -----------
    # Sample the sky reference as the median colour of the topmost row.
    row0 = []
    for xx in range(0, w, 4):
        o = (0 * w + xx) * 3
        row0.append((d[o], d[o + 1], d[o + 2]))
    row0.sort()
    ref = row0[len(row0) // 2]
    content = 0
    for i in range(n):
        o = i * 3
        if abs(d[o] - ref[0]) + abs(d[o + 1] - ref[1]) + abs(d[o + 2] - ref[2]) > 18:
            content += 1

    # --- local contrast in the lower (terrain) half -------------------------
    lc = 0
    lcp = 0
    for y in range(h // 2, h):
        for xx in range(0, w - 1):
            o = (y * w + xx) * 3
            o2 = o + 3
            lc += abs(luma(d[o], d[o + 1], d[o + 2]) - luma(d[o2], d[o2 + 1], d[o2 + 2]))
            lcp += 1

    return {
        "width": w,
        "height": h,
        "pixels": n,
        "ppm_sha256": "sha256:" + hashlib.sha256(open(path, 'rb').read()).hexdigest(),
        "luminance": {
            "p01": q(0.01), "p05": q(0.05), "p50": q(0.50), "p90": q(0.90),
            "p95": q(0.95), "p99": q(0.99), "p999": q(0.999),
            "mean": round(sum(lum) / n, 3),
        },
        "highlight_fraction_pct": {
            "ge_200": round(100.0 * sum(1 for v in lum if v >= 200) / n, 4),
            "ge_240": round(100.0 * sum(1 for v in lum if v >= 240) / n, 4),
        },
        "colour": {
            "distinct_rgb": len(colors),
            "mean_saturation": round(sat / n, 3),
            "pct_saturation_le_4": round(100.0 * sat_low / n, 3),
        },
        "sky": {
            "top_quarter_coverage_pct": round(100.0 * sky / max(tot, 1), 2),
            "column_identical_pairs": col_identical,
            "column_pairs": col_pairs,
            "column_identical_pct": round(100.0 * col_identical / max(col_pairs, 1), 2),
            "row_identical_pairs": row_identical,
            "row_pairs": row_pairs,
            "row_identical_pct": round(100.0 * row_identical / max(row_pairs, 1), 2),
        },
        "content_coverage_pct": round(100.0 * content / n, 3),
        "terrain_local_contrast": round(lc / max(lcp, 1), 4),
    }


# ---------------------------------------------------------------------------
# REGISTERED SKY-BANDING GATE (sprint §3.1 / §2.4)
#
# Thresholds are registered HERE, in a file that is not fed by the
# measurement's output, and the gate is deliberately one-sided: dithering can
# only push the identical-neighbour fraction DOWN or leave it flat, never up.
#
# The residual is not zero and is not expected to be. An ORDERED 8x8 Bayer
# pattern is periodic, so a fixed fraction of adjacent pixel pairs legitimately
# receive the same offset. The observed candidate value is 18.77% in all three
# views — IDENTICAL across views of different size and content, which is the
# signature of the pattern's own period rather than of scene-dependent sky
# banding. A true residual-banding artefact would vary per view.
#
# KNOWN-BAD CONTROL: the frozen baseline, which scores 98.37 / 100.0 / 100.0
# and therefore FAILS this gate by a wide margin. If the gate ever passes the
# frozen baseline, the instrument is broken and every candidate result is void.
# ---------------------------------------------------------------------------
SKY_ROW_IDENTICAL_MAX_PCT = 40.0
SKY_COLUMN_IDENTICAL_MAX_PCT = 40.0
KNOWN_BAD_CONTROL_ROOT = "artifacts/parity/baseline/run-a"


def sky_gate(view):
    """Return (passed, reasons). Never returns PASS on an ungated view."""
    s = view["sky"]
    reasons = []
    if s["row_pairs"] == 0 or s["column_pairs"] == 0:
        return False, ["sky scan produced no comparable pairs: the gate gated nothing"]
    if s["row_identical_pct"] > SKY_ROW_IDENTICAL_MAX_PCT:
        reasons.append("sky row_identical_pct %.2f > %.1f"
                       % (s["row_identical_pct"], SKY_ROW_IDENTICAL_MAX_PCT))
    if s["column_identical_pct"] > SKY_COLUMN_IDENTICAL_MAX_PCT:
        reasons.append("sky column_identical_pct %.2f > %.1f"
                       % (s["column_identical_pct"], SKY_COLUMN_IDENTICAL_MAX_PCT))
    return (not reasons), reasons


def self_check():
    """Prove the gate can FAIL. A gate that cannot fail is not a gate."""
    root = os.path.join(REPO_ROOT, KNOWN_BAD_CONTROL_ROOT)
    control = os.path.join(root, "close", "native_capture.ppm")
    if not os.path.exists(control):
        return ["known-bad control missing at %s: gate unproven, refusing to report PASS" % control]
    view = measure_view(control)
    passed, reasons = sky_gate(view)
    if passed:
        return ["known-bad control PASSED the sky gate (row_identical_pct=%.2f): "
                "the measurement cannot detect banding and every result is void"
                % view["sky"]["row_identical_pct"]]
    return []


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("root")
    ap.add_argument("--json")
    ap.add_argument("--label", default="run")
    ap.add_argument("--no-gate", action="store_true",
                    help="report metrics only; the gate result is then simply absent")
    args = ap.parse_args()

    out = {"label": args.label, "root": args.root, "views": {}}
    for v in ("close", "medium", "wide"):
        p = os.path.join(args.root, v, "native_capture.ppm")
        if not os.path.exists(p):
            continue
        out["views"][v] = measure_view(p)

    gate = {"enabled": not args.no_gate, "views": {}, "self_check": []}
    if not args.no_gate:
        gate["self_check"] = self_check()
        control_proven = not gate["self_check"]
        gate["known_bad_control_proven"] = control_proven
        all_pass = control_proven
        for v, m in out["views"].items():
            passed, reasons = sky_gate(m)
            gate["views"][v] = {"pass": passed, "reasons": reasons}
            all_pass = all_pass and passed
        if not out["views"]:
            gate["views"] = {}
            all_pass = False
            gate["self_check"] = gate["self_check"] or [
                "no view was measured: a gate over zero cells is FAILURE, not PASS"]
        gate["result"] = "PASS" if all_pass else "FAIL"
    out["sky_banding_gate"] = gate

    if args.json:
        with open(args.json, "w") as f:
            json.dump(out, f, indent=2, sort_keys=True)
            f.write("\n")

    for v, m in out["views"].items():
        s = m["sky"]
        print("%-7s %dx%d  p50=%-4d p90=%-4d p99=%-4d  hi>=200=%-7.4f%%  colors=%-6d  content=%-6.2f%%  sky_identical_col=%-6.2f%%  row=%-6.2f%%" % (
            v, m["width"], m["height"],
            m["luminance"]["p50"], m["luminance"]["p90"], m["luminance"]["p99"],
            m["highlight_fraction_pct"]["ge_200"], m["colour"]["distinct_rgb"],
            m["content_coverage_pct"], s["column_identical_pct"], s["row_identical_pct"]))
    if not args.no_gate:
        for problem in gate["self_check"]:
            print("SELF-CHECK FAIL: %s" % problem)
        for v, r in sorted(gate["views"].items()):
            print("gate %-7s %s%s" % (v, "PASS" if r["pass"] else "FAIL",
                                      "" if r["pass"] else "  <- " + "; ".join(r["reasons"])))
        print("SKY BANDING GATE: %s" % gate["result"])
    print("wrote %s" % (args.json or "(stdout only)"))


if __name__ == "__main__":
    main()
