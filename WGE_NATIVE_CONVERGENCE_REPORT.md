# WGE Native Convergence Report

Status: **green for the engine-neutral native convergence checkpoint**.

Date: 2026-09-28.

This checkpoint turns the Rust/Julia certification spine into the canonical
engine-neutral transaction path and exposes it through a bounded model-facing
operation surface. The existing native-MVP path remains a regression oracle;
the older Python/semantic-kernel lanes remain compatibility material and are
not the authority for a native current snapshot.

Source of truth: [GAME_DEV_HARNESS_ROADMAP.md](GAME_DEV_HARNESS_ROADMAP.md),
[AUTHORITY_RECLAMATION_001.md](AUTHORITY_RECLAMATION_001.md),
[CODEX_WGE_PEAK_SHAPE_HANDOFF.md](CODEX_WGE_PEAK_SHAPE_HANDOFF.md), the WGE
language specification, and the Rust semantic-kernel contracts.

## Scope and explicit deferrals

The certified scope is an engine-neutral vertical slice: typed intake and
ProjectSpec, Julia/Rust world construction, collision/navigation/traversal,
general gameplay, deterministic reference evidence, bounded repair authority,
and native snapshot promotion.

These remain deferred and are not represented as passes:

- production character rigging, skinning, retargeting, and arbitrary
  mesh-to-character generation;
- Unity editor import, build, and playthrough;
- Unity-versus-WGE benchmarking;
- multiplayer/netcode and universal backend parity.

The supplied bad GLB remains a permanent negative/rejection control:
`sha256:858fa104880822d081405579fb5b39d533d3b3b341d38aa1490a44b634f5e2b4`.

## Canonical ownership model

| Concern | Authority | Boundary |
| --- | --- | --- |
| ProjectSpec, typed intake/template lowering, semantic identity | Rust `project_ledger` | `compile_project_spec` and `semantic_spec_digest`; target/backend metadata is excluded from semantic identity. |
| Durable project transaction, candidate staging, current pointer, history, rollback | Rust `wge_control_plane` | `ProjectStore` is the only promotion path for this checkpoint. Open revalidates all stored identities and certification reports. |
| Receipt registry, gate profile, receipt identity, byte-bound evidence, promotion | Rust `certification_authority` | Registered validators recompute results. Producer strings, status fields, and pass-shaped evidence have no authority. |
| Terrain height/slope/region fields | Julia `terrain_lab` | Closed request/response contract; Rust verifies worker/source digests and derives the world artifact. |
| World artifact, collision, navigation, spawns, encounters, traversal, reference gameplay, reference capture | Rust `reference_runtime` | Authored layouts and Julia fields are compiled into a content-addressed native world and independently replayed. |
| Reference inspection rendering | Bevy viewer | Consumes the native world artifact. Capture provenance is versioned and Rust-validated; the registered reference visual receipt remains the promotion gate. |
| Provider/source staging and transport | Python | Parse-only authoring, byte staging, MCP/CLI argument transport, and external-provider glue. Python does not assign native identity, derive gate status, or move the native pointer. |
| Compatibility/regression lanes | legacy Python and `semantic_kernel` | Retained for regression and compatibility. Their stores do not become the native control-plane current snapshot. |

## Native transaction and receipt authority

`wge_control_plane::ProjectStore` provides the canonical lifecycle:

`create/open → inspect → stage candidate → attach typed evidence → native
validate → inspect failures → propose bounded repair → execute authorized work
→ rebuild/re-measure → validate → commit → reopen/rollback`.

The Rust CLI exposes:

```text
create
open / inspect-project
inspect-current
inspect-failures
inspect-artifact
propose-work
build-candidate / create-candidate
attach-evidence
run-playtest
capture-evidence
evaluate-candidate / verify-candidate
propose-repair
execute-work-order / apply-repair
commit-candidate
rollback
profile
```

The engine-neutral registry binds required gates for semantic, world,
gameplay, asset, visual, and repair evidence. Rigging and Unity import/build/
playthrough are registered deferred gates and only accept indeterminate
evidence in this profile. Every candidate identity includes the complete
non-repair artifact set and authorized repair IDs. Every promoted receipt is
bound to a registered validator, schema, gate, candidate, and exact evidence
bytes; the authority re-runs the validator before promotion.

The store writes candidates and snapshots through staging/atomic replacement,
publishes the pointer only after certification, rejects stale parents, checks
pointer/history agreement on reopen, and fails closed on corruption, symlink
escape, forged registry digest, stale artifacts, or mismatched reports.

## One model-facing authoring story

The authoring story is now explicit and provider-neutral:

1. `pipeline/wge_authoring.py` parses declarative JSON only. It rejects
   executable syntax, duplicate/unknown fields, malformed source bindings, and
   invalid spans. It preserves observations, inferences, assumptions,
   conflicts, confidence, evidence regions, declaration spans, provenance,
   and raw source bytes.
2. Its `lower`/`bind` CLI and Python API produce typed source/intake
   material; the Rust intake CLI assigns source identities and independently
   normalizes the provider response. Binding never rewrites caller bytes or
   invents claims.
3. `wge-project-ledger compile-spec INTAKE.json TEMPLATE.json --output
   SPEC.json` lowers canonical intake plus an explicit typed template into
   `ProjectSpec`. Missing or contradictory design decisions fail rather than
   being silently invented.
4. The native control plane creates the project transaction from that spec and
   owns all subsequent artifact, evidence, repair, and promotion decisions.

Scaffold generation is recognition-first input preparation, not a semantic
shortcut: its placeholder claim must be replaced by an explicit provider
interpretation before native intake and promotion. The old 3B-class success
criterion has been removed; WGE instead minimizes the model capability needed
for professional game-development work without sacrificing output quality.

## Native world, traversal, and gameplay proof

The native world path uses authored semantic layouts plus Julia numerical
fields and Rust-owned world artifacts. It exercises terrain features, blocked
regions, collision, navigation, required spawns, encounters, objective routes,
and actual reference traversal. The checked-in worlds include:

- `riverwatch` — the original native regression world;
- `quartz_marsh` — a materially different 120×84 marsh layout with distinct
  terrain, obstacles, blocked regions, spawns, and encounters;
- fresh `cedar_saddle_relay` input used in the recorded non-fixture native
  transaction smoke.

Gameplay is generalized behind bounded typed contracts for entities, tags,
resources, abilities, requirements, costs, cooldowns, targeting, instant and
duration effects, damage/healing/resource changes, stacking, events,
objectives, NPC policies, and deterministic replay. Two mechanically
different scenarios are covered: a five-entity combat/relay scenario with
damage, periodic effects, stacking, NPC ability use, and chained objectives;
and a resource-gated multi-patient triage scenario with area healing and
stabilization objectives.

## Bevy/reference visual evidence

The Bevy viewer accepts `--native-world` and validates the canonical native
world before rendering. Native capture provenance v2 binds:

- world and Julia spatial-field digests;
- camera and allowed view;
- PNG format and the digest of the final file reread after Bevy saves it;
- renderer ID, package, and version.

`reference_runtime::validate_bevy_capture_provenance` is Rust-owned and
fail-closed. Bevy is an inspection instrument, not a second semantic
authority. The registered reference-runtime visual receipt remains the
engine-neutral promotion gate so target-renderer quirks cannot silently alter
world acceptance. Flat/black/insufficient captures remain failures rather than
cosmetic warnings.

## Work orders and typed repair

Work orders bind parent snapshot/candidate, operation, input/output artifacts,
capabilities, required gates, write scope, and byte/artifact budgets. Native
proposal records are content-addressed and execution requires the exact
persisted authorization. Results are checked against the declared output set,
scope, capabilities, parent, regular-file identity, digest, and budget.

Repair is a native evidence loop, not a status toggle:

- `inspect_failures` revalidates the failed candidate and localizes reasons;
- `propose_repair` refuses candidates that are not natively revalidated as
  rejected and binds the failed layers and authorized artifact IDs;
- `apply-repair` requires that proposal, its candidate identity, a bounded
  work order, and exact output bytes;
- the repaired bytes are staged as a new candidate, rebuilt and re-measured;
- before/after evidence remains candidate-bound and is independently checked
  by the repair validator before promotion.

Forged proposals, unvalidated failure claims, stale parents, unauthorized
outputs, escaped writes, stale result bytes, excess budgets, and pass-shaped
receipts are rejected.

## Agent and MCP surface

`pipeline/wge_agent_surface.py` is a thin bounded adapter over the Rust CLI;
`pipeline/wge_neura_mcp.py` is its stable launcher. The MCP surface exposes
16 semantic tools with typed required fields, bounded resource roots, request/
response limits, timeouts, argument-vector subprocess calls, capability
discovery, and readable tool-level errors. It contains no candidate hashing,
receipt interpretation, gate policy, repair policy, or pointer mutation.

A real stdio smoke against the built native binary passed `initialize`,
`tools/list` (16 tools), `wge_inspect_project`, and the initialized
notification path.

## Adversarial controls

| Boundary | Green controls |
| --- | --- |
| Candidate/receipt authority | Status-only, producer-only, malformed, stale, wrong-world, changed-byte, forged/rehashed, deferred-gate, and exact-repair-delta controls. |
| World | Disconnected navigation, blocked-region, stale world/telemetry, deterministic rebuild, and second-layout controls. |
| Gameplay | Invalid requirements, targets, resources, costs, cooldowns, stack bounds, malformed schema, stale replay, and forged receipt controls. |
| Visual | Flat/black/insufficient capture failure, relabel resistance, deterministic capture, final-image digest, renderer provenance, and Rust revalidation. |
| Authoring/intake | Executable/unknown JSON rejection, duplicate keys, malformed spans/digests, stale source binding, retained conflicts, source provenance, and scaffold-first controls. |
| Work/repair | Exact capabilities, outputs, scopes, parent, proposals, symlink escape, stale result, forged proposal, unauthorized artifact, and byte-budget controls. |
| Store | Fail-closed open, registry/spec/pointer/history identity checks, path/id bounds, atomic candidate publication, and native MVP regression protection. |
| Deferred gates | Rigging and Unity are explicitly indeterminate; the bad GLB rejection control remains in the regression corpus. |

## Verification record

The final checkout was verified with:

```text
cargo fmt --manifest-path world_core/Cargo.toml --all -- --check   PASS
cargo clippy --offline --manifest-path world_core/Cargo.toml --workspace --all-targets -- -D warnings PASS
cargo test --offline --manifest-path world_core/Cargo.toml --workspace --quiet PASS
julia --project=terrain_lab terrain_lab/test/runtests.jl           PASS (8/8)
python3 -m unittest discover -s tests -q                           PASS (706 tests, 604.191s)
python3 -m unittest tests.test_wge_authoring tests.test_wge_agent_surface tests.test_wge_native_transaction tests.test_wge_control_plane -v PASS (23 tests)
python3 -m py_compile pipeline/wge_authoring.py pipeline/wge_agent_surface.py pipeline/wge_native_transaction.py pipeline/wge_neura_mcp.py PASS
```

The full Python suite's expected legacy-MVP diagnostic output still identifies
deferred/negative controls such as Unity import, runtime asset readiness, and
visual quality in its candidate fixture. The suite itself passes; those
fixture statuses are not being relabeled as engine-neutral certification.

Recorded fresh non-fixture transaction smoke:

```json
{
  "capture_status": "passed",
  "committed_status": "engine_neutral_certified",
  "playtest_outcome": "completed",
  "reopened_snapshot": "current-certified",
  "rollback_snapshot": "current-certified",
  "smoke_status": "engine_neutral_certified",
  "transaction_validation": "engine_neutral_certified"
}
```

That run used fresh Cedar Relay source/layout input, routed the final native
candidate through the Rust transaction store, performed native validation,
reference playtest, deterministic capture, commit, reopen, and rollback. A
preflight was used only to derive a typed template and provider-ready artifact
set; the final engine-neutral certification did not claim rigging. The bad GLB
remained a separate rejection control.

## Reproducibility commands

From the WGE repository root, after building the native binaries:

```sh
cargo build --offline --manifest-path world_core/Cargo.toml --workspace

python3 -m pipeline.wge_authoring lower --input authoring-source.json --output authoring-request.json
python3 -m pipeline.wge_authoring bind --input authoring-request.json --prepared-bundle source-bundle.json --output native-intake-request.json

# Native intake preparation/normalization assigns source identities and
# validates the provider response; then compile the typed ProjectSpec.
world_core/target/debug/wge-intake-repair prepare-source-bundle ...
world_core/target/debug/wge-intake-repair normalize-intake ...
world_core/target/debug/wge-project-ledger compile-spec semantic-intake.json project-template.json --output project-spec.json

world_core/target/debug/wge-control-plane create PROJECT_ROOT --spec project-spec.json --profile engine-neutral
python3 -m pipeline.wge_neura_mcp --project-root PROJECT_ROOT --control-plane world_core/target/debug/wge-control-plane
```

The MCP adapter is intentionally a transport surface; native CLI/API calls
remain available for NIRA-Prime integration without MCP.

## Final checkpoint

This checkpoint satisfies the engine-neutral convergence definition:

1. one native transaction/ledger/evidence authority is used for the
   converged path;
2. current-pointer changes occur only after native certification and reopen
   fails closed on disagreement;
3. ProjectSpec identity is engine-neutral;
4. nontrivial Rust/Julia worlds build and traverse through native artifacts;
5. Bevy consumes those artifacts with Rust-validated deterministic provenance;
6. collision, navigation, traversal, gameplay, visual, asset, semantic, and
   repair evidence are registered and revalidated;
7. gameplay is generalized beyond the original fixture;
8. bounded WorkOrder execution is exposed to the agent surface;
9. the Neura-MCP adapter exposes the semantic operations without semantic
   authority;
10. the authoring path is parse-only, scaffold-first, source-spanned, typed,
    provenance-preserving, and connected to Rust intake/ProjectSpec lowering;
11. the native-MVP regression corpus and supplied bad-GLB rejection control
    remain green;
12. the second world and second gameplay scenario use the same native model;
13. rigging and Unity remain explicitly deferred;
14. touched Python is transport/provider glue rather than promotion authority;
15. architecture, ownership, legacy boundaries, adversarial evidence, and
    reproducibility are recorded here.

Stop at this converged WGE plus agent-surface checkpoint. Do not begin Unity,
production rigging, multiplayer, or post-checkpoint breadth in this goal.
