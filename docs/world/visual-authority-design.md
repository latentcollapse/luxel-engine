# Campaign 1: Native Visual-Quality Authority

## Registered gate

The Rust certification authority now registers the strict native visual
contract as a required `visual_quality` gate:

| Field | Value |
| --- | --- |
| Validator | `luxel.validator.visual-quality/v1` |
| Receipt schema | `luxel.visual-quality-receipt/v1` |
| Packet artifact kind | `native_graphics_scene_packet` |
| Frame receipt artifact kind | `native_graphics_frame_receipt` |
| Capture artifact kind | `native_rgba8_capture` |
| Quality evidence artifact kind | `native_visual_quality_evidence` |

The closed receipt payload names the candidate world artifact and each of the
four native graphics artifacts by ID. The receipt envelope independently binds
the exact artifact kind and raw-byte SHA-256 for every named artifact. Extra,
missing, duplicate, stale, or detached evidence is rejected.

## Independent validation

For a candidate receipt, the Rust validator:

1. Parses and validates the candidate-bound world artifact.
2. Parses the exact `GraphicsScenePacket` and checks its world artifact ID,
   world digest, and spatial-field digest against the bound world bytes.
3. Parses the exact `GraphicsFrameReceipt` and raw RGBA bytes.
4. Parses `VisualQualityEvidence` and calls
   `luxel_native_graphics_contract::validate_visual_quality_evidence` with those
   exact values. That contract verifies packet and frame-receipt identities,
   raw capture dimensions and digest, native frame measurements, profile and
   evidence digests, then recomputes the strict visual assessment.
5. Checks that the strict gate and the required reference-runtime `world` gate
   bind the same world artifact ID, kind, and digest.

The producer's outer receipt status is never used to infer success. A valid
typed evidence bundle with a matching digest is still insufficient if native
remeasurement produces another outcome. A status-only or pass-shaped payload
cannot substitute for the closed payload and five exact artifact bindings.

## Status policy

The validator derives its status only from independently reproduced
`QualityOutcome`:

| Recomputed outcome | Derived receipt status | Certification effect |
| --- | --- | --- |
| `Good` | `Pass` | Satisfies the required gate |
| `Bad` | `Fail` | Rejects certification |
| `Failed` | `Fail` | Rejects certification |
| `Indeterminate` | `Indeterminate` | Does not satisfy the required gate; certification is rejected |

Malformed JSON, bad native schemas, stale hashes, invalid packet or receipt
bindings, mismatched raw measurements, and evidence that differs from Rust
remeasurement are hard validation errors. `Indeterminate` is registered so the
authority can report a reproducible unavailable measurement without claiming
success; it is not an optional or deferred disposition in either standard
profile.

## Profile policy and compatibility

Both standard profiles now require `visual_quality` alongside the existing
`visual` gate. The existing `luxel.validator.visual-reference/v1` remains
registered and unchanged; it continues to revalidate the reference-runtime P6
capture and evidence. The additional gate evaluates the native scene packet,
promoted frame receipt, raw RGBA capture, and strict technical quality
measurements.

Engine-neutral certification therefore has seven required gates and one
explicit deferred gate: rigging. Native MVP has eight required gates, including
rigging, and no deferred gates. Strict visual quality is required in both
profiles.

The request format does not carry an independently versioned profile ID; the
authority accepts only the exact registered gate vectors. Existing clients
must fetch the current profile and submit a `visual_quality` receipt. A stale
gate vector or an old candidate without the native artifacts is rejected. The
authority does not silently reinterpret an old receipt as satisfying the new
gate. If historical profile replay must remain a supported product feature,
that needs an explicit versioned-profile contract in a later control-plane
change.

## Scope and trust boundary

This gate proves that the native packet, frame receipt, capture bytes, and
quality evidence are mutually bound and that the declared measurements are
reproduced by the Rust graphics contract. It preserves the existing native
frame checks and does not turn the technical threshold profile into an
aesthetic-quality score. Renderer-origin attestation remains bounded by the
current `GraphicsFrameReceipt` contract; this change adds no signing or device
attestation scheme.

The authority tests use deterministic structured pixel fixtures to exercise
the contract and its tampering boundaries. Those fixtures test validation
behavior, not world-rendering quality or Lava execution. The runtime integration
test still uses the actual checked-in Rust/Julia world artifact when binding
the strict native packet to the required world gate.
