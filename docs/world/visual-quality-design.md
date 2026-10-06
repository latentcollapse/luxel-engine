# Campaign 1 Visual Quality Evidence

Status: implemented as a separate native contract; synthetic controls cover its
initial technical thresholds. The current Riverwatch diagnostic captures are
expected to fail this profile. No production-art or aesthetic claim follows from
the gate.

## Contract boundary

`wge-native-graphics-contract::visual_quality` adds a versioned quality
assessment alongside the existing native frame gate. The existing gate still
owns capture integrity, packet binding, measurement agreement, minimum terrain
presence, and semantic-marker visibility. Quality assessment requires that
gate's complete `GraphicsFrameReceipt` to validate and then independently
measures the same RGBA8 sRGB bytes in a terrain-derived region.

The public entry points are:

```rust
VisualQualityProfile::terrain_reference_v1(width_px, height_px)
assess_visual_quality(packet, receipt, rgba_bytes, &profile)
validate_visual_quality_evidence(&evidence, packet, receipt, rgba_bytes)
```

Profiles and evidence use closed Serde structures (`deny_unknown_fields`) and
versioned schemas. Thresholds use integer basis points and byte-domain
luminance values. This keeps serialization, digests, and pass/fail decisions
deterministic.

## Region and overlay handling

The profile can measure either the complete projected terrain surface or a
declared pixel rectangle intersected with that surface. The projected-terrain
mask comes from a bounded, regularly sampled triangulation of the packet's
height field, projected through the packet camera. The maximum sample grid side
is explicit in the profile and capped at 257 by the contract. If no terrain
projects into the requested region, the result is indeterminate.

Before measuring pixels, Rust rasterizes every declared semantic point, circle,
and polyline into an exclusion mask. Exclusion does not depend on marker color,
so markers cannot provide luminance, color, edge, or tile variation. If
semantic overlays cover more than the profile limit, the result is Bad. A
screen-space UI layer is not part of the current graphics packet contract; it
must be represented and excluded by a future typed layer before it can be
treated as content evidence.

## Reference profile

`terrain_reference_v1` fixes the following defaults; the full serialized
profile travels with every result:

| Measurement | Required floor |
| --- | ---: |
| Projected region coverage | 2,000 bp of the capture |
| Overlay-free samples | 1,024 pixels |
| Semantic-overlay coverage | at most 2,500 bp of the selected region |
| Luminance P95 minus P05 | at least 32/255 |
| Occupied luminance bins | 6 of 32 |
| Occupied quantized RGB bins | 12 of 4,096 |
| Neighbor pairs over the edge threshold | 500 bp |
| Tiles with minimum luminance span | 2,500 bp of the 8×8 grid |

The measurements report exact pixel/pair/tile counts, integer ratios, the
luminance quantiles, and the terrain sampling grid size. Raster work is bounded;
unsupported size or work exhaustion produces Indeterminate rather than
silently passing.

These floors detect empty, flat, overlay-assisted, and weakly structured output.
They do not judge composition, material believability, lighting taste, visual
coherence, animation, or similarity to a shipped game. A Good result means
only that this versioned technical floor passed.

## Evidence and outcomes

The evidence body binds the profile and its digest, packet identity and digest,
capture ID, camera ID, frame-receipt digest, raw RGBA digest, dimensions, and
format. Its own canonical body receives a SHA-256 digest. The validator checks
that digest, revalidates the native receipt against the packet and bytes (which
also reruns the old gate), then recomputes the quality assessment and compares
the complete evidence.

Outcomes have distinct meanings:

- `good`: every configured technical threshold passed.
- `bad`: valid promoted pixels were measured and one or more quality thresholds
  failed; each failed threshold has a typed reason.
- `indeterminate`: the capture is valid but the requested terrain region cannot
  be measured, or a deterministic analysis budget was exhausted.
- `failed`: profile, packet, receipt, capture binding, digest, or native
  remeasurement is malformed or inconsistent.

Neither a status field nor a matching evidence digest can create a Good result.

## Controls and present capture status

The focused tests include a structured synthetic terrain control that passes,
and a flat terrain image with five large colored semantic markers. The latter
passes the old native minimum-presence gate, but the new gate excludes marker
pixels and returns Bad for excessive overlay coverage and insufficient terrain
luminance structure. Tests also tamper with capture bytes and receipt fields,
bind the wrong dimensions, reject unknown profile fields, exercise a declared
content rectangle, and revalidate the evidence.

Riverwatch remains a diagnostic capture. Its saved overview is a broad,
low-detail field with sparse semantic marks, so the reference profile is
expected to return Bad. The current screenshot sidecar records the raw capture,
packet, and receipt digests, but the screenshot bundle does not retain the full
scene packet and receipt JSON needed to replay this API against that exact old
capture. Therefore this document does not claim an API-measured Riverwatch
outcome. A future saved capture bundle should include packet and receipt JSON so
the quality result is reproducible from the handoff itself.
