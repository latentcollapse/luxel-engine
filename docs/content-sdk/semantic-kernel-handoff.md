# Semantic kernel handoff — proven contracts after Goal 003

Date: 2026-09-26. This file describes what the tests prove now. Goal 001's lane transaction still passes, and the Goal 003 multi-operation slice is implemented in the current worktree.

## Identity model

Semantic generation identity, not source-sensitive identity.

A generation is the certified world: parent, canonical IR digest, measurement digest, solver image, and registry digest. A comment or other byte that does not change that IR is a different proposal, not a different world. The generation record does not store `authoring_digest`. Two records with the same generation id and different bytes are an `identity_conflict`. Identical bytes are an idempotent write.

Authoring bytes live on `wge.proposal/v0`.

This is Model A. Model B was rejected because the Goal 001 record already stored `authoring_digest` inside a generation whose id ignored it, so one id could have been asked to name two bodies. Putting the digest in the id would have made a comment a different world. The proposal record keeps the "which bytes were submitted" question without that collision.

## Generation `wge.generation/v0`

G0, written only by `create-store`:

`generation_id` `G0`, `parent_id` null, `ir_digest` null, `measurement_sha256` null, `solver_image` null, `commit_receipt_id` null, `registry_digest`, empty artifact and evidence indexes, `determinism` `exact`, `seed` null.

A single-gate committed generation adds `ir_digest`, `measurement_sha256`, `solver_image`, `commit_receipt_id`, `artifact_index` keyed by operation id, and `evidence_index` keyed by gate id. A multi-gate generation instead carries those two indexes plus `receipt_index`, with `measurement_sha256` and `commit_receipt_id` set to null. Its id is `G-` plus 16 hex chars of sha256 over `parent`, IR digest, solver image, registry digest, and the canonical artifact/evidence indexes. No pid, host, time, path, or authoring digest enters either hash.

Opening recomputes that id and rebuilds the canonical single- or multi-gate body (`determinism` is `exact`, `seed` is null). Any other bytes under the same id are `identity_conflict`. A committed generation whose parent file is missing fails `missing_generation`. It does not fall back to G0.

## Proposal `wge.proposal/v0`

`proposal_id`, `candidate_id`, `authoring_digest`, `parent_id`, `ir_digest`, `measurement_sha256`, `operation`, `role` (`world_author`), `repair_id`, `solver_image`, `registry_digest`, `generation_id`, `receipt_id`, `status` (`rejected` or `committed`). For the Goal 003 specimen, a committed proposal labels the certified operation set as `lane_overlap+path_length`; the authoritative per-gate measurements remain in the generation indexes and receipts.

Candidate ids include the authoring digest, so two texts are two candidates even when they share an IR. Proposal ids include the candidate id.

## Evidence

Blob: `evidence/<hex>.bin`. The kernel hashes the bytes it stored. The same bytes are the same blob. Different bytes under that hash are `identity_conflict` or `evidence_hash_mismatch`. There is no candidate id on the blob.

Binding `wge.evidence-binding/v0`: `binding_id`, `candidate_id`, `parent_id`, `operation`, `gate_id`, `solver_image`, `input_digest`, `evidence_sha256`, `proposal_id`. Bindings are immutable. Two candidates may name one blob. `bind-evidence` recomputes the binding id from the candidate, parent, evidence hash, solver image, and input digest. A file whose `candidate_id` was edited no longer matches that id and does not certify anyone.

## Receipt `wge.receipt/v0`

One semantic decision, not one authoring text. Fields: `receipt_id`, `generation_id`, `parent_id`, `ir_digest`, `measurement_sha256`, `registry_digest`, `solver_image`, `gate_id`, `gate_result`, `operation` `generation.commit`, `minted_by` `rust-kernel`. No authoring digest and no candidate id. The id is a hash of generation id, measurement, gate, solver image, and registry. Open and `accept-receipt` recompute that id and require the receipt body to be the canonical pass decision for those generation fields. A rewritten `gate_result`, solver, IR, parent, or registry is `forged_receipt`. `submit-equivalent` commits only when the new generation id is the current one. A different passing IR returns `not_equivalent` and does not move the pointer.

## Store schema

`store.json` holds `schema` `wge.store/v0` and the store's `registry_digest`. That pin is not taken from whatever a generation file claims. `create-store` writes it only when no `store.json`, `pointer.json`, or `generations/G0.json` exists, and it writes G0 before the pointer. If any of those markers exist, create returns `store_exists` and writes nothing. A committed generation's canonical body uses only registered operation/gate ids: `lane_overlap`/`lane.footprint_clear` and, for Goal 003, `path_length`/`path.within_budget`. A same-length edit of an id, or of the genesis registry digest, is `identity_conflict`. The parent generation file is checked the same way.

`open` / `status` / `inspect` read the schema, the pointer, and the generation. Unknown schema is `unsupported_schema`. A pointer to a missing file is `missing_generation`. They do not create G0 and they do not rewrite committed files.

## Pointer

`pointer.json` is replaced only after `generations/<id>.json` exists. The write is a temp file, `sync_all`, then rename. A failed candidate does not call that replace. Commit order is receipt, proposal, generation file, then pointer.

## Corruption

Covered by `tests/test_semantic_kernel_persistence.py` and the Rust unit test `reopen_does_not_reset_and_corruption_fails_closed`: unsupported schema, missing generation, a generation body that no longer matches its id, evidence bytes that no longer match their hash, a forged receipt, a rewritten binding, a dangling parent, and a second create. `test_same_length_inplace_edits_fail_closed` edits the committed file in place, changing `lane_overlap` to `lane_overlaX` without reparsing, and flips the last nibble of G0's registry digest. Both fail `identity_conflict` with the pointer on the child and, for the digest, with the pointer on G0. Nothing is rewritten and G0 is not used as a fallback.

## Solver image

sha256 over the length-prefixed bytes of the worker script, `terrain_lab/Project.toml`, `terrain_lab/Manifest.toml`, the `julia --version` line, and the protocol string `wge.worker-protocol/v0`. Not included: pid, hostname, timestamp, username, temp path. `solver_image_changes_when_julia_version_changes` proves a different version string changes the image. `check-solver` rejects a different image without moving the pointer. `poke-worker` changes pid and keeps the image and the pointer.

## Lane domain versus the kernel

`lane.rs` owns the operation id `lane_overlap`, the gate id `lane.footprint_clear`, the repair class `move_placement_off_lane`, the pass/fail rule on the intersects flag, and the repair sentence. `path.rs` owns `path_length`, `path.within_budget`, `shorten_path`, and the length predicate. `store.rs` owns create, open, immutable bytes, and the pointer. The transaction in `lib.rs` moves proposals, blobs, bindings, receipts, and the pointer. It does not compute either domain measurement.

## What `goal002-reopen.log` shows

One store, each step a new process, every command exit 0.

1. `fail-proposal` creates the store, current `G0`, worker pid `913170`. The invalid placement measures overlap area 600 (`sha256:54b44a3d10b2d43e441ae8933dfa1d6a915b6bd2625d11c148296bdf49a91d56`). Rust fails `lane.footprint_clear`. `SemanticRepair` `F-534a9577c179c0d1` names `blocked_keep` line 5. The process stops with current still `G0`.
2. `inspect` after that process exits still reads `G0`, receipt null, and the rejected proposal `P-d4a3277e293a7e17` with authoring digest `sha256:36a4d4fe7fadf6c23bf1212930c43367995e426eeb8bbdd192f80513654214c1` and repair `F-534a9577c179c0d1`.
3. `apply-repair` is a new process, worker pid `913216`. It commits `G-dca7774b2cfff7ec`, parent `G0`, receipt `R-3cd99edbf404f842`, measurement `sha256:111e05ce547ad67fa68d69c5d8ff2f2b44a2bddfc3ed9c26080cdd534b1a6bb5`, IR `sha256:bbe9a08e60f450f3e73d1ea76ddf484b69fe52271ce19462caa7227ac10f5e20`.
4. `inspect` in the next process still shows that generation, that receipt, parent `G0`, and both proposals. The generation file hashes to `sha256:a7701f9207dc694fff54b880af43694177666e457ba2039bc959916f686e058f`. The evidence blob exists and hashes to the same measurement digest. The receipt file exists. G0's file hash is `sha256:5e30dba1e089b3ba7032e9a8fb62424b2d9c437b53b2325d18137f5401efd11b`.
5. `submit-equivalent` on the comment-only source is another process, pid `913260`. Its authoring digest is `sha256:c4d0e793a6d2419f7f0142ac240ee02350ffe029047fefbb99eb50a5cddb76b0`, candidate `C-8964327a0fe615b6`, proposal `P-0ff1086a4b523253`. The IR digest and measurement digest match the committed generation. The event is `equivalent` with `unchanged: true` and generation `G-dca7774b2cfff7ec`. The generation bytes and the G0 bytes are unchanged. The pointer stays `{"generation_id":"G-dca7774b2cfff7ec","schema":"wge.pointer/v0"}`.
6. A later `inspect` and `status` still report `G-dca7774b2cfff7ec`, the same receipt, and three proposals with three authoring digests. The two committed proposals share that generation id. The rejected proposal does not.

From this log alone, the A–Z letters that are visible are A (fresh G0), B (reopen did not reset G0), C and Y (commit, exit, reopen, still `G-dca7774b2cfff7ec`), D (a further reopen and `status` stay there), I and J (same generation, different authoring digest and proposal), K (the equivalent measurement reuses `sha256:111e05ce…`), Q and R (the failed proposal stops on G0), T (the repair is applied by a later process), and Z (the passing IR and measurement digests match the other fresh runs of this specimen). Letters that are not in this log are not claimed from it.

## Checkpoint re-run

On 2026-09-25, after the code above was already in the tree:

- `python3 -m unittest discover -s tests -v` — 646 tests, 172.400 s, OK, exit 0
- `cargo test --workspace --offline` from `world_core/` — viewer 22, worldspec 24, semantic kernel 8, exit 0
- `julia --project=. --startup-file=no test/runtests.jl` from `terrain_lab/` — 5 terrain-analysis and 3 hydrology tests passed, exit 0
- The multi-process sequence `fail-proposal`, `inspect`, `apply-repair`, `inspect`, `submit-equivalent`, `inspect`, `status` on one new store — every command exit 0, current generation `G-dca7774b2cfff7ec`, parent `G0`, receipt `R-3cd99edbf404f842`, passing measurement `sha256:111e05ce547ad67fa68d69c5d8ff2f2b44a2bddfc3ed9c26080cdd534b1a6bb5`, generation bytes unchanged by the equivalent source

## Still only claims

Crash-injection (power loss mid-rename) is not tested. The write order and the missing-target failure are what is tested. There are two specimen domains, but `build_zone.py` does not use this store. No sysimage. No migration from `wge.store/v0`.

## Goal 003 takeover result — 2026-09-26

The partially started second operation was completed without adding a second pointer or a second evidence layout. The current worktree adds `path_invalid.wge`, `path_repaired.wge`, `path_repaired_note.wge`, `path_tampered.wge`, and `tests/test_semantic_kernel_goal003.py`.

The focused four-test gate proves:

- a failing path emits `SemanticRepair` class `shorten_path` and leaves G0 current;
- lane overlap and path length run through one warm worker PID and commit as one generation with two artifact indexes, two evidence indexes, and two receipts;
- a comment-only equivalent source and a fresh independent store produce the same multi-gate generation id while preserving different authoring provenance;
- corrupting the multi-gate evidence index fails closed, while `verify-current` and `accept-receipt` accept the valid receipt index.

The first takeover pass also fixed the inherited compile break in `submit_equivalent` (the multi-operation branch had referenced an out-of-scope `solved` value), taught `verify-current` and `accept-receipt` the multi-receipt form, and records the composite operation label on committed proposals. Remaining boundaries are deliberate: this is still a specimen store, `build_zone.py` does not consume it, and crash injection/sysimage/migration are not claimed.
