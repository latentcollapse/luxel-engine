# Campaign 1: live evidence contract

Status: standalone contract slice; not integrated into the renderer or supervisor.

## Boundary

`wge-live-evidence-contract` supplies a small Rust state machine for WGE's
two-tier evidence design. It schedules Tier A sample requests, carries their
packet-bound identities, records bounded Tier B frame telemetry, and describes
session transitions. It does not certify a project, validate a receipt, capture
a frame, or own renderer behavior.

Tier A remains the existing native snapshot path: the future caller must freeze
the live packet, run it through the existing Rust-owned validators and full
promotion path, and pass a `Certified` result to this contract only after those
validators have accepted the exact snapshot. The result binds project/session,
request ID, packet and capability digests, snapshot/candidate identity, receipt
digest, validator ID, and registry digest. This crate checks those identities
against the pending request; it does not rerun the validator registry.

Tier B is a live-frame attestation with fixed bounded counters (frame and GPU
time, draw calls, submitted/visible instances, and dropped frames). It has no
certification status and no rendered-pixel digest. Its packet and capability
hashes are session identity bindings, not a per-frame image measurement.

## Scheduling and state

Periodic sampling becomes due after the configured interval. `scene_changed`,
`camera_cut`, and `capability_changed` trigger an immediate sample. If a semantic
event arrives while a sample is pending, the request is rebound to the newest
packet and the original expiry is retained; a response to the superseded
request is stale. Request triggers are sorted and deduplicated before identity
hashing.

| Current state | Event | Next state |
| --- | --- | --- |
| `live` | periodic or semantic sample trigger | `sample_pending` |
| `sample_pending` | matching Tier A accepted result | `certified` |
| `sample_pending` | missing, expired, stale, or indeterminate sample | `indeterminate` |
| `sample_pending` | Tier A rejection, including injected visual corruption | `demoted` |
| `certified` | further live frames | `certified` |
| `certified` | next periodic or semantic sample trigger | `sample_pending` |
| `indeterminate` | later periodic or semantic retry | `sample_pending` |
| any non-demoted state | invalid transition or forged pass-shaped status | `demoted` |
| `demoted` | any further transition | remains `demoted` |

The sample window is inclusive at its deadline; a result completed after the
deadline is rejected. Missing or stale evidence cannot preserve a prior
`certified` session state. The contract uses caller-supplied monotonic
milliseconds so scheduling and replay tests do not depend on wall-clock time.

Wire structs reject unknown fields. The canonical encoding is compact serde
JSON with declared struct field order and normalized trigger vectors; typed
contract payloads contain no unordered maps. Session digests cover the complete
typed state record. A `status: "pass"` field is diagnosed as forged pass-shaped
evidence rather than interpreted as a Tier A result.

## Intended integration points

1. A future Rust live supervisor creates a session after it has pinned the
   project, packet, and device-capability identities.
2. The live Lava/window path presents frames and reports only the bounded Tier B
   counters. The supervisor checks counters and monotonic frame sequence before
   accepting an attestation.
3. The scheduler requests a snapshot at the configured interval and on semantic
   events. The supervisor freezes the relevant scene packet and invokes the
   existing full Rust validation/promotion path.
4. The supervisor maps the native authority's independently validated receipt
   into `TierAResult::Certified`, `Rejected`, or `Indeterminate`; this crate
   checks request/session/packet/time correspondence and updates session state.
5. Only the existing native authority and project store can update certified
   project pointers. Tier B never moves a pointer.

## Not implemented here

- Window/swapchain presentation, frame pacing, input, or a continuously running
  game loop.
- A live renderer telemetry source, authenticity or freshness guarantees for
  those counters, or GPU synchronization/resource-lifetime policy.
- Receipt parsing, validator-registry lookup, semantic/world/gameplay/visual
  measurement, artifact capture, promotion, or pointer updates.
- Supervisor restart/recovery, persistence/replay of session state, sample
  scheduling integration, or the screenshot/video capture path.
- Physics or gameplay simulation changes.

The crate is standalone by design and is not yet a member of the `world_core`
workspace. Integration should happen only in the supervisor after this contract
has been reviewed against the actual windowed renderer and native receipt path.
