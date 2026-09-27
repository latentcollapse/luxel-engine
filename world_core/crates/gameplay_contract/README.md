# WGE gameplay contract

This engine-neutral Rust crate defines one deterministic, single-process game
loop. Its closed snapshot includes exactly two playable entities, one guarding
NPC, a navigation graph, one energy-costed/cooldown ability, and an objective
that requires the NPC's defeat. The reference runtime consumes typed input
events and emits per-event state digests plus a final outcome receipt.

`GameSnapshot::validate` rejects unsupported schemas, malformed or disconnected
navigation, invalid role tags and attributes, silent abilities, and combat
resource budgets that cannot defeat the NPC. `run_replay` rejects invalid
actions without returning a partial receipt. `verify_replay` compares the
snapshot, trace, and receipt digests against prior evidence.

All serialized collections are ordered; runtime rules use integer state and a
fixed NPC policy. The checked-in vertical-slice fixture and expected receipt
digest pin the deterministic outcome. Run the CLI from the WGE root with:

```sh
cargo run --manifest-path world_core/crates/gameplay_contract/Cargo.toml -- \
  run tests/fixtures/gameplay_contract/vertical_slice_v1.json /tmp/receipt.json
```

The typed JSON input is a closed `{ "snapshot": ..., "trace": ... }` object;
unknown fields and unsupported schemas fail with a named `GameFailure` and a
nonzero exit code. The output is the compact canonical receipt JSON, and stdout
contains its SHA-256 identity. Run the crate tests with:

```sh
cargo test --manifest-path world_core/crates/gameplay_contract/Cargo.toml
```
