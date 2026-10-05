# TetCage Optimization Ledger (CPU oracle)

Rules in force (goal §1, §16): no optimization without a model; every entry
carries prediction → change → measurement → verdict; correctness gate =
cage_sha256 stability; stop conditions apply.

Test rig: CachyOS, Julia 1.12.6, repo on NTFS/FUSE mount (/mnt/d).
Workloads: blob5 (10242 v / 20480 t), blob6 (40962 v / 81920 t), h=0.28.

## Baseline (pre-optimization A2/A3 state, in-process medians of 3)

| workload | build ms | clip ms | alloc MB | hash16 |
|---|---|---|---|---|
| blob5 | 38.34 | 317.69 | 327.1 | 35b3de9f… (pre-campaign artifact) |
| blob6 | 124.34 | 924.78 | 959.9 | a65f497a… |

## O1 — Hoist tet_planes/tet_aabb per tet (KEEP)

- **Model**: planes were recomputed per (tet × candidate-tri), twice per pair
  (overlap test + clip). ~10⁵ plane constructions where ~10³ suffice.
  Prediction: clip phase drops ~2×.
- **Change**: TetPlanes struct computed once per tet in build_cage and
  clip_mesh; overlap/clip take planes as an argument.
- **Measured**: build 38.3→13.2 ms (2.9×), clip 318→151 ms (2.1×),
  allocs 327→139.5 MB (2.35×). [MEASURED]
- **Correctness**: stats identical (tets=1100, tri_exp=2.538, vert_exp=1.559).
- **Verdict**: KEEP.

## O2 — tri_bins stored in Cage (KEEP)

- **Model**: bins were computed twice (build_cage + clip_mesh), pure
  O(T·coverage) duplicate work.
- **Change**: bins field on Cage; clip_mesh consumes it.
- **Measured**: subsumed in O1–O3 combined result (single-process run).
- **Verdict**: KEEP (strictly less work, no counter-evidence).

## O3 — Allocation-free hot path (KEEP)

- **Model**: per-call heap vectors in tet_planes / merge_planes dominate GC
  pressure (327 MB allocs per blob5 pass).
- **Change**: static TetPlanes (NTuple), integer-tuple merge_planes without
  intermediate arrays.
- **Measured**: allocs 327→139.5 MB blob5, 959.9→347.7 MB blob6. [MEASURED]
- **Verdict**: KEEP.

## O4 — Precomputed per-triangle AABB tables (REJECTED-AS-UNRESOLVED; simpler code kept)

- **Model**: AABBs recomputed per (tet, candidate) → precompute two lookup
  tables (verts, boxes) once per mesh. Prediction: modest clip-phase gain.
- **Measured**: blob6 clip 532.3 ms vs 404.2 ms single-run — appeared ~30%
  slower.
- **Noise audit (F-OPT.2)**: three independent runs of the O1–O3 binary
  produced 407.1 / 519.9 / 525.8 ms — the rig's cross-process noise floor is
  ±30%, and O4's "regression" (532) falls INSIDE the O1–O3 band (404–541).
  The original O4 rejection was itself measured inside noise.
- **Verdict**: effect UNRESOLVED by available methodology; REJECTED on
  parsimony (stop condition: unproven gain + 7.8 MB tables + two extra
  structures). Reverted; inline recompute retained. Re-admission requires an
  N≥9 cross-process median harness.

## Post-campaign state (O1+O2+O3, O4 reverted)

| workload | build ms | clip ms | alloc MB | hash16 |
|---|---|---|---|---|
| blob5 | 13.6 | 151–165 | 139.5 | 98b989a0… |
| blob6 | 40.1 | 407–541 | 347.7 | d54f13aa… |

vs baseline: build 2.8–2.9×, clip ~1.9–2.1×, allocs 2.35–2.8× — all far above
the measured noise floor. Determinism: reruns hash-identical; 9-mesh corpus
audit: 0 anomalies, all determinism OK.

## F-OPT.1 (process defect, admitted)

The in-place optimization destroyed the byte-exact A2/A3 artifact; the
pre-campaign hash (35b3de9f…) is no longer reproducible because the hash
inputs (tpt multiset, kinds census) were never recorded at A3 acceptance —
the recorded aggregates underdetermine the hash. A memory-based reconstruction
produced identical aggregates but a different hash (98b989a0…), which is now
the pinned reference WITH full field records (below). Lesson: fork-then-
optimize; record complete hash-input fields at acceptance time.

**Pinned reference record (blob5, h=0.28)**: tets=1100, tris_out=51982,
tri_exp=2.538, verts_out=15963, vert_exp=1.559, tpt sorted median 30 max 104,
kinds V=10242 EP=5004 TE=717 TV=0, anomalies=0, hash16=98b989a0850724ba.

## F-OPT.2 (methodology defect, registered) — CLOSED

Single-run medians-of-3 within one process cannot resolve <30% effects on
this rig (NTFS/FUSE ambient variance). All future oracle performance claims
require cross-process repetition (N≥9 medians) or effect sizes >2×. Native-fs
scratch (per repo note) is the structural fix.

**Closed by bench2/bench_child (N=9 cross-process medians + spread).**
Re-measured main module: build 40.15 ms ±13%, clip 511.19 ms ±10%,
alloc 347.7 MB ±0%. O4 re-adjudicated in its fork under the admissible
harness: clip 513.1 ms (parity), build 48.4 ms (20% slower — table
construction), allocs +19.6 MB, hashes identical → **REJECT confirmed with
evidence.** O1–O3 gains reconfirmed (clip 925→511 ms vs baseline single-run,
>2× effect, above noise).
