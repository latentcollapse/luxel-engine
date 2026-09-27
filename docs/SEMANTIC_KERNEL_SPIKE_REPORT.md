# Spike report — prove the semantic transaction kernel

Goal 001 is complete. Goal 002 changed store identity after this report was first written. The generation record no longer contains `authoring_digest`, evidence blobs no longer carry `candidate_id`, and the solver image includes the Julia version line. For the contracts that tests prove now, read `docs/SEMANTIC_KERNEL_HANDOFF.md`. The narrative below is the Goal 001 specimen, not the current store schema.

## 1. What changed

A closed authoring file is parsed as data, lowered to canonical typed IR, and handed to a Rust kernel. The kernel opens a candidate, asks one warm Julia process to measure rectangle overlap, applies `lane.footprint_clear` itself, and either writes a `SemanticRepair` or commits an immutable generation. The author role cannot commit, cannot invoke the solver by name, and cannot send Julia source.

The specimen is one lane and one placement. The first file intersects. The repair moves only that placement. A second file that also changes another line is rejected. D29 was not touched.

## 2. Files

Added:

- `world_core/crates/semantic_kernel/` (library, binary, `registry_v0.json`)
- `terrain_lab/bin/lane_overlap_worker.jl`
- `pipeline/wge_semantic_ir.py`
- `pipeline/wge_kernel_demo.py`
- `tests/test_semantic_kernel.py`
- `tests/fixtures/semantic_kernel/{invalid,repaired,tampered}.wge`
- `docs/SEMANTIC_KERNEL_HANDOFF.md`
- this report

Modified: `world_core/Cargo.toml` (workspace member).

Existing terrain, worldspec, worldbuilder, and `build_zone.py` paths were not rewritten.

## 3. Spike architecture

```text
.wge text
  → Python parse_module (no exec) → wge.semantic-ir/v0
  → Rust kernel store
       generation.propose
       solver.invoke  →  warm Julia lane_overlap
       Rust predicate lane.footprint_clear
       SemanticRepair  or  generation.commit
  pointer.json names G0 or the committed generation
```

Julia's heap holds `AxisAligned` methods and scratch rectangles. The world is the pointer plus content-addressed evidence.

## 4. IR shape

`wge.semantic-ir/v0`: `schema`, `registry_digest`, `module_id`, `declarations[]`. Each declaration has `binding`, `constructor` (`lane` or `place`), `id`, integer `footprint` `{x0,x1,y0,y1}`, and `span` `{line,column,end_line,end_column}`. Unknown fields and constructors fail. Digest is SHA-256 of canonical JSON.

## 5. Generation shape

`wge.generation/v0` as listed in the handoff. G0 has null authoring, IR, solver, and receipt. A committed generation points at the measurement by hash. Candidate records live beside the pointer and do not replace it when rejected.

## 6. Effects implemented

`generation.propose` (author), `solver.invoke` (kernel), `generation.commit` (kernel). Host effects are recognized only so they can be rejected as the wrong class.

## 7. Job protocol

Length-prefixed JSON frames on stdin/stdout. One process serves the failing job and the passing job. See the handoff for the byte schemas. There is no `ExecuteCode` and no NeuraJL.

## 8. Solver pin

SHA-256 of worker script bytes, `Project.toml` bytes, and `Manifest.toml` bytes, each prefixed by its length. Ready-frame script hash must match the kernel's own hash of the script. No sysimage.

## 9. Receipts

Minted only in Rust after a passing predicate. `verify-current` rehashes the stored measurement bytes. Worker JSON that contains `gate_passed` or `receipt` is an unknown field and is not stored as authority. A receipt file the test writes by hand is refused unless its id is the generation's commit receipt.

## 10. Repair authorization

The legal class is `move_placement_off_lane`. Rust locates the `blocked_keep = place(...)` line in the original and edited sources and requires every other byte to match. The failing span is line 5 of `invalid.wge`. The kernel does not choose the new coordinates.

## 11. Demo command

From `WGE/`:

```sh
python3 pipeline/wge_kernel_demo.py
```

Tests:

```sh
python3 -m unittest tests.test_semantic_kernel
```

## 12. Test results

Checkpoint commands on 2026-09-25, all exit 0:

- `python3 -m unittest discover -s tests -v` from `WGE/`: 640 passed, 234.615 s, none skipped.
- `cargo test --workspace --offline` from `world_core/`: viewer 22, worldspec 24, `wge-semantic-kernel` 6. None ignored.
- `julia --project=. --startup-file=no test/runtests.jl` from `terrain_lab/`: 8 passed.
- `python3 pipeline/wge_kernel_demo.py`: one fresh store, exit 0, wall clock 1.49 s.

That demo committed `G-0a6a9aa426ead34a` with measurement `sha256:111e05ce547ad67fa68d69c5d8ff2f2b44a2bddfc3ed9c26080cdd534b1a6bb5` and IR `sha256:bbe9a08e60f450f3e73d1ea76ddf484b69fe52271ce19462caa7227ac10f5e20`. The failing measurement was `sha256:54b44a3d10b2d43e441ae8933dfa1d6a915b6bd2625d11c148296bdf49a91d56` (area 600). Those three digests matched the earlier Goal 001 pair of runs. `tests/test_semantic_kernel.py` is inside the 640 and still covers checks A–R.

## 13. Cold startup

`cold_ms` in the demo is process start until the worker's ready frame. On this checkpoint run that was **646 ms**, because the Julia depot had already compiled JSON3 and SHA.

The first overlap job in that same process was **489402 µs**. That is first-call compilation of `measure`, not the rectangle math.

A cold-depot start was observed once during Goal 001 implementation: **22549 ms** to ready and about **517 ms** for the first job. It was not repeated at checkpoint time. No sysimage is in the repository.

## 14. Warm job

The second job on process `826743` was **240 µs**. Both jobs used `lane_overlap` on that pid. Protocol overhead is inside that figure.

## 15. Nondeterminism

None observed on the certifying path. This checkpoint demo reproduced the same measurement hash, IR hash, candidate ids, repair id, receipt id, and generation id as the Goal 001 pair of runs. Coordinates are integers. The worker is single-threaded and has no RNG. The pid is not inside those hashes.

## 16. Machinery that fought this

- The 0.6 kernel stops before typed IR and keeps expression trees as dicts. Spans exist on assignments, so the specimen uses assignments. Bare expression statements would have lost the pen.
- `WorldBuilderError.suggestion` is a free string. Using it as a `SemanticRepair` would not bind a span, a gate, or a measurement hash.
- `build_zone.py` starts a new `julia` for every terrain analysis. Calling that warm would have been false. It was left alone.
- `worldspec` identity is a ZoneSpec fingerprint, not a generation pointer.
- The first job after ready still compiles `measure`. A warm process does not remove first-call compilation.

None of these blocked the transaction. They are why the spike is a new crate instead of a patch to the terrain DSL.

## 17. Debt left behind

- Vocabulary is one lane, one place, one gate. Multiline declarations are rejected.
- Span equality is "delete the declaration line and compare the rest," which is enough for this file and brittle for wrapped calls.
- No PackageCompiler image. Cold depot startup is about 22 s; warm-depot startup is about 0.7 s; first job is about 0.5 s.
- Python still lowers IR. Rust re-validates it, but the lowerer is a second place that must stay in lockstep with the registry file.
- Determinism classes other than `exact` are a field, not a taxonomy.
- Terrain, navmesh, and D29 are unchanged.

## 18. Falsification

The thesis that held: a model-facing document can be data; a pinned Julia worker can compute the geometry; Rust can own the predicate, the pointer, and the receipt; a failed candidate does not become the world; an edit outside the repair span is rejected; repeating the certified path reproduces the hashes.

What was not claimed, and is not solved: Julia startup is cheap. It is not. That is an engineering cost, not an authority leak. No test showed the worker moving the pointer, forging a receipt the kernel accepted, or registering an operation from a payload.

## 19. Next experiment (not implemented)

Add a second pinned operation id on the same warm worker, still with no author-supplied source. The specimen can stay rectangles. The new gate should be a different repair class, for example "lane width below a declared minimum," so a repair object has to choose between two legal pens instead of always pointing at `blocked_keep`. Do not start from D29. Do not build the sysimage until that second op exists; otherwise the image will pin a one-function worker and teach the wrong freeze.
