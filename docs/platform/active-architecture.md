# WGE Active Architecture

Status: canonical orientation document for the native demo-ready engine sprint.

This file answers one question for a fresh agent:

> What is the active WGE path, who owns each decision, and which files are
> historical or optional?

The execution roadmap is [docs/archive/2026-09_roadmaps-and-audits/demo-ready-mega-sprint.md](../archive/2026-09_roadmaps-and-audits/demo-ready-mega-sprint.md).
The repository inventory is [docs/platform/repository-topology.md](repository-topology.md).
The recurring agent-friction evidence is [docs/platform/friction-ledger.md](friction-ledger.md).

## Product doctrine

WGE is the game engine. The shipped game must run on WGE's native runtime and
must not require Unity, Unreal, Godot, Blender, or another game runtime.

The preferred runtime/build envelope is:

```text
Rust        semantic authority, identity, contracts, runtime, certification
Julia       numerical/spatial execution behind typed packets
Lava/Vulkan GPU execution and native graphics backend
Python      transport, staging, provider orchestration, optional DCC glue
```

Blender is an optional build-time provider. It may prepare meshes, UVs, rigs,
weights, animations, retargets, or bakes. It does not own project truth,
gameplay semantics, certification, runtime execution, or promotion. Every
provider artifact is reimported and independently validated by WGE.

Bevy and other inspection instruments are not semantic authority and are not a
runtime dependency of a shipped WGE game. They may remain useful as bounded
reference/inspection tooling where an existing contract explicitly names them.
No backend representation becomes canonical WGE state.

## Authority and ownership

| Concern | Canonical owner | Boundary rule |
|---|---|---|
| Project/spec identity, receipts, promotion | `world_core/crates/wge-control-plane` and its Rust contracts | Python may transport requests; it cannot decide identity, gate status, or promotion |
| Typed intake and bounded repair contracts | `wge-intake-repair-contract` plus Rust promotion path | Observations, inferences, assumptions, conflicts, confidence, regions, and provenance remain explicit |
| Assets, source identity, validation | `wge-asset-contract` and Rust authority | External/DCC output is an input artifact, never authority |
| Gameplay contracts | `wge-gameplay-contract` | Runtime behavior is represented by typed contracts and evidence, not provider conventions |
| World/traversal/runtime semantics | Rust reference/native runtime crates | Julia supplies numerical fields; Rust validates and owns meaning |
| Numerical terrain/spatial fields | `terrain_lab` Julia package | Julia does not promote semantic state or invent unbound identity |
| Native graphics packet execution | `graphics_lab` and Lava behind typed Rust-owned packets | Julia/Lava execution is coarse-grained; no per-frame semantic FFI choreography |
| Native live evidence | `wge-live-evidence-contract` plus certification authority | Tier-B live telemetry is cheap; Tier-A snapshots are independently promoted |
| Certification and validator registry | `wge-certification-authority` / registered Rust validators | Status-only, producer-only, stale, malformed, or fake evidence fails closed |
| Transport and provider orchestration | `pipeline/wge_*.py` and provider modules | Parse, stage, invoke, and serialize; do not become semantic authority |

## Active source-of-truth hierarchy

When documents disagree, use this order:

1. The active user goal and its explicit exclusions.
2. `docs/archive/2026-09_roadmaps-and-audits/authority-reclamation-001.md` and the current semantic-kernel contracts.
3. `docs/archive/2026-09_roadmaps-and-audits/demo-ready-mega-sprint.md` and campaign-specific active reports.
4. Rust contract implementations and their tests.
5. Locked Julia/Lava projects and their tests.
6. Current native control-plane/provider documentation.
7. Historical migration, MVP, and pre-convergence documents, for context only.

The repository topology inventory is descriptive evidence, not a new semantic
authority. If code and prose disagree, the contract and its passing gates win;
the prose must then be corrected.

## Active tree

### Rust workspace

`world_core/Cargo.toml` is the active Rust workspace. The native path is built
from the workspace crates, especially:

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
material until a campaign explicitly retires them. They must not be treated as
the current native promotion path.

`world_core/apps/world_viewer` is an inspection/reference instrument. It is not
part of the native authority source roots and is not permission to make Bevy,
Godot, Unity, or Unreal a canonical runtime backend.

### Julia/Lava

`graphics_lab/` and `terrain_lab/` are first-class active packages with locked
`Project.toml`/`Manifest.toml` environments. Their interfaces are typed,
coarse-grained packet seams. Julia owns numerical execution; Rust owns the
meaning and promotion of the result.

### Python surface

The current model-facing and native orchestration surface includes:

```text
pipeline/wge_agent_surface.py
pipeline/wge_native_transaction.py
pipeline/wge_native_mvp.py
pipeline/wge_neura_mcp.py
pipeline/wge_source_intake.py
pipeline/wge_engine_neutral.py
```

These are transport/staging/orchestration lanes. Provider modules such as
`pipeline/rigging_provider.py` may call Blender or another bounded tool, but
their output re-enters through typed Rust validation. New semantic behavior
must not be added to legacy Python-owned pipeline modules.

## Model-facing construction surface

The low-level transaction surface currently exposes inspect, propose, apply,
build, evidence, playtest, repair, verify, commit, and rollback operations.
The first read-only capability discovery slice is now exposed by the native
control plane and the transport surface. The mega-sprint is extending this
toward discoverable semantic operations such as:

```text
project.intake / project.plan
capability.list / capability.explain / capability.plan
style.compile
world.construct / scene.compose
asset.prepare / character.prepare
gameplay.compose
runtime.launch
quality.inspect
repair.propose / repair.apply
project.verify / project.package
```

The Rust-owned semantic facade catalog is also available through
`facade_list`/`facade_explain`. It distinguishes callable, partial, planned,
and deferred verbs; planned verbs are intentionally not exposed as executable
transport. `capability.list`, `capability.explain`, typed `style.compile`, and
the read-only `project.plan` / `construction_validate` slices are callable now;
world/asset/gameplay execution and promotion remain subsequent C1/C2 slices.
Names are provisional until their Rust-owned contracts land. A fresh agent
should use the declared surface and inspection responses rather than search
backend source for undocumented commands.

## Verification commands

From `WGE/`:

```bash
# Bounded native gate: fences + semantic Rust members + focused Python control tests
python3 pipeline/wge_native_gate.py

# Full cold graphics/runtime evidence (separate, intentionally expensive)
python3 pipeline/wge_native_gate.py --gpu

# Native Rust members (default-members intentionally excludes compatibility apps/crates)
cargo check --manifest-path world_core/Cargo.toml --offline --all-targets
cargo test --manifest-path world_core/Cargo.toml --offline --all-targets
cargo clippy --manifest-path world_core/Cargo.toml --offline --all-targets -- -D warnings
cargo fmt --manifest-path world_core/Cargo.toml --all -- --check

# Numerical packages
julia --project=graphics_lab --startup-file=no graphics_lab/test/runtests.jl
julia --project=terrain_lab --startup-file=no terrain_lab/test/runtests.jl

# Focused native Python transport/control regression
python3 -m unittest tests.test_wge_control_plane tests.test_wge_engine_neutral tests.test_wge_native_mvp
```

The broad historical `tests/` discovery lane contains compatibility/provider
tests and is not the default native gate or proof of native certification. Run
it only as an explicit compatibility audit. `cargo test --workspace` likewise
intentionally includes the quarantined compatibility/reference members and is
not the first command for a fresh agent. The `--gpu` mode is the explicit cold
Lava/Vulkan certification run; its wall time is tracked separately from the
fast semantic gate.

## Archive and provider fence

The following are useful references or compatibility lanes, not active native
authority:

```text
engine_adapters/unity/
engine_adapters/unreal/
pipeline/zone_to_unity.py
pipeline/zone_to_unreal.py
pipeline/zone_spec_to_godot.py
pipeline/zone_assets_to_godot.py
pipeline/*unreal*
legacy terrain/ecology/hydrology scripts not reached by the native path
old Godot/Codeweald project material
```

Do not delete these blindly: a later campaign may salvage an algorithm or use
a regression specimen. Do not import them into active canonical code. Any
resurrection requires a typed contract, an ownership decision, and evidence.

Tracked intake reports under `asset_intake_reports/` are retained only as
fixture/evidence records for asset controls; they are not source authority.
New generated reports and caches belong under ignored output directories.

## Current execution rule

Work campaign-by-campaign from the mega-sprint. Before adding a feature:

1. identify its authoritative contract and write scope;
2. use the smallest native slice that proves the semantic behavior;
3. preserve known-good and known-bad controls, hashes, provenance, and restart;
4. run the relevant Rust/Julia/Python gates;
5. adversarially test the authority boundary;
6. record friction, rejected experiments, and remaining gaps;
7. do not begin the next dependent campaign until the slice is green.

The demo exit is not “all engine features exist.” It is a fresh-agent,
native-runtime, high-quality vertical slice with independently certified
evidence, bounded repair, deterministic rebuild, and no runtime dependency on a
provider or external engine.
