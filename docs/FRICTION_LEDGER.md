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

### FR-0009 — Lava window-loop predicate is a throwing assertion

```yaml
friction_id: FR-0009
status: reproduced
first_seen: 2026-10-02
last_seen: 2026-10-02
campaign: C3
operation: continuous presented-frame loop
symptom: "`Lava.checkopen(win) || break` exits the loop on every iteration because `checkopen` returns nothing and throws only on a destroyed handle — it is an assertion, not a predicate"
repeated_behavior: an agent writing the natural loop guard silently presents zero frames; the error mode is an empty result, not an exception
responsible_layer: provider
workaround: loop on `Base.isopen(win)` (handle alive and no close request); adapter now encodes this in `render_window_frames!` with a comment at the call site
proposed_improvement: Lava could name the predicate (`isopen`) and the assertion differently so autocomplete and reading the signature disambiguate them; WGE-side, the presented-session handoff documents the distinction for the pinned Lava revision
expected_leverage: diagnosis
authority_impact: none
before_metrics:
  frames_presented: 0
after_metrics:
  frames_presented: 3
  probe: /tmp/c3_window_probe.jl (checkopen→isopen; window session open→present→close)
tests:
  - graphics_lab/test/lava_adapter.jl
  - world_core native_graphics_contract::presented_session (both tests)
decision: keep
```

### FR-0010 — offscreen/window `draw!` kwarg asymmetry fails as MethodError

```yaml
friction_id: FR-0010
status: reproduced
first_seen: 2026-10-02
last_seen: 2026-10-02
campaign: C3
operation: drawing to a presented window target
symptom: "`draw!` to a `WindowTarget` rejects `descriptor_set_layout`/`descriptor_set`/`depth_clear`/`depth` kwargs with a MethodError that names neither the offscreen/window asymmetry nor the next legal action (window targets have no depth attachment and no descriptor sets)"
repeated_behavior: an agent reusing the working offscreen resolve draw against the window hits a MethodError and must read Lava's `graphics/api.jl` signatures to learn the boundary
responsible_layer: provider
workaround: the presented path composites into the capture framebuffer (full descriptor/depth machinery) and reaches the swapchain through `Lava.blit!` of a window-sized buffer — the window never needs a descriptor-bearing draw
proposed_improvement: Lava could give the window-target method a clear "no descriptors/depth on window targets" error; the C3 handoff records the asymmetry for the pinned revision
expected_leverage: diagnosis
authority_impact: none
before_metrics:
  failure_mode: MethodError drawing resolve directly to WindowTarget
after_metrics:
  presented_frames: "composite→readback→upload→blit→present path presents frames and the composite is byte-identical to the certified capture source"
tests:
  - world_core native_graphics_contract::presented_session_presents_frames_and_samples_tier_a_evidence
decision: keep
```

### FR-0011 — mid-batch readback silently invalidates the pending present

```yaml
friction_id: FR-0011
status: reproduced
first_seen: 2026-10-02
last_seen: 2026-10-02
campaign: C3
operation: sampling presented-frame evidence without breaking the frame
symptom: "a readback recorded mid-frame flushes the active batch (`flush!` sets `bq.active_batch = nothing`), so the subsequent `present_frame!` throws `called without an active recording batch`; the error names the symptom but not the ordering rule"
repeated_behavior: an agent naturally tries to read back the presented image for evidence and breaks the batch the present needs
responsible_layer: provider
workaround: presented frames carry no capture; Tier-A evidence is sampled through the independent offscreen promotion path (`capture_and_promote`), and the window loop reads back only the offscreen capture framebuffer before opening the present batch — `present_frame!` is the submit that adds the acquire-semaphore wait, so that ordering is safe
proposed_improvement: Lava could document the readback/flush/present ordering invariant next to `readback_window`'s warning; the C3 handoff records the safe order for the pinned revision
expected_leverage: reliability
authority_impact: none
before_metrics:
  failure_mode: present_frame! without an active recording batch
after_metrics:
  evidence_model: "live frames never carry un-promoted evidence; Tier-A stays Rust-owned through the offscreen authority path"
tests:
  - world_core native_graphics_contract::presented_session_presents_frames_and_samples_tier_a_evidence
decision: keep
```

### FR-0012 — `cargo test` accepts one positional filter for long GPU suites

```yaml
friction_id: FR-0012
status: reproduced
first_seen: 2026-10-02
last_seen: 2026-10-02
campaign: cross-cutting
operation: running the native graphics certification ladder
symptom: "`cargo test -p wge-native-graphics-contract <filter> <second-filter>` is rejected (one positional filter); the serial `native_graphics` suite exceeds 10 minutes, so an unfiltered run times out and leaves no per-test evidence"
repeated_behavior: every graphics verification session rediscovers the one-filter limit and the per-test serial invocation pattern
responsible_layer: orchestration
workaround: one positional filter plus repeated `--skip`, or `--exact` per test with `--test-threads=1`; package is `wge-native-graphics-contract` (crate path `native_graphics_contract`)
proposed_improvement: encode the full ladder (session, presented_session, native_graphics per-test, lib, fmt, clippy) as one gate script so fresh agents run one command
expected_leverage: latency | context
authority_impact: none
before_metrics:
  native_graphics_serial: ">600s (timed out, no result line)"
after_metrics:
  per_test: "lower 6.5s–275s each, all 7 green; lib 19/19; session 5/5; presented_session 2/2"
tests:
  - cargo test -p wge-native-graphics-contract --test native_graphics -- <test> --exact
decision: keep
```

### FR-0013 — parallel Rust tests race shared temp-fixture writes

```yaml
friction_id: FR-0013
status: verified
first_seen: 2026-10-02
last_seen: 2026-10-02
campaign: C3
operation: session contract tests sharing a world fixture
symptom: "tests copying the same layout file into a per-process temp dir raced (`std::fs::copy` truncates while another thread's Julia reads), surfacing `EOF while parsing a value` on a *different* test each run"
repeated_behavior: flaky failures with a misleading parse error pointing at the layout, not the race
responsible_layer: contract
workaround: build the world once per process behind a `OnceLock` and hand each test a clone (WorldArtifact is Clone)
proposed_improvement: prefer process-shared immutable fixtures for any test whose fixture crosses a subprocess boundary
expected_leverage: reliability
authority_impact: none
before_metrics:
  flaky_runs: "2/5 then 3/5 passing, different tests each time"
after_metrics:
  session_tests: "5/5 stable across repeated runs"
tests:
  - cargo test -p wge-native-graphics-contract --test session
decision: keep
```

### FR-0014 — Julia test gate silently requires an explicit test target on Julia 1.12

```yaml
friction_id: FR-0014
status: verified
first_seen: 2026-10-02
last_seen: 2026-10-02
campaign: pre-demo audit
operation: `Pkg.test()` for the graphics_lab Julia package
symptom: "`Pkg.test(test_args=[\"lava_adapter\"])` aborted with `ArgumentError: Package Test not found in current path.`; Project.toml declared no [extras]/[targets], and Julia 1.12 no longer loads stdlibs like Test implicitly in the temporary test environment"
repeated_behavior: every test invocation failed at environment resolution before any test ran; the C3-closure "runtests.jl exit 0" claim was not reproducible on the current toolchain
responsible_layer: contract (package environment definition)
workaround: none needed — declare `[extras]` Test and `[targets] test = ["Test"]` in graphics_lab/Project.toml
proposed_improvement: run `Pkg.test()` as part of every Julia campaign exit gate on the pinned toolchain, not only during the slice that added the tests
expected_leverage: reliability
authority_impact: none (environment declaration only; no source or schema change)
before_metrics:
  julia_gate: "exit 1 before any test executes (Package Test not found)"
after_metrics:
  julia_gate: "exit 0; lava_adapter suite 26/26 passed"
tests:
  - julia --project=. -e 'using Pkg; Pkg.test(test_args=["lava_adapter"])'
decision: keep
```

### FR-0015 — a port's comparator pin silently loses its domain when the ladder widens

```yaml
friction_id: FR-0015
status: verified
first_seen: 2026-10-02
last_seen: 2026-10-02
campaign: TetCageRT P0 (Vulkan compute parity port)
operation: porting a verified kernel to a new backend, gated against the old backend's measured walls
symptom: "P0-G4 reported a 20.4x wall regression at blob/wind/N=128. The kernel was never slower: the wind comparator pins came from a campaign that ran no N-ladder, so every wind pin was an N=1 measurement, and the new harness applied it at all four ladder points — comparing 128x the work against a 1x pin."
repeated_behavior: an operator must re-derive, per comparator, which axes (N, family, layout, config) it was actually measured over, before any ratio gate may consume it; nothing in the pin's representation enforced that
responsible_layer: contract (comparator/pin provenance carries no machine-checkable domain)
workaround: per-row `pin_applicable` flag; the 2x gate asserts only where a same-(class,N) pin exists, and the remainder is labeled UNAVAILABLE with an explicitly-weaker same-work substitute
proposed_improvement: a comparator pin should carry its measured domain as data (axes + values), and a comparison helper should refuse or downgrade any pairing that falls outside it, rather than dividing
expected_leverage: diagnosis
authority_impact: review-required (changes what a "within 2x of the reference" claim is allowed to mean)
before_metrics:
  gate_claim: "20.4x regression attributed to the ported kernel"
  pinned_cells: "wind applied at N in {1,8,32,128} against N=1-only pins"
after_metrics:
  gate_claim: "claim asserted only where a same-(class,N) pin exists; wind N>1 labeled unavailable"
  measured: "wind and rot cost the same at every N (blob/N=128: rot 196.8 vs wind 201.6 us attributed; N=1: 1.54 vs 2.04 us)"
tests:
  - "P0-G4d/G4f substitute coverage line in TetLab/results/spiral_p0_vulkan_compute.csv"
decision: keep
```

### FR-0016 — a threshold derived from a measurement silently disabled the gate that consumed it

```yaml
friction_id: FR-0016
status: verified
first_seen: 2026-10-02
last_seen: 2026-10-02
campaign: TetCageRT P0 (Vulkan compute parity port)
operation: calibration-then-gate (measure a resolution floor, use its spread to decide which cells a wall gate may rank)
symptom: "the run printed ALL GATES PASS while the substitute wall gate covered ZERO of 21 cells. The resolution limit was the submit-floor probe's own spread; that probe had no warmup, so measured cold it reported a 656 us band against a ~14 us steady-state floor, which marked every cell UNRESOLVABLE."
repeated_behavior: an operator must check what a gate actually gated, every run, and must sanity-assert every calibration constant before trusting a threshold derived from it
responsible_layer: contract (no assertion tied calibration validity to gate coverage)
workaround: sanity assert on the calibration itself (`floor_band < 0.5 * floor`), a coverage assert (DRAM-bound cells must be resolvable), and a per-run coverage count in the artifact
proposed_improvement: a gate should emit the size of the set it gated, and refuse to report success when that set is empty or suspiciously small
expected_leverage: reliability
authority_impact: none (reporting and self-check only; no threshold loosened)
before_metrics:
  calibration: "floor band 656.48 us vs floor 13.88 us (cold, min-max over 5 trials)"
  gated_cells: "0 of 21"
after_metrics:
  calibration: "floor band 2.71-3.34 us vs floor 13.88-15.26 us (25 trials, warm, p90-p10)"
  gated_cells: "4-5 of 21 ratio-gated, remainder recorded as floor-bound and unrankable"
tests:
  - "P0-G4g sanity assert trips on an intermediate 8.55 us min-max band"
  - "P0-G4h coverage assert trips if a DRAM-bound substitute cell is unresolvable"
decision: keep
```

### FR-0017 — "stable across trials" was read as "stable across runs"

```yaml
friction_id: FR-0017
status: verified
first_seen: 2026-10-02
last_seen: 2026-10-02
campaign: TetCageRT P0 (Vulkan compute parity port)
operation: benchmarking a ported kernel at the throughput-bound corner
symptom: "the DRAM-write-bound cell (blob/N=128, soa+packed) measured 211.72, 39.10 and 214.56 us across three runs of the same binary, while its within-run trial spread stayed tight (193.79-216.42 us). A harness that takes 50 samples per run can prove steadiness and still be a sample of one machine state."
repeated_behavior: capacity claims at that corner need repeated fresh processes, not repeated trials; determinism had to be argued from the parity columns instead of the wall
responsible_layer: runtime (process-level GPU state: buffer placement or clock/power state, mechanism NOT established)
workaround: stable columns are the deterministic evidence (parity/error vectors, byte-identical x2); the wall at that corner is recorded with its min/max and carried as a band into P2
proposed_improvement: a repeatability harness should distinguish trial-level and process-level variance, and report a capacity claim only from process-level repeats
expected_leverage: reliability
authority_impact: review-required (capacity claims at that working set carry the band)
before_metrics:
  corner_wall: "single-run sample only; ordering claims treated as run-independent"
after_metrics:
  corner_wall: "3 runs spanning 39.10-214.56 us on soa+packed; other two layouts stable at ~208-214 us"
tests:
  - "sust_min_us/sust_max_us columns in TetLab/results/spiral_p0_vulkan_compute.csv"
decision: keep
```

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
