#!/bin/bash
# Parity sprint A/B render runner.
# Usage: parity_ab.sh OUTPUT_DIR LOGFILE
set -e
cd "/mnt/d/Code Projects/WGE"
OUT="$1"
LOG="$2"
mkdir -p "$OUT"
exec "${WGE_PARITY_BIN:-world_core/target/debug/wge-native-graphics-contract}" render-campaign2-layout \
  world_core/crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia terrain_lab graphics_lab \
  graphics_lab/bin/wge_graphics_worker.jl \
  "$OUT" > "$LOG" 2>&1
