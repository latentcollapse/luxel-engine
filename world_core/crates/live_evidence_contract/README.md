# WGE live evidence contract

This standalone crate defines the typed identity and session-state contract for
Campaign 1's two-tier evidence model. It is a scheduling and binding layer, not
a certification authority or renderer.

Tier A requests bind project, session, packet/capability identity, sample
window, and normalized triggers. A Tier A result can move the session into
`certified` only when its request identity matches. The supplied snapshot value
is an identity reference to a result already validated by WGE's native
certification registry. This crate does not validate that receipt or grant its
status.

Tier B carries bounded timing and draw counters for a live frame. Its schema has
no status field and no rendered-image digest. A bad telemetry record demotes the
session; a stale packet or missed/absent sample makes it indeterminate. A
rejected Tier A sample demotes the session, including the injected visual
corruption control.

The crate declares its own Cargo workspace so it can be tested without adding
it to `world_core/Cargo.toml`:

```sh
cargo test --manifest-path world_core/crates/live_evidence_contract/Cargo.toml
cargo fmt --manifest-path world_core/crates/live_evidence_contract/Cargo.toml -- --check
```

See [`CAMPAIGN1_LIVE_LOOP_DESIGN.md`](../../../docs/CAMPAIGN1_LIVE_LOOP_DESIGN.md)
for the intended supervisor integration and the intentionally unimplemented
runtime pieces.
