# Luxel Construction-Leverage Benchmark

Status: **benchmark design**, 2026-09-30  
Purpose: measure how much recurring model construction work Luxel removes while preserving or improving correctness, quality, provenance, and recovery

This is a controlled research benchmark. It is not a Unity comparison, a claim that ordinary Three.js/Blender tooling is inferior, or a single-number ranking of engines.

Related sources:

- [Autonomous Game-Construction Forensics](autonomous-game-construction-forensics.md)
- [Luxel Capability Registry Design](../../platform/capability-registry-design.md)
- [Luxel Style Profile Contract](../../world/style-profile-contract.md)
- [Luxel Graphics Campaign 2 Report](../2026-10_graphics-sprint-reports/graphics-campaign-2-report.md)
- [Luxel Native Graphics Benchmark](../../world/native-graphics-benchmark.md)

## Benchmark question

For the same model, task, inputs, time budget, and acceptance criteria:

> How much repeated construction work does Luxel eliminate, and what does that do to successful completion, repair cost, determinism, evidence coverage, and final quality?

The baseline is an ordinary model-accessible graphics/game environment. The treatment is Luxel's typed construction, execution, inspection, and certification surface.

## Experimental controls

Every run must pin or record:

- model identifier and reasoning setting;
- prompt and attached references;
- task seed;
- source assets and their hashes;
- tool/API surface available to the model;
- external providers and versions;
- host hardware and driver/runtime versions;
- network availability and cache state;
- wall-clock deadline;
- model-token and tool-call budget;
- manual intervention policy;
- build and test commands;
- visual capture cameras and dimensions.

If a control is unknown, the result is marked incomplete rather than normalized after the fact.

## Two environments

### Environment A — ordinary model-accessible stack

Use a clearly documented, lightweight stack appropriate to the task. A browser stack such as TypeScript/Vite/Three.js and a headless asset toolchain are possible controls, but the exact stack is not sacred. The environment must expose enough inspection and test tooling to make a fair construction experiment possible.

Record what the stack provides before the model starts:

- renderer and material system;
- asset import/export;
- physics/collision;
- input/time control;
- worker or background execution;
- LOD/culling;
- testing and capture;
- packaging.

### Environment B — Luxel

Use only the Luxel surface permitted by the current checkpoint:

- typed semantic intake;
- world/gameplay contracts;
- native graphics packet and capability registry entries;
- fixed-step runtime;
- typed asset/provider seams;
- deterministic captures and evidence;
- Rust authority and repair loop.

Unavailable Luxel capabilities remain unavailable. The benchmark must not quietly grant a feature because the baseline happens to have it.

## Task ladder

Tasks should begin small and become more general only after the harness is stable.

### T0 — authored calibration scene

Brief: a terrain surface, hero asset, foliage cluster, wet/reflective region, controlled lighting, fixed cameras, and three evidence views.

Purpose:

- validates graphics construction and style lowering;
- exercises materials, environment, camera, evidence, and repair;
- directly builds on Campaign 2 without requiring gameplay breadth.

### T1 — small playable vertical slice

Brief: one compact world, one traversal route, one interaction, one objective, one failure/repair injection, and a reproducible handoff.

Purpose:

- tests the full semantic-to-runtime loop;
- measures whether the model spends effort on game design rather than scaffolding;
- exercises mechanical, visual, traversal, and restart evidence.

### T2 — style transfer

Hold world semantics and gameplay constant while changing concept art, style constraints, and presentation policy.

Purpose:

- tests whether `StyleProfile` changes the correct layers;
- detects hard-coded scene-specific rendering;
- measures collateral damage to mechanics, identity, and determinism.

### T3 — asset and character slice

Deferred until the rigging/skinning/retargeting gates are genuinely certified. It must not be scored as complete using static meshes or status-only rig evidence.

### T4 — recovery and adversarial repair

Inject failures into semantic intake, asset preparation, packet lowering, runtime traversal, visual evidence, and packaging.

Purpose:

- measures whether the model diagnoses the right layer;
- verifies bounded repair;
- proves that failed evidence cannot be promoted by cosmetic changes.

## Metrics

### Cost and time

- total model tokens;
- total tool calls;
- tool/runtime cost where available;
- wall-clock time to first runnable artifact;
- wall-clock time to first passing artifact;
- wall-clock time to final certified handoff;
- cold-start and rebuild time.

### Completion and intervention

- successful completion rate;
- number of independent runs;
- number of repair iterations;
- number of retries and restarts;
- manual interventions;
- unresolved blockers;
- unsupported requests surfaced honestly.

### Construction leverage

- generated task-specific code volume;
- reused capability executions;
- repeated utility code recreated by the model;
- percentage of final implementation covered by registered capabilities;
- capability selection versus capability invention count;
- provider/tool invocations;
- “reinvention ratio” for recurring machinery.

### Correctness and evidence

- semantic validation coverage;
- mechanical/gameplay pass rate;
- traversal/playthrough pass rate;
- visual evidence coverage by axis;
- provenance completeness;
- deterministic rebuild/replay success;
- known-good and known-bad control behavior;
- injected-failure detection and repair success.

### Runtime and quality

- frame and GPU cost;
- upload/readback and memory cost;
- visible/total population;
- silhouette readability;
- material separation;
- grounding/contact;
- lighting consistency;
- atmospheric depth;
- texture frequency;
- composition;
- density;
- temporal stability;
- artifact rate;
- final mechanical correctness.

Do not collapse these into one score for the primary report. A secondary summary may show a dashboard, but every decision must retain the vector and its indeterminate axes.

## Run protocol

1. Freeze the task bundle, prompt, references, seed, and acceptance criteria.
2. Start both environments from clean state.
3. Give the same model the same brief and the same time/tool budget.
4. Record every model action, tool call, file mutation, checkpoint, test, capture, and repair.
5. Permit ordinary reversible repairs autonomously; record all of them.
6. Deny untracked human fixes. If manual intervention is allowed for a specific experiment, record it as a separate condition.
7. Run semantic, mechanical, traversal, visual, runtime, determinism, and packaging gates.
8. Inject the planned failure controls after the first successful build.
9. Run the repair loop and preserve before/after evidence.
10. Rebuild from the final snapshot in a fresh process.
11. Compare results using the metric vector and evidence provenance.
12. Archive the complete run manifest, logs, source snapshot, outputs, receipts, and images.

## Result manifest

Each run should eventually emit a machine-readable result equivalent to:

```yaml
schema_version: luxel.construction-leverage-result/v1
run_id: run-...
environment: ordinary-stack | luxel
task_id: T0-authored-calibration
model:
  id: pinned
  settings: pinned
inputs:
  brief_sha256: sha256:...
  references_sha256: sha256:...
  assets_sha256: sha256:...
  seed: 1234
budgets:
  wall_clock_s: 3600
  model_tokens: recorded
  tool_calls: recorded
outcome: passed | failed | indeterminate | blocked
metrics: {}
repairs: []
manual_interventions: []
evidence:
  semantic: []
  mechanical: []
  traversal: []
  visual: []
  runtime: []
  provenance: []
deterministic_replay: passed | failed | not_run
handoff_snapshot: sha256:...
```

## Failure injection matrix

The first adversarial matrix should include:

| Layer | Injection | Expected behavior |
| --- | --- | --- |
| Intake | contradictory material or camera constraints | conflict surfaced; no silent choice |
| Asset | malformed or unsupported GLB | rejected with typed reason; bad control remains bad |
| World | stale spatial-field identity | lowering rejected |
| Packet | altered material/instance payload | Rust receipt or validator rejects it |
| Runtime | blocked traversal or invalid interaction | mechanical/traversal gate fails with location |
| Visual | texture/lighting degradation | relevant visual axis fails; not downgraded to warning |
| Repair | unauthorized field mutation | repair rejected |
| Restart | worker/process death | clean restart either reproduces or returns explicit failure |
| Packaging | missing provenance artifact | handoff rejected |

The expected outcome is part of the benchmark. “The model eventually made an image” is not recovery.

## Replication and analysis

Do not draw conclusions from one run per environment. The initial study should target at least three independent runs per task/environment and expand to five when variance is high. Report:

- median and spread for time/cost metrics;
- success and failure counts;
- exact failure categories;
- repair count distribution;
- evidence coverage distribution;
- quality-vector results per view;
- deterministic replay rate;
- manual intervention count;
- confidence and known confounds.

If a provider, model, or runtime version changes, start a new benchmark cohort. Do not merge incomparable runs into a single trend line.

## Interpretation rules

Luxel has demonstrated leverage when it reduces repeated model work without reducing:

- semantic correctness;
- mechanical correctness;
- visual evidence quality;
- reproducibility;
- provenance completeness;
- failure recovery;
- explicit handling of unsupported requirements.

A lower token count with more manual intervention or weaker evidence is not a win. A prettier screenshot with weaker determinism is not a win. A higher success rate achieved by silently narrowing the brief is not a win.

## First benchmark execution

The first executable cohort should use T0 and T1:

- T0 establishes the graphics/style baseline from Campaign 2;
- T1 tests whether the construction surface helps the model build a complete small game rather than only a scene;
- T4 failure injection is applied to both after the first successful build.

The first report should answer:

1. Which work did ordinary tooling force the model to reinvent?
2. Which of that work is already covered by Luxel?
3. Which recurring work is missing from the capability registry?
4. Did Luxel improve recovery, not merely first-pass completion?
5. Which quality axes remain indeterminate in both environments?

## Promotion criteria for benchmark findings

A benchmark finding may drive implementation when:

- it recurs across independent tasks or materially blocks the same task family;
- its desired semantic interface is clear;
- the authority boundary is compatible with Luxel doctrine;
- the capability has a validator and failure behavior;
- the expected model-cost or quality benefit is measurable;
- the change does not require weakening existing gates.

The benchmark is therefore a feedback loop into the capability registry, not a race whose winner dictates architecture by screenshot appeal.

