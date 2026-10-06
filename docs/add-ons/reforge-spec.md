# WGE Reforge — Brownfield Modernization, Preservation, Refactoring, and Remastering

Status: **Semantic Stub / Agent-Executed Subsystem, registered 2026-10-02.**
Namespace `wge.reforge`. Initial implementation strategy: Cyan procedure +
existing repository / build / profiling / WGE capabilities. Development method:
**RPDO Semantic Stubbing** (register the canonical semantics and the artifact
contracts now; let dedicated machinery be *earned* through repeated use — see
§18). Maturity: **REFORGE = SEMANTIC / PROCEDURAL** (see §5 — this status must
remain visible; this document does not imply native machinery that does not
exist).

Provenance: operator-authored product specification, registered verbatim-intent
as a semantic stub. Where this condensation and the operator's full text
differ, the operator's text wins; file corrections as evidence. Authority
chain as elsewhere in WGE: the Rust kernel owns identity/policy/evidence;
Julia/Lava execute; Cyan is the executing agent; the operator is
creative/technical director.

---

## 0. Executive summary

Reforge is the semantic subsystem for ingesting an existing game client,
engine, runtime, toolchain, or other game-adjacent software system and
systematically modernizing it while preserving explicitly selected behavior.

**Reforge addresses brownfield software.** Its job is not "generate a
replacement that seems equivalent." Its job is:

```
UNDERSTAND WHAT EXISTS
    ↓
CAPTURE WHAT MUST BE PRESERVED
    ↓
IDENTIFY WHAT SHOULD CHANGE
    ↓
MIGRATE IN BOUNDED SLICES
    ↓
VERIFY CONTINUITY
    ↓
REFINE THE RESULT
```

Reforge initially exists primarily as a Semantic Stub: Cyan executes a
canonical modernization procedure using existing tools. As repeated
modernization projects reveal recurring operations, those operations should
crystallize into dedicated Reforge machinery.

## 1. Reforge vs Remaster

`Reforge` is the subsystem. `Remaster` is one operating profile within Reforge.

Reforge may support goals such as: preservation, modernization, refactoring,
performance improvement, portability, renderer replacement, remastering,
architectural cleanup, moddability, accessibility improvement, maintainability,
tooling modernization, partial WGE migration, full WGE-native migration.

A Remaster project may include modern rendering and presentation while
preserving game identity. A Reforge project may involve no visual changes at
all.

## 2. Core principles

**Existing software is evidence.** Do not treat legacy code merely as
something old to replace. It may encode undocumented gameplay semantics,
compatibility behavior, timing assumptions, protocol details, asset
conventions, animation quirks, user expectations, content assumptions, and
decades of accumulated bug fixes. Therefore:

> PRESERVE OBSERVABLE BEHAVIOR BEFORE IMPROVING IMPLEMENTATION, UNLESS
> BEHAVIORAL CHANGE IS EXPLICITLY PART OF THE GOAL.

**Second principle:** MODERNIZE THROUGH BOUNDED MIGRATIONS WITH COMPATIBILITY
EVIDENCE, NOT HEROIC REWRITES.

## 3. Initial product experience

User:

> "Reforge this client. Preserve gameplay and protocol behavior. Make it
> substantially more responsive. Multithread what is safely parallelizable.
> Add a Classic graphics mode and a Modern graphics mode that preserves the
> visual soul of the original. Modernize the architecture for long-term
> community moddability. Do not alter observable gameplay semantics without
> approval."

Cyan should not require the user to manually explain every subsystem.
Instead Cyan performs archaeology, develops a semantic model, and asks only
for decisions that cannot safely be inferred.

## 4. Initial semantic API

The first Reforge namespace exposes these operations. They initially lower to
an **agent procedure**; their semantics remain stable as dedicated machinery
grows underneath.

| Operation | Intent | Primary artifacts | Exit condition |
|---|---|---|---|
| `wge.reforge.intake` | Accept the system + goals; establish rights posture | `REFORGE_GOAL.md`, `SOURCE_MANIFEST`, `RIGHTS_AND_PROVENANCE_MANIFEST`, `INITIAL_CONSTRAINTS`, `INITIAL_OPEN_QUESTIONS` | Goals, preservation requirements, desired changes, optional improvements, and uncertain areas are separated |
| `wge.reforge.inspect` | Archaeology: structured inventory | `REFORGE_INVENTORY` (24 categories, §8) | Enough semantic understanding exists to modernize safely (not documentation completeness) |
| `wge.reforge.baseline` | Get the original running reproducibly; freeze observable behavior | `BASELINE_MANIFEST`, `COMPATIBILITY_CONTRACT`, `PERFORMANCE_BASELINE`, `VISUAL_BASELINE`, `PROTOCOL_BASELINE`, `GAMEPLAY_BASELINE`, `KNOWN_QUIRKS`, `REPLAY_FIXTURES` | The ratchet exists: implementation may change; protected behavior is evidenced |
| `wge.reforge.model` | Repository structure → semantic system structure | `SEMANTIC_SYSTEM_MAP` | Every major subsystem has locations, dependencies, ownership, mutable state, thread affinity, external contracts, preserved behavior, goals |
| `wge.reforge.classify` | Assign explicit migration posture per subsystem | `MODERNIZATION_CLASSIFICATION` | Every major subsystem is PRESERVE/WRAP/REFACTOR/REPLACE/RETIRE/UNKNOWN with risk + validation method + order |
| `wge.reforge.plan` | Bounded migration design | `MODERNIZATION_PLAN` | Each slice answers: what changes / what stays invariant / how equivalence is checked / how to revert / what capability is gained |
| `wge.reforge.migrate` | Execute one bounded slice | slice record (§24) | Slice's test plan + comparison evidence pass; rollback remains available |
| `wge.reforge.compare` | Continuous old/new comparison | comparison evidence per slice | Continuously comparable, not asserted at the end |
| `wge.reforge.verify` | Verification against protected behavior | `VERIFICATION_REPORT` | Each check is classified equivalent / intentionally changed / unresolved / regressed |
| `wge.reforge.refine` | Fold in Grindstone experience findings | accepted repairs, updated contracts | Accepted experience repairs are incorporated as evidence-backed changes |
| `wge.reforge.package` | Honest packaging + provenance | §26 output set | Rights fail-closed; known differences explicit |

## 5. Maturity status

Initial state: **REFORGE = SEMANTIC / PROCEDURAL.**

Meaning:

- the subsystem has canonical semantics;
- the workflow is repeatable;
- artifacts are structured;
- Cyan can execute it today;
- most operations use generic tools;
- specialized Reforge machinery remains future work.

This status must remain visible. Do not imply native implementation where none
exists.

## 6. Reforge pipeline

```
INTAKE
  ↓
ARCHAEOLOGY
  ↓
BASELINE
  ↓
BEHAVIOR CONTRACT
  ↓
SEMANTIC SYSTEM MAP
  ↓
TARGET STATE
  ↓
MIGRATION PLAN
  ↓
BOUNDED MIGRATION SLICES
  ↓
CONTINUOUS COMPARISON
  ↓
GRINDSTONE REFINEMENT
  ↓
VERIFICATION
  ↓
PACKAGE
```

## 7. Intake

`wge.reforge.intake`

Inputs may include: source repository, binaries, documentation, build scripts,
assets, protocol documentation, screenshots, videos, design notes, existing bug
trackers, community documentation, previous reverse-engineering notes, and
target modernization goals.

Outputs: `REFORGE_GOAL.md`, `SOURCE_MANIFEST`,
`RIGHTS_AND_PROVENANCE_MANIFEST`, `INITIAL_CONSTRAINTS`,
`INITIAL_OPEN_QUESTIONS`.

Cyan distinguishes preservation requirements, desired changes, optional
improvements, and uncertain areas.

## 8. Archaeology

`wge.reforge.inspect` builds a structured inventory (`REFORGE_INVENTORY`):

languages · build systems · dependencies · repository topology · renderer ·
GPU APIs · networking · input · simulation · world state · threading model ·
assets · animation · UI · audio · persistence · scripting · mod surfaces ·
tooling · platform assumptions · test infrastructure · packaging · licensing ·
known technical debt.

The purpose is not documentation completeness. The purpose is to establish
enough semantic understanding to modernize safely.

## 9. Baseline first

`wge.reforge.baseline`: before significant refactoring, get the original
system running reproducibly whenever possible. Freeze observable behavior.

Potential evidence: build result, startup behavior, network traces, packet
fixtures, player movement, camera, screenshots, render captures, input traces,
frame timing, animation behavior, asset-loading behavior, UI flows, persistence
behavior, replay fixtures, protocol fixtures.

Artifacts: `BASELINE_MANIFEST`, `COMPATIBILITY_CONTRACT`,
`PERFORMANCE_BASELINE`, `VISUAL_BASELINE`, `PROTOCOL_BASELINE`,
`GAMEPLAY_BASELINE`, `KNOWN_QUIRKS`, `REPLAY_FIXTURES`.

This creates the ratchet: implementation may change; protected behavior must
remain evidenced.

## 10. Behavior contract

Not every behavior deserves preservation. Reforge distinguishes:

`MUST_PRESERVE` · `SHOULD_PRESERVE` · `MAY_CHANGE` · `SHOULD_CHANGE` ·
`MUST_CHANGE` · `UNKNOWN`

Example:

```
network packet semantics:   MUST_PRESERVE
original art direction:     MUST_PRESERVE
renderer implementation:    MAY_CHANGE
single-threaded asset load: SHOULD_CHANGE
undefined crash:            MUST_CHANGE
historical movement quirk:  UNKNOWN — requires user decision
```

This avoids confusing implementation preservation with experience preservation.

## 11. Semantic system map

`wge.reforge.model` converts repository structure into system structure —
e.g. Authentication, Character Selection, Network Transport, Packet Decode,
World State, Simulation, Input, Camera, Rendering, Asset Loading, UI, Audio,
Persistence, Modding.

Each semantic subsystem maps to: source locations, dependencies, ownership,
mutable state, thread affinity, external contracts, preserved behavior,
modernization goals.

The semantic map becomes more important than historical file layout.

## 12. Modernization classification

Every major subsystem is assigned an explicit migration posture:

`PRESERVE` · `WRAP` · `REFACTOR` · `REPLACE` · `RETIRE` · `UNKNOWN`

Each classification records: subsystem, classification, reason, risk,
dependencies, preserved_behavior, target_state, validation_method,
migration_order.

Example: packet codec `PRESERVE / WRAP`; renderer `REPLACE incrementally`;
blocking asset decode `REFACTOR`; abandoned telemetry layer `RETIRE`.

## 13. Modernization plan

Artifact `MODERNIZATION_PLAN`:

```
├── target architecture
├── preserved contracts
├── migration slices
├── dependencies
├── risk order
├── performance targets
├── compatibility tests
├── rollback boundaries
├── extension goals
└── package target
```

Migration prefers bounded slices over rewrite events. Each slice answers:
what changes? what remains invariant? how is equivalence checked? how do we
revert? what new capability is gained?

## 14. Multithreading / performance modernization

Reforge must not treat multithreading as a universal improvement. Procedure:

1. Establish measured latency and frame-time baselines.
2. Identify actual stalls.
3. Map shared mutable state.
4. Determine ordering constraints.
5. Separate safely parallelizable work.

Candidate domains: asset decoding, filesystem operations, network receive,
packet parsing, render preparation, animation evaluation, world streaming,
background compilation, telemetry, UI preparation.

Simulation state that depends on deterministic ordering may remain serialized.

The system should be able to report, in its own words:

> "The socket loop is not the primary latency source. Main-thread asset
> preparation and render submission produce the visible stalls. Three safe
> worker domains have been identified. Game-state application remains ordered."

Performance work must be evidence-driven.

## 15. Remaster profile

`reforge.profile = remaster`: attempt to improve presentation while preserving
identity. Profiles: `Classic`, `Modern`, `Experimental`.

```
RenderingIntent
├── Classic
│   ├── original geometry behavior
│   ├── original material character
│   ├── original lighting intent
│   ├── original effects style
│   └── modern compatibility fixes only
└── Modern
    ├── same world identity
    ├── improved material response
    ├── modern shadows
    ├── improved atmospheric rendering
    ├── improved terrain presentation
    ├── improved foliage
    ├── improved resolution handling
    ├── modern antialiasing
    ├── better animation presentation
    └── modern post-processing
```

The goal is not "make it look like a generic contemporary engine." The goal is
"improve fidelity while preserving the visual soul." Classic and Modern should
ideally remain selectable policies over shared game truth rather than two
diverging games.

## 16. Style preservation

When visual identity matters, Reforge builds an explicit style model.
Dimensions: silhouette language, palette, material character, lighting
contrast, atmospheric density, terrain composition, architecture, foliage
character, animation presentation, VFX restraint/intensity, UI styling, camera
presentation.

The Modern profile remains constrained by the preserved style model.
Grindstone can help determine when technically superior rendering has drifted
from the original experience.

## 17. Moddability modernization

A major Reforge goal may be: "Make the client AzerothCore-class moddable."
This does not mean merely cleaning source code. It requires intentional
extension surfaces: semantic hooks, versioned APIs, plugin registration,
data-driven content, stable scripting boundaries, capability manifests,
modular packages, external content definitions, event hooks, extension-owned
assets, extension-owned gameplay, compatibility/version negotiation.

Desired direction:

```
hardcoded behavior
    ↓
explicit semantic capability
    ↓
stable extension seam
```

Community extensions should avoid repeatedly editing engine core.

## 18. Semantic stub growth strategy

Initial Reforge operations are performed procedurally. Repeated friction is
recorded, and recurring operations crystallize:

```
Cyan repeatedly reconstructs dependency graphs
    ↓ build dedicated dependency archaeology helper
Cyan repeatedly captures old/new renders
    ↓ build renderer comparison primitive
Cyan repeatedly determines thread affinity
    ↓ build concurrency-analysis helper
Cyan repeatedly identifies extension points
    ↓ build extension-surface extractor
Cyan repeatedly freezes packet behavior
    ↓ build protocol baseline machinery
```

Eventually: framework → procedure → helpers → typed subsystem machinery. This
is intentional.

## 19. Reforge + Grindstone

Reforge modernizes implementation. Grindstone protects and refines experience.
Together:

```
REFORGE     "preserve and modernize the software"
GRINDSTONE  "preserve and improve how it feels"
```

Example — user: "Movement feels subtly worse than the original." Grindstone
captures original trace, modern trace, input timing, camera behavior,
acceleration, latency, animation state. Cyan identifies the discrepancy. A
candidate repair is tested. Reforge then incorporates the accepted result.

(Grindstone's own registered specification: `docs/add-ons/grindstone-spec.md`.)

## 20. Classic vs Modern comparison

A strong Reforge Remaster permits controlled comparison — a UX where the user
may stand in the same location and switch presentation profiles:

```
[ Classic ]   [ Modern ]
```

This allows direct evaluation of identity preservation, lighting, materials,
atmosphere, animation presentation, readability, and performance.
Modernization should be inspectable rather than asserted.

## 21. Protocol preservation

Networked legacy clients require special care. Unless explicitly authorized:

> NO protocol modification.

Capture: packet schemas, ordering assumptions, timing expectations,
serialization, handshake behavior, compatibility quirks.

Network modernization may change internal implementation while preserving
observable protocol behavior. Any intentional protocol change requires explicit
migration semantics.

## 22. Rights / licensing / provenance

Source availability is not equivalent to unrestricted redistribution rights.
Before packaging or redistributing a Reforge result, inspect: source licenses,
asset licenses, trademarks, fonts, music, third-party libraries, protocol
documentation provenance, generated material, redistributed binaries, and
proprietary data dependencies.

Artifact: `RIGHTS_AND_PROVENANCE_MANIFEST`. Reforge must not silently assume
that "open repository = reusable everything." Packaging fails closed when
required rights remain unresolved.

## 23. Safety rules

Initial canonical guardrails:

1. NO rewrite without baseline evidence.
2. NO behavior change hidden inside "refactor."
3. NO dependency replacement without compatibility analysis.
4. NO threading change without ownership/order analysis.
5. NO renderer replacement without visual comparison.
6. NO protocol modification without explicit authorization.
7. NO asset redistribution assumption from source availability.
8. NO canonical migration promotion without verification.
9. NO destruction of the old path before the replacement proves itself.

## 24. Bounded migration

Every migration slice defines:

```
parent generation · affected subsystem · preserved invariants ·
expected improvement · implementation delta · test plan ·
comparison evidence · rollback plan · resulting generation
```

This aligns Reforge with WGE's general generation/receipt philosophy: the old
path survives until the replacement proves itself.

## 25. Verify

`wge.reforge.verify` — verification depends on protected behavior. Possible
checks: build reproducibility, protocol fixtures, gameplay traces, screenshot
comparison, render semantics, input behavior, replay comparison, performance
regression, asset loading, extension compatibility, startup/shutdown, save
compatibility, crash resistance.

Verification must distinguish: `equivalent` · `intentionally changed` ·
`unresolved` · `regressed`.

## 26. Package

`wge.reforge.package` describes the resulting system honestly. Outputs:

```
MODERNIZED_CLIENT · REFORGE_REPORT · COMPATIBILITY_REPORT · RIGHTS_MANIFEST ·
PERFORMANCE_REPORT · MODDING_GUIDE · CLASSIC_MODERN_PROFILE_DOC ·
MIGRATION_HISTORY · KNOWN_DIFFERENCES
```

The result preserves provenance from original behavior to final system.

## 27. First practical example: legacy MMO client

Example user goal:

> "Reforge this old MMO client. Preserve gameplay and existing server
> compatibility. Improve responsiveness. Safely multithread obvious worker
> domains. Add Classic and Modern graphics profiles. Keep Modern faithful to
> the visual identity of the original. Refactor toward long-term community
> moddability. Do not alter gameplay behavior unless I approve it."

Expected Reforge execution:

```
repository archaeology → baseline build → protocol freeze → visual baseline →
performance profiling → subsystem map → modernization classification →
bounded migration → Classic/Modern rendering profile → extension seams →
Grindstone comparison → verified package
```

The user acts primarily as creative/technical director. Cyan performs the
excavation and routine engineering.

## 28. Future dedicated machinery

Potential native Reforge primitives — dependency archaeology, semantic
repository mapping, call-graph analysis, thread-affinity inference,
behavior-capture harnesses, protocol freezing, old/new renderer comparison,
asset-pipeline extraction, migration slicing, extension-surface inference,
compatibility scoring, automated modernization receipts, patch-series
planning, legacy subsystem classifiers.

These should be earned through repeated use. Do not build them merely because
they sound useful.

## 29. Friction ledger

Every Reforge project records: repeated manual archaeology, source-search
volume, repeated transformations, missing tools, invalid assumptions,
hard-to-compare behavior, recurring failure classes, excessive context
requirements, high-latency procedures, reusable helper scripts.

This becomes the evidence base for Reforge's eventual implementation, and is
appended to the campaign's existing friction evidence
(`docs/platform/friction-ledger.md`).

## 30. Success criteria for the semantic stub

The initial Reforge Semantic Stub is successful if Cyan can reliably take a
legacy project and produce:

1. an honest inventory;
2. a reproducible baseline;
3. a preservation contract;
4. a semantic system map;
5. a modernization classification;
6. a bounded migration plan;
7. verified modernization slices;
8. an explicit difference report;
9. a provenance/rights report;
10. a maintainable final package;

without requiring the user to manually explain every implementation detail.
That is useful even before dedicated Reforge machinery exists.

## 31. North star

A user should eventually be able to say:

> "I love this old client. Keep what makes it itself. Fix what age made
> painful. Modernize the architecture. Make it easier to modify. Give me a
> faithful modern presentation. Show me anything you're unsure about."

And Cyan should be able to turn that intent into a disciplined modernization
campaign rather than a speculative rewrite.

WGE Reforge is the mechanism for bringing old software forward without
discarding the reasons it mattered.

## 32. The stub pattern beyond Reforge

RPDO Semantic Stubbing is not Reforge-specific. When a capability's canonical
semantics are clear but its implementation should not be committed to yet, the
move is the same: register the semantics, the artifact contracts, the
guarantees, and the visible maturity status — then let Cyan execute
procedurally and let machinery be earned. Subsystems registered this way keep
their operations' meaning stable while the implementation underneath changes;
this document is the first use of that pattern at full scale, and the same
move is available for any operator-facing subsystem whose semantics are ahead
of its code.