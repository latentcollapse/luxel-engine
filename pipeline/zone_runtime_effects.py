#!/usr/bin/env python3
"""Derive portable runtime-effect intent from reviewed ZoneSpec semantics.

Effects are a small engine-neutral contract, not arbitrary engine script. They
make the motion requirements of water, objectives, and vegetation explicit so
Godot, Unity, and Unreal can all compile equivalent runtime behavior.
"""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
from typing import Any, Iterable

from zone_compiler import ZONE_SPEC_VERSION, ZoneCompileError


RUNTIME_EFFECTS_VERSION = "codeweald.zone-runtime-effects/v1"
REALM_BANNER_COLORS = {
    "albion": [0.10, 0.32, 0.86],
    "midgard": [0.72, 0.10, 0.025],
    "hibernia": [0.05, 0.55, 0.24],
}


def build_runtime_effects(zone_spec: dict[str, Any]) -> dict[str, Any]:
    if zone_spec.get("schema_version") != ZONE_SPEC_VERSION:
        raise ZoneCompileError("Expected %s" % ZONE_SPEC_VERSION)
    effects: list[dict[str, Any]] = []
    derivation = zone_spec.get("derivation", {})
    area_ratio = float(derivation.get("area_ratio", 1.0))
    linear_scale = math.sqrt(area_ratio) if 0.0 < area_ratio < 1.0 else 1.0
    for feature in zone_spec.get("features", []):
        if not isinstance(feature, dict):
            continue
        feature_id, semantic = feature.get("id"), feature.get("semantic")
        if semantic == "stream":
            effects.append({"feature_id": feature_id, "kind": "water_flow", "loop": True, "parameters": {"flow_speed_mps": 0.65, "wave_amplitude_m": 0.10, "wave_frequency_hz": 0.8}})
        elif semantic == "arcane_ruin":
            effects.append({"feature_id": feature_id, "kind": "objective_pulse", "loop": True, "parameters": {"period_s": 2.4, "light_energy_min": 0.45, "light_energy_max": 1.6, "emission_color_srgb": [0.32, 0.03, 0.58]}})
        elif semantic == "forest":
            effects.append({"feature_id": feature_id, "kind": "foliage_wind", "loop": True, "parameters": {"gust_period_s": 7.0, "sway_degrees": 3.0, "strength": 0.38}})
        elif semantic == "faction_keep":
            realm = str(feature.get("properties", {}).get("realm", "")).lower()
            source_height = 130.0 if realm in {"albion", "hibernia"} else 88.0
            effects.append({
                "feature_id": feature_id,
                "kind": "banner_wave",
                "loop": True,
                "parameters": {
                    "period_s": 3.6,
                    "amplitude_m": round(max(0.08, 0.72 * linear_scale), 4),
                    "color_srgb": REALM_BANNER_COLORS.get(realm, [0.45, 0.45, 0.45]),
                    "banner_height_m": round(source_height * linear_scale, 4),
                    "banner_size_m": [
                        round(14.0 * linear_scale, 4),
                        round(9.0 * linear_scale, 4),
                    ],
                },
            })
    return {"schema_version": RUNTIME_EFFECTS_VERSION, "zone_id": zone_spec.get("zone", {}).get("id", "unknown"), "effects": effects}


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Derive portable Codeweald runtime effects from a ZoneSpec")
    parser.add_argument("zone_spec", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        zone_spec = json.loads(args.zone_spec.read_text(encoding="utf-8"))
        output = build_runtime_effects(zone_spec)
    except (OSError, json.JSONDecodeError, ZoneCompileError) as exc:
        parser.error(str(exc))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print("Wrote %d runtime effects at %s" % (len(output["effects"]), args.output))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
