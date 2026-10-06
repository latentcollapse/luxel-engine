#!/usr/bin/env python3
"""N-2 measured acceptance from a `render-calibration` summary (docs/world/converge/converge1-contracts.md §2).

  1. Chrome ball shows the sky and the ground: its upper half (reflecting sky)
     is brighter and bluer than its lower half (reflecting ground).
  2. Metal and rough metal differ in highlight blur only: mean chromaticity of
     their spheres within 0.01 (they share albedo, normals and AO).
  3. Wet vs dry stone: sphere median luminance within 25%, while the specular
     lobe (p99 minus median luminance, the highlight's excess over the body)
     differs by at least 2x.

Each is reported for the pre-IBL rig and its IBL twin so the change is visible.

Usage: ibl_acceptance.py artifacts/calibration/run3/calibration_summary.json [--rig sun]
"""
import argparse
import json


def lum(rgb):
    return 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("summary")
    ap.add_argument("--rig", default="sun")
    args = ap.parse_args()
    renders = json.load(open(args.summary))["renders"]
    probe = {(r["rig"], r["view"]): r.get("spheres", {}) for r in renders}
    results = {}
    for rig in (args.rig, f"{args.rig}-ibl"):
        out = {}
        chrome = probe.get((rig, "close-calibration"), {}).get("ball_chrome")
        if chrome:
            up, low = chrome["upper_mean_linear_rgb"], chrome["lower_mean_linear_rgb"]
            out["chrome_upper_over_lower_luminance"] = lum(up) / max(lum(low), 1e-9)
            out["chrome_upper_blue_ratio"] = up[2] / max(up[0], 1e-9)
            out["chrome_lower_blue_ratio"] = low[2] / max(low[0], 1e-9)
            out["chrome_reflects_sky_over_ground"] = lum(up) > lum(low) and up[2] / max(up[0], 1e-9) > low[2] / max(low[0], 1e-9)
        metal = probe.get((rig, "close-metal"), {}).get("metal_sphere")
        rough = probe.get((rig, "close-rough_metal"), {}).get("rough_metal_sphere")
        if metal and rough:
            d = max(abs(a - b) for a, b in zip(metal["chromaticity_rg"], rough["chromaticity_rg"]))
            out["metal_chromaticity_delta"] = d
            out["metal_chromaticity_pass"] = d <= 0.01
            out["metal_p99_over_rough_p99"] = metal["luminance_p99"] / max(rough["luminance_p99"], 1e-9)
        stone = probe.get((rig, "close-stone"), {}).get("stone_sphere")
        wet = probe.get((rig, "close-wet_stone"), {}).get("wet_stone_sphere")
        if stone and wet:
            ld = abs(wet["luminance_median"] - stone["luminance_median"]) / max(stone["luminance_median"], 1e-9)
            lobe = lambda s: max(s["luminance_p99"] - s["luminance_median"], 1e-9)
            ratio = lobe(wet) / lobe(stone)
            out["wet_dry_median_luminance_delta"] = ld
            out["wet_over_dry_specular_lobe"] = ratio
            out["wet_dry_pass"] = ld <= 0.25 and ratio >= 2.0
        results[rig] = out
    print(json.dumps(results, indent=2))


if __name__ == "__main__":
    main()
