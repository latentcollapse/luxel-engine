#!/bin/bash
# Parity sprint A/B render with an explicit candidate render policy.
# Usage: parity_ab_policy.sh OUTPUT_DIR LOGFILE POLICY(dither|dither-post|full|converge0|converge1|converge2|converge3)
set -e
cd "/mnt/d/Code Projects/WGE"
OUT="$1"
LOG="$2"
POLICY="$3"
mkdir -p "$OUT"
export WGE_PARITY_RENDER_POLICY="$POLICY"
# converge0 surfaces its terrain with the scanned N-4 layer set; the files must
# be fetched first (python3 tools/fetch_terrain_layers.py tools/terrain_layers/converge0.json).
if [ "$POLICY" = "converge0" ] || [ "$POLICY" = "converge1" ] || [ "$POLICY" = "converge2" ] || [ "$POLICY" = "converge3" ]; then
  export WGE_TERRAIN_LAYER_SET="${WGE_TERRAIN_LAYER_SET:-tools/terrain_layers/converge0.json}"
fi
# converge2 adds the N-5 hero kit, converge3 its R-1 damaged ruin
# (build them first: python3 tools/build_kit.py --set kit1|kit2).
if [ "$POLICY" = "converge2" ]; then
  export WGE_KIT_SET="${WGE_KIT_SET:-tools/kit/kit1.lock.json}"
fi
if [ "$POLICY" = "converge3" ]; then
  export WGE_KIT_SET="${WGE_KIT_SET:-tools/kit/kit2.lock.json}"
fi
# WGE_PARITY_BIN selects the binary (e.g. a release build for long N-4 runs:
# debug decodes and hashes the ~35 MB layered packets several times per view).
exec "${WGE_PARITY_BIN:-world_core/target/debug/wge-native-graphics-contract}" render-campaign2-layout \
  world_core/crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia terrain_lab graphics_lab \
  graphics_lab/bin/wge_graphics_worker.jl \
  "$OUT" > "$LOG" 2>&1
