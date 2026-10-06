# WGE Autonomous Game-Construction Forensics

Status: **research design**, 2026-09-30  
Owner: WGE research / construction-harness track  
Scope: extracting reusable construction machinery from Astra-class autonomous game and 3D production workflows

This document defines how WGE studies frontier-model game construction without copying a particular model, engine, provider, or public demonstration. It is a methodology document, not evidence that any external claim has been independently verified.

Related WGE sources:

- [WGE Generality Bridge](../../content-sdk/generality-bridge.md)
- [Game Dev Harness Roadmap](game-dev-harness-roadmap.md)
- [WGE Graphics Campaign 2 Report](../2026-10_graphics-sprint-reports/graphics-campaign-2-report.md)
- [WGE Gameplay Kit Architecture](../../gameplay/gameplay-kit-architecture.md)
- [WGE Native Graphics Architecture](../../platform/native-graphics-architecture.md)

## Executive thesis

The useful question is not:

> How did a frontier model build an entire general-purpose game engine in one shot?

The useful question is:

> Which capabilities did the model inherit from its substrate, which machinery did it repeatedly construct, and which of those repeated procedures should become WGE infrastructure?

The working model is:

```text
brief / references
    -> model interpretation
    -> capability selection and provider orchestration
    -> game-specific systems
    -> deterministic build / run / inspect loop
    -> evidence-driven repair
    -> certified snapshot
```

This implies that WGE does not need to reproduce every host tool or secretly emulate a full commercial engine before it becomes useful. It needs to make the recurring construction procedures explicit, typed, inspectable, and cheaper for the next model.

## Research boundary

This campaign studies:

- one-shot and short-horizon game construction;
- 3D asset preparation and production workflows;
- style inference and style application;
- procedural content, LOD, worker execution, and deterministic generation;
- testing, visual review, playtesting, and repair;
- the boundary between reusable substrate and project-specific code.

It does not:

- treat a public demo, marketing claim, or narrated workflow as implementation evidence;
- assume that a reported rig, renderer, or game system generalizes from one example;
- make an external engine the WGE runtime or semantic authority;
- turn model-generated code into WGE canonical state;
- replace WGE's Rust authority, Julia/Lava packet boundary, or receipt discipline;
- begin broad feature implementation before repeated-task evidence identifies a stable abstraction.

## Evidence discipline

Every case study receives an evidence grade.

| Grade | Available evidence | Permitted use |
| --- | --- | --- |
| A | Frozen source, prompt, assets, dependencies, tests, tool trace, and runnable build | Forensic reconstruction and quantitative comparison |
| B | Runnable artifact plus partial source or reproducible build instructions | Behavioral reconstruction with explicit unknowns |
| C | Screenshots, video, report, or narrated claim without a complete artifact | Hypothesis generation only |
| D | Repeated secondary description with no inspectable artifact | Background context; never a WGE requirement |

Public Astra-class examples in the research notes begin as hypotheses until the exact source, version, prompt, asset set, and test harness are pinned. A strong result is still valuable at Grade C, but it must remain labelled as an observation or inference rather than a fact about the underlying implementation.

For every claim, record:

- `claim_id`;
- exact source and retrieval date;
- evidence grade;
- direct observation;
- inference drawn from the observation;
- competing explanations;
- confidence;
- what experiment would discriminate between them.

This keeps a compelling output from turning into an undocumented architecture decision.

## Subsystem-origin taxonomy

The central forensic operation is to classify every meaningful subsystem by where its capability came from.

| Class | Meaning | Example questions |
| --- | --- | --- |
| `INHERITED_SUBSTRATE` | Capability supplied by the host stack before the model acts | Did Three.js, Vite, Blender, browser APIs, or a library already provide this? |
| `EXTERNAL_PROVIDER` | Capability supplied by a separate tool or service invoked by the workflow | Did a model, rigging package, asset generator, or DCC perform the hard step? |
| `MODEL_GENERATED_REUSABLE` | Machinery created by the model that could plausibly serve another project | Is this a general terrain worker, input layer, animation helper, or test harness? |
| `PROJECT_SPECIFIC` | Logic whose semantics belong to this particular game | Is this a quest, enemy rule, level layout, weapon, or bespoke interaction? |
| `HARNESS_INSPECTION` | Machinery used to build, run, inspect, test, or repair the project | Can another model use this to observe and correct its work? |
| `MANUAL_OR_UNKNOWN` | Human intervention, hidden scaffold, or unresolved origin | What must be reproduced before this can influence WGE design? |

No item may be classified as reusable merely because it has a convenient name. It needs a second-task test or a compelling substrate-independent contract.

## Forensic case record

Each case should have a machine-readable manifest eventually, with a Markdown report for interpretation. The minimum record is:

```yaml
case_id: example-one-shot-001
subject: short descriptive name
evidence_grade: A
retrieved_at: 2026-09-30
source_artifacts:
  prompt: sha256:...
  source_tree: sha256:...
  assets: sha256:...
  dependency_lock: sha256:...
  build: sha256:...
  tests: sha256:...
environment:
  model: pinned identifier or unknown
  tool_surface: pinned description
  host: pinned hardware/software
  network_and_providers: pinned or unknown
construction_trace:
  tool_calls: preserved or unknown
  checkpoints: preserved or unknown
  repair_iterations: count or unknown
subsystems: []
claims: []
unknowns: []
candidate_wge_capabilities: []
```

For each subsystem:

```yaml
id: terrain-generation
origin: INHERITED_SUBSTRATE | EXTERNAL_PROVIDER | MODEL_GENERATED_REUSABLE | PROJECT_SPECIFIC | HARNESS_INSPECTION | MANUAL_OR_UNKNOWN
evidence: [claim-id, file-path, test-id]
inputs: []
outputs: []
determinism: deterministic | bounded-nondeterministic | unknown
reused_across_cases: []
failure_modes: []
promotion_candidate: true
```

## Research tracks

### Track A — one-shot game archaeology

Reconstruct the complete construction stack for source-complete examples.

Inventory:

- renderer and presentation path;
- input and time model;
- physics and collision;
- gameplay/state systems;
- procedural content and generation workers;
- LOD, culling, streaming, and memory policy;
- assets and external providers;
- AI/NPC systems;
- tests and deterministic math checks;
- browser or desktop automation;
- visual review and repair;
- persistence, packaging, and handoff.

For each item, separate host-provided machinery from model-authored machinery. Pay special attention to the “small mini-engine” that often appears inside a project: it is a likely WGE capability candidate, but only after it survives the reuse test.

### Track B — 3D production archaeology

Follow assets from reference to runtime rather than judging the final image alone.

Record:

1. source geometry and topology;
2. UVs and texture sources;
3. material construction and texture conditioning;
4. hierarchy and pivots;
5. skeleton/profile choice;
6. bind and weight generation;
7. deformation probes;
8. animation/action construction;
9. retargeting or export;
10. runtime import and validation.

The important question is not whether a model “can rig.” It is which parts were selected, scripted, delegated, tested, and repaired. A Blender provider that performs automatic weights is evidence of a useful provider seam, not evidence that WGE should make Blender canonical.

### Track C — style decomposition

Give a construction system several substantially different visual targets and inspect what changes. Record whether each change occurs in:

- silhouette and proportions;
- mesh density, bevels, and faceting;
- texture frequency and conditioning;
- base color, roughness, metallic, normal, and emissive channels;
- lighting and shadow policy;
- environment and atmosphere;
- population density and asset selection;
- camera and exposure;
- animation timing;
- VFX and UI.

The output is not a list of named styles. It is a vocabulary of observable policies that can be represented by a typed `StyleProfile` and lowered into capabilities.

### Track D — repeated-task extraction

Construct several games or scenes with materially different briefs. Diff the workflows, not only the source trees.

Look for repeated:

- tool calls;
- repair scripts;
- asset preparation steps;
- procedural algorithms;
- runtime probes;
- camera/capture setup;
- material assignments;
- LOD and worker patterns;
- failure diagnoses;
- evidence formats.

Anything that recurs independently is stronger evidence than a polished one-off implementation. Anything that recurs but has incompatible semantics may need a family of capabilities rather than one universal abstraction.

### Track E — WGE transfer test

Run the same model, brief, references, inputs, and acceptance criteria through:

```text
A: ordinary model-accessible graphics/game tooling
B: WGE's typed construction and inspection surface
```

This is a construction-leverage experiment, not a Unity comparison and not a claim that WGE must imitate the ordinary stack. The question is how much repeated work WGE eliminates while preserving or improving correctness, quality, provenance, and recovery.

## Standard procedure

1. **Acquire and freeze.** Pin the source, prompt, assets, dependencies, tool versions, model identifier when available, and output artifacts.
2. **Rebuild independently.** Start from a clean environment. Record every missing dependency, undocumented manual action, and nondeterministic step.
3. **Inventory the substrate.** Identify what existed before the model's first action.
4. **Trace construction.** Capture tool calls, file edits, checkpoints, tests, visual inspections, and repairs.
5. **Classify origin.** Assign every subsystem an origin class and evidence grade.
6. **Probe reuse.** Apply candidate reusable machinery to a second task without copying project-specific semantics.
7. **Measure failure.** Inject or reproduce failures at the semantic, asset, runtime, visual, and packaging layers.
8. **Extract the contract.** Describe inputs, outputs, preconditions, identity, determinism, cost, evidence, and repair behavior.
9. **Compare with WGE.** Determine whether the candidate is already covered, partially covered, or genuinely missing.
10. **Promote cautiously.** Only a repeated, independently validated procedure becomes a WGE capability candidate.

## Promotion rule for extracted machinery

A finding may become a WGE subsystem requirement only when:

- it appears in at least two independent tasks, or one task exposes a clearly reusable provider-independent contract;
- the semantic inputs and outputs are identifiable;
- canonical identity can be owned by Rust;
- execution can occur behind a typed packet or provider boundary;
- success and failure are independently validated;
- determinism is guaranteed or the nondeterministic envelope is explicit;
- a repair procedure can be bounded;
- the capability does not smuggle backend representation into WGE state.

The default outcome for an interesting one-off is “research observation,” not “new engine feature.”

## First excavation campaign

The first campaign should use:

1. one source-complete browser game case;
2. one source-complete 3D asset/animation case;
3. one procedural-world case;
4. one deliberately style-divergent reconstruction;
5. one WGE implementation of the same small vertical slice.

The initial deliverables are:

- one case manifest per artifact;
- subsystem-origin maps;
- construction traces and repair ledgers;
- a recurrence matrix;
- a list of proposed capabilities with evidence grades;
- a WGE gap map tied to existing contracts;
- a benchmark-ready task and acceptance bundle.

## Failure modes this research must prevent

- selecting a demo because it looks impressive while ignoring hidden scaffolding;
- confusing provider output with model reasoning;
- treating a one-off generated utility as a reusable engine subsystem;
- inferring universal auto-rigging from one successful character;
- encoding a style as a brand label with no measurable visual semantics;
- measuring only final screenshots and not repair cost or determinism;
- comparing different prompts, models, time budgets, or providers;
- allowing a visual critic to promote its own suggestions;
- importing a backend object as canonical WGE state;
- adding breadth before the recurring construction bottleneck is known.

## Expected research conclusion

The desired outcome is not “WGE copied Astra.” It is a map of the smallest set of typed, reusable construction capabilities that lets future models spend their intelligence on game design rather than repeatedly rebuilding terrain workers, asset import scripts, style plumbing, test harnesses, and repair loops.

