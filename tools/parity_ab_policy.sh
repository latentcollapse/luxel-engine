#!/bin/bash
# Parity sprint A/B render with an explicit candidate render policy.
# Usage: parity_ab_policy.sh OUTPUT_DIR LOGFILE POLICY(dither|dither-post|full)
set -e
cd "/mnt/d/Code Projects/WGE"
OUT="$1"
LOG="$2"
POLICY="$3"
mkdir -p "$OUT"
export WGE_PARITY_RENDER_POLICY="$POLICY"
exec world_core/target/debug/wge-native-graphics-contract render-campaign2-layout \
  world_core/crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia terrain_lab graphics_lab \
  graphics_lab/bin/wge_graphics_worker.jl \
  "$OUT" > "$LOG" 2>&1
