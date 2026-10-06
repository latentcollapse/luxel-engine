#!/bin/bash
# Parity sprint A/B render runner.
# Usage: parity_ab.sh OUTPUT_DIR LOGFILE
set -e
cd "$(dirname "$0")/.."
OUT="$1"
LOG="$2"
mkdir -p "$OUT"
exec "${LUXEL_PARITY_BIN:-world_core/target/debug/luxel-native-graphics-contract}" render-campaign2-layout \
  world_core/crates/reference_runtime/examples/riverwatch.layout.json \
  /home/mattc/.juliaup/bin/julia terrain_lab graphics_lab \
  graphics_lab/bin/luxel_graphics_worker.jl \
  "$OUT" > "$LOG" 2>&1
