# WGE Rust workspace

`world_core/` is the Rust authority and native-runtime workspace for WGE. Its
active path owns typed project/intake/asset/gameplay/graphics contracts,
identity, receipts, validation, promotion, deterministic runtime behavior, and
native control-plane transactions. It is not a Godot, Unity, or Unreal export
layer.

Read the repository-level [docs/platform/active-architecture.md](../docs/platform/active-architecture.md)
and [docs/platform/repository-topology.md](../docs/platform/repository-topology.md) before choosing
a crate. The mega-sprint and current goal define campaign scope.

## Active crates

The workspace manifest is the source of truth. The principal native crates are:

```text
wge-control-plane
wge-project-ledger
wge-intake-repair-contract
wge-asset-contract
wge-gameplay-contract
wge-reference-runtime
wge-certification-authority
wge-live-evidence-contract
wge-native-graphics-contract
```

`semantic_kernel` and `codeweald-worldspec` remain compatibility/regression
material. They must not be treated as the current promotion authority without
an explicit reclamation campaign.

`apps/world_viewer` is bounded inspection/reference tooling. It does not make
Bevy a canonical WGE runtime or turn an external engine into a product
dependency.

## Verification

From the WGE root:

```bash
cargo fmt --manifest-path world_core/Cargo.toml --all -- --check
cargo check --manifest-path world_core/Cargo.toml --offline --workspace --all-targets
cargo test --manifest-path world_core/Cargo.toml --offline --workspace --all-targets
cargo clippy --manifest-path world_core/Cargo.toml --offline --workspace --all-targets -- -D warnings
```

The native project/receipt/runtime gates are the authoritative evidence. A
provider artifact, status-only claim, stale receipt, or compatibility snapshot
cannot promote a current project.
