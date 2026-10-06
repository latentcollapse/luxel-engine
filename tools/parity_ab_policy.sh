#!/bin/bash
# Parity sprint A/B render with an explicit candidate render policy.
# Usage: parity_ab_policy.sh OUTPUT_DIR LOGFILE POLICY(dither|dither-post|full|converge0|converge1|converge2|converge3|converge4)
set -e
cd "$(dirname "$0")/.."
OUT="$1"
LOG="$2"
POLICY="$3"
mkdir -p "$OUT"
export LUXEL_PARITY_RENDER_POLICY="$POLICY"
# converge0 surfaces its terrain with the scanned N-4 layer set; the files must
# be fetched first (python3 tools/fetch_terrain_layers.py tools/terrain_layers/converge0.json).
if [ "$POLICY" = "converge0" ] || [ "$POLICY" = "converge1" ] || [ "$POLICY" = "converge2" ] || [ "$POLICY" = "converge3" ] || [ "$POLICY" = "converge4" ]; then
  export LUXEL_TERRAIN_LAYER_SET="${LUXEL_TERRAIN_LAYER_SET:-tools/terrain_layers/converge0.json}"
fi
# converge2 adds the N-5 hero kit, converge3 its R-1 damaged ruin
# (build them first: python3 tools/build_kit.py --set kit1|kit2).
if [ "$POLICY" = "converge2" ]; then
  export LUXEL_KIT_SET="${LUXEL_KIT_SET:-tools/kit/kit1.lock.json}"
fi
if [ "$POLICY" = "converge3" ] || [ "$POLICY" = "converge4" ]; then
  export LUXEL_KIT_SET="${LUXEL_KIT_SET:-tools/kit/kit2.lock.json}"
fi
# converge4 adds the N-3 km-scale backdrop
# (build it first: python3 tools/build_backdrop.py --set backdrop1).
if [ "$POLICY" = "converge4" ]; then
  export LUXEL_BACKDROP_SET="${LUXEL_BACKDROP_SET:-tools/backdrop/backdrop1.lock.json}"
fi
# LUXEL_PARITY_BIN selects the binary (e.g. a release build for long N-4 runs:
# debug decodes and hashes the ~35 MB layered packets several times per view).
exec "${LUXEL_PARITY_BIN:-world_core/target/debug/luxel-native-graphics-contract}" render-campaign2-layout \
  world_core/crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia terrain_lab graphics_lab \
  graphics_lab/bin/luxel_graphics_worker.jl \
  "$OUT" > "$LOG" 2>&1
