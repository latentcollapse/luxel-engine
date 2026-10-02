# Campaign 1 visual axis experiment

## Decision

The terrain relief readability experiment was rejected and its renderer change was reverted. The trial darkened the rolling ground, but both the Rust-reported capture-wide luminance spread and a fixed terrain crop's spread decreased. There is no measured basis to call that an improvement.

## Scope and method

The single axis tested was slope-driven terrain relief readability. The candidate changed only the response curve applied to the existing Julia-produced terrain slope at the Lava vertex stage. It did not change the scene packet, camera, material ownership, receipt thresholds, or Rust promotion logic.

Both captures use `riverwatch.layout.json`, `render-world-showcase-layout`, the same Rust `GraphicsWorkerSupervisor` path, Lava/Vulkan, and a 768×512 RGBA8 sRGB frame. The packet SHA and capture ID match exactly. Rust independently promoted both frames and remeasured each capture. The renderer identity changed for the candidate, as expected from the existing source identity check.

The local contrast proxy is grayscale standard deviation in the fixed 180×120 pixel crop at `(100, 220)` of the PPM capture. It samples the terrain interior and avoids the shrine. It is a useful comparison for this fixed camera, not a general terrain-quality score. The capture-wide standard deviation is the measurement returned in the Rust-promoted frame receipt.

## Evidence

| Measurement | Before | Slope-curve trial | Change |
| --- | ---: | ---: | ---: |
| Rust capture-wide luminance standard deviation | 0.04666205 | 0.04622062 | −0.00044143 (−0.95%) |
| Fixed terrain-crop grayscale standard deviation | 0.04919500 | 0.04831840 | −0.00087660 (−1.78%) |
| Fixed terrain-crop grayscale mean | 0.336160 | 0.332137 | −0.004023 (−1.20%) |
| Distinct capture RGB colors | 8,097 | 8,147 | +50 (+0.62%) |

The color-count increase does not offset the lower contrast: it can rise from small pixel changes without improving terrain readability. The crop and full-frame metrics agree that the candidate did not improve this axis. The detailed receipt bindings and deltas are in [visual-axis-metrics-2026-09-29.json](../artifacts/screenshots/visual-axis-metrics-2026-09-29.json).

## Captures

- [Before](../artifacts/screenshots/visual-axis-before-2026-09-29.png) — Rust-promoted baseline.
- [After candidate](../artifacts/screenshots/visual-axis-after-2026-09-29.png) — Rust-promoted slope-curve trial; intentionally retained as rejected experimental evidence.

The trial still shows the current diagnostic scene's sparse, low-detail terrain and props. A slope remap alone does not address those visible limits. No visual change is retained in `LavaAdapter.jl`.

## Verification

The real Julia protocol/parser and Lava adapter suites were rerun after restoring the original slope response. Both passed. The Rust supervisor also passed source identity, packet/capture binding, and frame promotion for both captures. The protocol contract, packet meaning, and native visual thresholds were not changed.
