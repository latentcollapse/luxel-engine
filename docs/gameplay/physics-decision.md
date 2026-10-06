# Campaign 1 deterministic physics seam

Status: bounded reference-runtime substrate implemented; broader physical-body
semantics remain an explicit integration decision.

## Decision

The first WGE physics seam is a deterministic, grounded kinematic disc over the
already certified `WorldArtifact`. It reuses the authored traversal radius and
grade limit, Julia's height/grade/region fields, and Rust-owned obstacle,
world-bound, and navigation semantics. It does not own terrain generation,
navigation authoring, or an engine-specific physics representation.

One command supplies a finite planar velocity and the exact next tick. Rust
integrates `position += velocity / REFERENCE_TICK_RATE_HZ`, samples the path at
no more than half the smallest terrain-cell spacing, and checks the complete
segment against authored circular obstacles. Height is bilinearly sampled from
the Julia grid; the local slope bound is the maximum grade stored at the four
corners of the containing cell. The body's origin is grounded at that height.

World boundaries use the authored body radius as clearance. Obstacle circles
are expanded by that same radius, matching `cell_is_clear` and navigation.
Blocked semantic regions use the nearest Julia region-code cell. Any boundary,
region, obstacle, or over-limit slope contact rejects the full movement step;
there is no partial move, sliding, impulse, acceleration, or gravity. The
rejected tick is consumed, the body stays at its prior grounded position, and
its velocity is cleared. Stationary states may be inspected on steep terrain,
but moving across an over-limit grade is rejected.

Contacts are ordered by sweep sample, fixed contact-kind order, stable authored
ID, and position. State and step bodies serialize as closed typed JSON structs
with fixed field order; domain-separated SHA-256 digests bind the canonical
body bytes. Receipt validation replays the step against the same world.

## Controls and limits

The integration tests use the authored Riverwatch world built through the real
Julia/Rust field path. They include a clear one-tick movement, an attempted
penetration of `fallen_spire`, movement over a Julia grade above the authored
limit, malformed/stale input and forged-state rejection, stable contact order,
and deterministic receipt replay.

The sweep is bounded to 4096 field samples per command. Commands requiring more
samples fail closed. This keeps adversarial or corrupted velocities from
turning a single reference tick into unbounded work. Tangency is legal, in
keeping with the existing strict overlap comparison in navigation.

## Integration decisions still open

- The current body is a grounded planar disc because that is the shape already
  represented by navigation. A capsule height, overhead clearance, and use of
  `ObstacleSpec.height_m` need a typed collision-profile contract before they
  can affect promotion or engine lowering. For now, authored obstacles block
  the navigation footprint regardless of height, exactly as existing world
  semantics do.
- This seam accepts velocity commands and performs all-or-nothing rejection.
  Acceleration, inertia, jumping, gravity, stepping, friction, sliding, and
  dynamic bodies require an explicit gameplay/runtime contract and evidence
  semantics; they are not implied by this kinematic query layer.
- Engine adapters need a separately validated mapping from the canonical WGE
  body radius, terrain field, obstacle IDs, and contact receipts to engine
  collider handles. No engine-native solver result is certified by this
  reference seam.
- Terrain height uses bilinear cell interpolation while region identity uses
  the nearest Julia region-code cell. Sub-cell authored region geometry cannot
  be recovered from the raster field; higher-resolution or exact polygon
  queries require an explicit spatial contract change.

These limits preserve the existing runtime/navigation semantics while making
the first deterministic collision/contact path executable and replayable.
