# Spiral 1 Disposition — TetCageRT weight-savings potential

Recorded retroactively against `rpd-sop.md` (the run predates the SOP copy
landing here). Substance conformed; process metadata is recorded here.

```text
Owner:    Buffy
Repo:     WGE
Layer:    Lab (graphics_lab/tet-lab — quarantined research lane)
Mode:     Meso (one subsystem question: what are the weight-savings
          economics of TetCageRT, and is it promotable, for what?)
Trigger:  Matt: "develop the weight-savings potential of AMD's
          TetCage research" (2026-09-30)
```

## Disposition: LAB (+ PARK for integration)

## Artifact map (SOP vocabulary → delivered files)

| SOP artifact | Delivered as |
|---|---|
| Evidence | `references/paper.txt` + pin (SHA-256), deterministic model `tetcage_weight_model.jl`, `results/weight-model-sweep.csv` (hashed, reruns bit-identical) |
| Interpretation | `tetcage-rt-research-handoff.md` — fact/inference/hypothesis ledger; the ρ_∞ = 0.309·(T/tets) law |
| Parking-lot note | `tetcage-rt-integration-contract.md` (DESIGN ONLY; adoption precondition = mega-sprint §17 entry gate, i.e. the SOP's closed-contract requirement) |
| Experiment design | `tetcage-rt-benchmark.md` phases 1–3 |
| Defect/non-goal record | `tetcage-rt-failure-register.md` F1–F7; verdict PROMOTE ONLY FOR / DO NOT PROMOTE FOR |
| Remaining questions | handoff §8 → seeds Spiral 2 |

## Load-bearing questions — dispositions

1. What exactly is the technique? — **closed** (paper pinned and reconstructed).
2. What are the memory/update savings? — **closed** (law + anchors; absolute-MB
   bounded by F1, ratios anchored).
3. Is it promotable? — **closed**: PROMOTE ONLY FOR dense uniquely-animated
   deforming geometry ≥ ~80 tris/tet; DO NOT PROMOTE FOR heroes/destructibles/
   rigid/watertight-requiring (F2).
4. How would it enter WGE without violating authority? — **parked** (contract
   design; no canonical code touched — quarantine verified by git status).

## Process defects of this spiral (for the ledger)

- Owner/repo/layer not named in the first reply (this file remedies it).
- Codex's parallel mandate was adopted without re-checking it against the
  exit gate; caught in Critic and registered as F6.

## Spiral 2 seed (per Law 10/11 — not an invitation to keep talking)

**Meso spiral: TetCageRT update-loop economics.** Load-bearing questions at
kickoff: (a) theoretical floor of c (bytes/tet/copy) and the ρ_∞ it implies;
(b) where the 3.575 ns/tet of Table 3 actually goes (skin vs instance write
vs tetLAS rebuild); (c) resolve F5's 2× cross-source band. Same rules: no
optimization without the law justifying it; no promotion claim; Lab layer.
