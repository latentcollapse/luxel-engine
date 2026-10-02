# WGE Friction Ledger

Status: **active evidence ledger**, initialized 2026-09-30  
Purpose: record repeated model/operator friction and the evidence needed to turn it into better WGE machinery

This is not a task backlog. Tether owns task status, dependencies, assignment, and integration state. This ledger records observations, hypotheses, before/after evidence, and whether a friction fix earned promotion.

## Operating rule

Record friction when:

- an agent must search source code to use a normal capability;
- a model must remember internal file paths or backend names;
- the same documentation has to be rediscovered repeatedly;
- the same invalid request occurs twice;
- an error identifies failure but not the next legal action;
- three or more mechanical calls always form one conceptual operation;
- receipts, hashes, and artifacts must be correlated manually;
- restart/rebuild choreography is repeated manually;
- a small semantic edit triggers an unnecessarily expensive rebuild;
- tool output is too large or omits the context needed for the next decision;
- the same reasoning procedure appears across two materially different projects.

The last signal is especially important: repeated reasoning should migrate downward into typed machinery.

## Record schema

The eventual machine-readable ledger should carry fields equivalent to:

```yaml
friction_id: FR-0001
status: candidate | observed | reproduced | fixed | verified | rejected | deferred
first_seen: 2026-09-30
last_seen: 2026-09-30
campaign: C0-C8 or cross-cutting
operation: semantic operation or construction task
symptom: concise description
repeated_behavior: what the model/operator had to do repeatedly
responsible_layer: surface | contract | docs | orchestration | runtime | provider | backend
evidence:
  runs: []
  artifacts: []
  logs: []
workaround: current workaround, if any
proposed_improvement: bounded change
expected_leverage: discovery | diagnosis | repair | latency | context | reliability
authority_impact: none | review-required
before_metrics: {}
after_metrics: {}
tests: []
tether_task: optional task id
decision: keep | reject | defer
```

## Initial candidate observations

These are starting hypotheses from the current roadmap and must be reproduced before being treated as measured defects.

| ID | Candidate friction | Evidence needed | Likely layer |
| --- | --- | --- | --- |
| FR-0001 | Fresh agents need source-tree archaeology to discover normal construction operations | clean-room C1 smoke trace | semantic surface/docs |
| FR-0002 | Capability names, provider boundaries, and validator ownership are not yet exposed as one discoverable registry | registry inventory and model trace | capability registry |
| FR-0003 | Scene, asset, world, gameplay, and graphics identities require manual correlation | real imported-asset slice trace | scene/object contract |
| FR-0004 | Live runtime choreography is not yet one semantic operation | native session trace | runtime surface |
| FR-0005 | Visual evidence does not yet independently judge all grounding/lighting/atmosphere axes | Campaign 4 evidence comparison | visual authority |
| FR-0006 | Provider jobs may require manual coordination between WGE intent and Blender output | bounded provider job trace | provider surface |
| FR-0007 | Repeated receipt/replay/rebuild steps are expensive or context-heavy | C8 construction trace | orchestration/runtime |

## Reproduced observations

### FR-0008 — cold graphics work must not gate semantic discovery

```yaml
friction_id: FR-0008
status: reproduced
first_seen: 2026-09-30
last_seen: 2026-09-30
campaign: C0/C3/C4
operation: default native repository gate
symptom: cold Julia/Lava/Vulkan integration tests dominate the default gate wall time
repeated_behavior: each graphics subprocess recompiles/initializes a worker before a small packet or window assertion
responsible_layer: runtime | orchestration | backend
workaround: split the default semantic gate from explicit --gpu certification mode
proposed_improvement: persistent Julia worker, sysimage/precompile, and a measured fast-path for repeated packets
expected_leverage: latency
authority_impact: none
before_metrics:
  default_gate: "interrupted after cold graphics suites; individual tests measured 142.16s, 510.84s, and 82.97s"
  failure_mode: "correctness remained green, but the command was not a practical fresh-agent first gate"
after_metrics:
  default_gate_seconds: 96.75
  default_gate_result: pass
  gpu_tests: "still available through python3 pipeline/wge_native_gate.py --gpu"
tests:
  - python3 pipeline/wge_native_gate.py
  - python3 pipeline/wge_native_gate.py --fences-only
decision: keep
```

The split preserves the expensive tests; it does not weaken or relabel them.
The next performance campaign must reduce the cold path rather than making the
GPU evidence optional in a final certification run.

## Evidence rules

1. A candidate is not a defect until a run reproduces it or the design review establishes a contract violation.
2. A proposed fix must state what authority, determinism, provenance, or scope boundary it preserves.
3. A fix is not verified by “the model liked it.” It needs a before/after trace, relevant tests, and a replay or deterministic result where applicable.
4. Repeated local friction may be fixed in the active slice. Cross-system friction becomes a Tether task.
5. Rejected or deferred friction remains recorded with the reason.

## AI-native acceptance checklist

For every new capability or front-door operation, ask:

- Can a fresh model discover it without source archaeology?
- Can it inspect the input/output schema and current status?
- Can it compose the operation with neighboring capabilities?
- Does failure identify the responsible semantic layer?
- Does failure suggest legal repairs without pretending unsupported work succeeded?
- Does normal use avoid backend jargon?
- Can the operation be replayed and independently verified?
- Is the context footprint small enough for an agent to use repeatedly?
- Are provenance and identity carried automatically?
