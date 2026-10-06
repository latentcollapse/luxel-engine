# CHECKPOINT_MANIFEST

## Name and date

`WGE-checkpoint-goal002-2026-09-25`

Checkpoint date: 2026-09-25.

Archive, beside the repository:

`/mnt/d/Code Projects/WGE-checkpoint-goal002-2026-09-25.zip`

The zip's top directory is `WGE/`.

The Goal 001 archive is historical and was not overwritten:

`/mnt/d/Code Projects/WGE-checkpoint-goal001-2026-09-25.zip`

## Status

Goal 001 and Goal 002 are complete. Goal 003 is not implemented.

The status above is historical for the 2026-09-25 archive. On 2026-09-26 the current worktree completed the bounded Goal 003 follow-up; the archive remains an exact Goal 002 checkpoint and was not rewritten.

## Repository revision

Git HEAD, which does not contain the Goal 001 or Goal 002 edits:

`68a072415df6602bcc42558266bb84c25eeb0da5`

`68a0724 D28: judge foliage by the instances placed, not by green pixels`

The work tree at checkpoint time was dirty. Those files are in this archive. They were not committed.

Modified:

- `world_core/Cargo.toml`
- `world_core/Cargo.lock`

Untracked, present in the archive:

- `docs/archive/2026-09_native-graphics-checkpoints/checkpoint-manifest.md`
- `docs/content-sdk/semantic-kernel-handoff.md`
- `docs/archive/2026-09_roadmaps-and-audits/semantic-kernel-spike-report.md`
- `pipeline/wge_kernel_demo.py`
- `pipeline/wge_semantic_ir.py`
- `terrain_lab/bin/lane_overlap_worker.jl`
- `tests/test_semantic_kernel.py`
- `tests/test_semantic_kernel_persistence.py`
- `tests/fixtures/semantic_kernel/` (`invalid.wge`, `repaired.wge`, `repaired_note.wge`, `tampered.wge`)
- `world_core/crates/semantic_kernel/` (`Cargo.toml`, `registry_v0.json`, `src/lib.rs`, `src/lane.rs`, `src/main.rs`, `src/store.rs`, `src/worker.rs`)

Also present, gitignored, and required:

- `terrain_lab/Manifest.toml` (solver image and `julia --project`)

## Archive contents and exclusions

Included: WGE source, tests, fixtures, docs, engine adapters, `terrain_lab` source and `Manifest.toml`, and the Rust workspace manifests.

Excluded:

- `.git/`
- `world_core/target/`
- `__pycache__/`, `*.pyc`
- `.pytest_cache/`
- virtualenvs
- `*.log`
- mesh dumps (`*.glb`, `*.gltf`, `*.fbx`, `*.blend`)
- Julia depot caches (outside this tree)
- credential files (none were part of this tree)

No generation store is in the archive. A run creates one in the directory passed as `--store`.

## Test commands and results

From unpacked `WGE/`, 2026-09-25, all exit 0:

```sh
python3 -m unittest discover -s tests -v
```

646 tests, 172.400 s, OK. None skipped.

```sh
cargo test --workspace --offline
```

Run from `world_core/`. Viewer 22, worldspec 24, semantic kernel 8. None ignored.

```sh
julia --project=. --startup-file=no test/runtests.jl
```

Run from `terrain_lab/`. 5 semantic terrain-analysis tests and 3 hydrology tests passed.

## Demonstration commands

Single-process Goal 001 path, from `WGE/`:

```sh
python3 pipeline/wge_kernel_demo.py
```

Multi-process Goal 002 path on one store. Each command is a new process. Paths below are the repository fixtures.

```sh
wge-semantic-kernel fail-proposal --store STORE ...fixtures...
wge-semantic-kernel inspect --store STORE
wge-semantic-kernel apply-repair --store STORE ...fixtures...
wge-semantic-kernel inspect --store STORE
wge-semantic-kernel submit-equivalent --store STORE --source repaired_note.wge ...
wge-semantic-kernel inspect --store STORE
wge-semantic-kernel status --store STORE
```

Checkpoint result, every command exit 0:

- After `fail-proposal`, current is `G0` and repair `F-534a9577c179c0d1` is on disk.
- `apply-repair` commits `G-dca7774b2cfff7ec`, parent `G0`, receipt `R-3cd99edbf404f842`.
- Passing measurement `sha256:111e05ce547ad67fa68d69c5d8ff2f2b44a2bddfc3ed9c26080cdd534b1a6bb5`.
- Passing IR `sha256:bbe9a08e60f450f3e73d1ea76ddf484b69fe52271ce19462caa7227ac10f5e20`.
- `submit-equivalent` reports `unchanged: true` for that same generation. Generation bytes and G0 bytes do not change.
- The last `status` is still `G-dca7774b2cfff7ec`.

## Store schema

`store.json` schema is `wge.store/v0`. It also pins `registry_digest`. An unknown schema is `unsupported_schema`. There is no migration.

## Canonical generation

`wge.generation/v0`.

G0: `generation_id` `G0`, null parent, null IR, null measurement, null solver image, null receipt, the pinned `registry_digest`, empty indexes, `determinism` `exact`, `seed` null.

A committed generation adds `ir_digest`, `measurement_sha256`, `solver_image`, `commit_receipt_id`, `artifact_index` with the single key `lane_overlap`, and `evidence_index` with the single key `lane.footprint_clear`. The id is `G-` plus 16 hex chars of sha256 over parent, IR digest, measurement digest, solver image, and registry digest, newline-separated. It does not include authoring bytes, pid, host, time, or path.

Open rebuilds that body from the store's pinned registry and the fixed operation and gate ids. Any other bytes, including a same-length edit of `lane_overlap` or of G0's registry digest, are `identity_conflict`.

## Proposal

`wge.proposal/v0`: `proposal_id`, `candidate_id`, `authoring_digest`, `parent_id`, `ir_digest`, `measurement_sha256`, `operation`, `role` (`world_author`), `repair_id`, `solver_image`, `registry_digest`, `generation_id`, `receipt_id`, `status` (`rejected` or `committed`).

Candidate ids include the authoring digest. The generation id does not.

## Evidence blob

`evidence/<hex>.bin`. The kernel hashes the bytes. Same bytes are the same blob. Different bytes under that hash fail. The blob has no candidate id.

## Evidence binding

`wge.evidence-binding/v0`: `binding_id`, `candidate_id`, `parent_id`, `operation`, `gate_id`, `solver_image`, `input_digest`, `evidence_sha256`, `proposal_id`.

The binding id is recomputed from candidate, parent, evidence hash, solver image, and input digest. Two candidates may share one blob. Editing `candidate_id` inside a binding file breaks the id, so the file certifies neither the forged candidate nor the original one.

## Receipt

`wge.receipt/v0`: `receipt_id`, `generation_id`, `parent_id`, `ir_digest`, `measurement_sha256`, `registry_digest`, `solver_image`, `gate_id`, `gate_result` `pass`, `operation` `generation.commit`, `minted_by` `rust-kernel`.

No authoring digest and no candidate id. The id is a hash of generation id, measurement, gate, solver image, and registry. Open recomputes it and requires the receipt body to be that decision. A rewritten gate result, solver, IR, parent, or registry is `forged_receipt`.

## Solver image

sha256 over the length-prefixed bytes of:

- `terrain_lab/bin/lane_overlap_worker.jl`
- `terrain_lab/Project.toml`
- `terrain_lab/Manifest.toml`
- the `julia --version` line
- the protocol string `wge.worker-protocol/v0`

Not included: pid, hostname, timestamp, username, temp path. A different Julia version string changes the image. `check-solver` rejects a different image and does not move the pointer.

## Create versus open

`create-store` runs only when `store.json`, `pointer.json`, and `generations/G0.json` are all absent. It writes G0, then `store.json`, then the pointer. If any marker exists it returns `store_exists` and writes nothing.

`open` / `status` / `inspect` read the schema, the pinned registry, the pointer, the current generation, and, when current is not G0, the parent file, the receipt, and the evidence blob. They do not create G0 and they do not rewrite committed files. A missing or corrupt target fails closed.

## Atomic pointer

The generation file is written first. `pointer.json` is written to a temp file, `sync_all`, then renamed, and only if `generations/<id>.json` already exists. A failed candidate does not replace the pointer. Commit order is receipt, proposal, generation file, then pointer.

## Repair persistence and staleness

A failed proposal writes `wge.semantic-repair/v0` with the candidate, parent, base authoring digest, span, gate `lane.footprint_clear`, and repair class `move_placement_off_lane`. A later process can `apply-repair` while current is still that parent. If current has moved, the same command returns `stale_repair` and does not change the generation bytes.

## Semantic identity rule

Model A. A generation is the certified world, not the source text. A comment that does not change the canonical IR, the measurement, the solver image, or the registry is a new proposal for the same generation. `submit-equivalent` commits only when the computed generation id is already current. A different passing IR returns `not_equivalent` and does not move the pointer.

## Proven process restart

The checkpoint demonstration above is separate processes on one directory. After commit, a new process still sees `G-dca7774b2cfff7ec`, parent `G0`, the same receipt, and the same measurement blob. The equivalent-source process does not change those bytes. A further `status` is still that generation.

## Corruption and fail-closed behavior

Proved by `tests/test_semantic_kernel_persistence.py` and the Rust test `reopen_does_not_reset_and_corruption_fails_closed`, which are inside the 646:

- unsupported schema
- pointer to a missing generation
- generation body that does not match its id, including a same-length in-place edit
- G0 registry digest nibble flipped, with the pointer on the child and with the pointer on G0
- evidence bytes that do not match their hash
- forged receipt
- rewritten binding
- dangling parent file
- second `create-store`

None of these recreate G0 or repair the store.

## Goal 001 and Goal 002 contracts the tests prove

Goal 001, still green inside the 646: parse-only `.wge` source, canonical IR, one warm Julia process for both measurements, Rust predicate, `SemanticRepair` on `blocked_keep`, out-of-span edit rejected, G0 held until the in-span repair commits, host-minted receipt.

Goal 002: create is not open, the pointer survives process exit, semantic identity is separate from authoring provenance, generation and evidence bytes are immutable, bindings and receipts are checked by recomputed ids, solver image includes the Julia version, and corruption fails closed.

## Known technical debt

- One domain: lane overlap. `build_zone.py` does not use this store.
- Span check is "delete the `blocked_keep = place(...)` line and compare the rest." Declarations are single-line.
- No PackageCompiler sysimage. No migration from `wge.store/v0`.
- Power loss during the pointer rename is not injected. The tested facts are write order and a missing target.
- `terrain_lab/Manifest.toml` is gitignored. This archive includes it because the solver pin reads it.
- Pre-existing worldspec warning: unused field `steep_edge_fraction`. Those tests still passed.

## Goal 003 suggestion

Historical checkpoint suggestion only. The current worktree has since implemented the second registered operation (`path_length`) and gate (`path.within_budget`) on the same worker/store. See `docs/content-sdk/semantic-kernel-handoff.md` and `tests/test_semantic_kernel_goal003.py` for the current proof and boundaries.

The archive itself still contains only the Goal 002 state described above.
