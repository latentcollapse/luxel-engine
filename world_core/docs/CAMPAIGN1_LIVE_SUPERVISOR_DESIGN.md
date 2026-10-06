# Campaign 1: live graphics supervisor seam

`luxel-native-graphics-contract::LiveGraphicsSession` binds the existing Rust
`GraphicsWorkerSupervisor` to `luxel-live-evidence-contract`. It is an on-demand
**offscreen capture** adapter. The current worker does not own a window,
swapchain, or presentation loop. `request_live_present()` returns an explicit
unsupported outcome and never substitutes a capture for a presented frame. A
separate `window::probe_lava_window` now proves one real RenderWindow/
swapchain/present cycle, but it is a capability probe, not this session's live
scene backend.

## Identity and time

Construction validates the exact `GraphicsScenePacket` and asks the supervisor
for its typed, validated `GraphicsReady` capability payload. The packet identity
uses the packet's canonical SHA-256. The capability identity is the SHA-256 of
the canonical typed capability payload, including backend, adapter, Lava
revision, device identity, and declared features. Both values are supplied to
the live evidence contract as one `PacketIdentity`.

The adapter never reads a system clock. Callers supply session start time,
semantic-change observation time, frame observation time, tick time, and Tier A
completion time in monotonic milliseconds. Callers also supply frame sequence
numbers. The underlying evidence contract rejects clock regression, stale
packet/capability identities, and non-increasing frame sequences.

## Transitions

`synchronize_binding()` refreshes worker capabilities and compares them with the
active packet and camera. Scene, camera, and capability changes become typed
semantic events. When a sample is already pending, the live contract rebinds
that request to the newest complete identity and coalesces the trigger set.
`tick()`, `pending_sample()`, `complete_snapshot()`, and
`complete_snapshot_json()` expose the contract's bounded Tier A transitions.
Malformed, stale, missing, expired, or pass-shaped results remain failed or
indeterminate according to the contract.

`render_capture_and_record()` calls `render_and_promote()` itself. Before it
records Tier B, it checks the returned packet and receipt identities, backend
and device binding, exact telemetry agreement, and capture bytes against the
Rust receipt validator. Tier B contains bounded counters from that promoted
frame. The current worker does not expose a presentation queue: `dropped_frames`
is therefore zero for this completed on-demand capture, and unavailable GPU
timestamps map to zero as “unreported.” Neither field is evidence of real-time
presentation performance or certification.

The adapter has no public method for inserting arbitrary frame attestations.
Tier B can only be appended after this supervisor path returns a promoted
capture. Tier A certification remains subject to independent native validator
checks by the eventual certification authority; this adapter does not grant
certification.

## Determinism and limits

The session digest is the evidence contract's canonical digest. With the same
project/session identity, typed packet and capability identities, caller-supplied
times, sequence numbers, and accepted transitions, it is deterministic. It does
not include wall-clock timestamps or renderer-private mutable state.

This seam provides scheduled snapshot requests and attested offscreen captures.
It does not claim continuous gameplay, window presentation, real-time dropped
frame measurement, or a live render loop. A real presentation backend will need
its own typed supervisor interface and independently measured presentation
telemetry before it can report those properties.
