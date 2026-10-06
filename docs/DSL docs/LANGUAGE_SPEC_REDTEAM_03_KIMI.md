# Red team 03 — Luxel model-native language specification (Kimi/Cursor)

**Reviewing:** `docs/DSL docs/LUXEL_LANGUAGE_SPEC.md` draft 0.4, 2026-08-03  
**Reviewer:** Cursor / Kimi (`cursor`), 2026-08-03  
**Requested by:** Matt  
**Scope:** Specification audit only. No edits to the canonical spec or code.  
**Method:** Independent findings formed against draft 0.4 before reading
`LANGUAGE_SPEC_REDTEAM_01.md`, `LANGUAGE_SPEC_REDTEAM_02.md`, or
`deepseek_redteam_01.md`. Prior files were then used only to classify novelty
and to verify claimed resolutions.  
**Taxonomy:** §20 classes — specification defect, implementation defect,
missing conformance test, backend limitation, usability regression, accepted
risk.

Severity scale used here:

| Severity | Meaning |
|---|---|
| Critical | Contradictory or incomplete MUST that breaks hashing, identity, certification, or sandbox authority if implemented as written |
| High | Normative gap or conflict that will force Codex to invent policy or ship silent nondeterminism |
| Medium | Incomplete algebra/API surface that blocks a conformance fixture or confuses small-model authoring |
| Low | Consistency / editorial / inventory issues that should still be fixed before freeze |

---

## Executive verdict

Draft 0.4 is substantially stronger than the early drafts: Grade-A profiles,
mandatory stochastic keys, three digest domains, independent realized-hard
validation, trusted evidence manifests, and strict/lenient promotion are real
improvements. It is **not freeze-ready** and **not safe for Codex to treat as
an implementation contract** until the Critical/High items below are repaired.

The densest remaining failure mode is **digest/identity underdefinition with
contradictory normative sentences** — exactly the class §2.4 and §8 claim to
have closed.

---

## Independent findings

### K3-01 — `document_digest` preimage both excludes and includes the digest fields

**Severity:** Critical  
**Class:** specification defect  
**Where:** §13.3, lines 730–734

**Defect.** The same paragraph states two incompatible preimage rules:

> `document_digest` covers the complete canonical IR document … **but excluding
> the three digest fields themselves**. … `semantic_digest` and
> `extension_digest` **remain ordinary included fields in this preimage**.

Either all three digest fields are excluded, or `semantic_digest` /
`extension_digest` are included. Both cannot be normative.

**Counterexample.**

```text
IR₀ = { …, semantic_digest: S₀, extension_digest: E₀, document_digest: ? }
```

Implementation A excludes `{S,E,D}` from the `document_digest` preimage and
hashes body bytes only. Implementation B excludes only `document_digest` and
includes `S₀`/`E₀` as ordinary fields. Equal IR bodies yield different
`document_digest` values → Grade-A / tamper-evidence split across conforming
compilers.

**Normative repair.** Replace the paragraph with one of these (prefer A):

> **A (recommended):** `document_digest` is SHA-256 over the canonical IR
> document with only the `document_digest` field itself omitted from the
> preimage. `semantic_digest` and `extension_digest` are ordinary included
> fields. Digest fields never appear in their own preimages.
>
> **B:** `document_digest` excludes all three digest fields and covers body
> content directly, including ignorable extension opaque bytes. Readers still
> verify `semantic_digest` and `extension_digest` as independent checks; those
> two fields are not inputs to `document_digest`.

**Regression fixtures.**

```text
fixture.document_digest.preimage_exclusion:
  - Emit IR with fixed body; compute document_digest under rule A or B (pinned).
  - Mutate only semantic_digest text field without changing body; assert whether
    document_digest MUST change (A) or MUST NOT (B) per the chosen rule.
  - Two compilers implementing the pinned rule MUST agree bit-for-bit.
```

---

### K3-02 — Stochastic stream tuple still contains undefined `operation_path` and overlapping identity axes

**Severity:** Critical  
**Class:** specification defect  
**Where:** §7 lines 359–371; §8 lines 400–404; §19.1 lines 1004–1007

**Defect.** §7 now correctly requires a mandatory explicit key for every
stochastic operation under a fully qualified declaration ID. §8 still derives
streams from:

```text
(module_seed, declaration_id, explicit_nested_key, explicit_seed, operation_path)
```

`operation_path` is never defined. `explicit_nested_key` is not equated to the
§7 key. `explicit_seed`’s optionality and defaulting are undefined. A
conforming implementer can lawfully invent path schemes (AST index, source
span, lowering-order ordinal) that reintroduce the reroll class §7 claimed to
kill.

**Counterexample.**

```python
from luxel.geometry import scatter
grove = scatter(domain=plot, mask=ridge, count=40, key="grove", seed=7)
# Add a pure, non-stochastic annotation sibling under the same owner:
grove_meta = annotate(grove, note="north ridge")
```

Compiler A’s `operation_path` is `declarations[i]/scatter`. Compiler B’s path
includes sibling ordinals and becomes `declarations[i]/ops[0]` after the
annotation is inserted → same keys, different stream → realized instances move
despite §8’s closing MUST.

**Normative repair.**

> Random streams derive solely from
> `(module_seed, fully_qualified_declaration_id, stochastic_key, explicit_seed)`.
> `stochastic_key` is the mandatory §7 key; it is required even when only one
> stochastic operation exists under the owner. `explicit_seed` is either a
> constructor argument of type `Seed` or the distinguished absent sentinel
> recorded in IR (one canonical encoding). No source span, occurrence index,
> AST path, lowering ordinal, or whole-program digest may participate.
> `operation_path` is removed from the 1.0 stream tuple.

**Regression fixtures.**

```text
fixture.stream.no_operation_path:
  - One stochastic op; add non-stochastic sibling; assert byte-identical instances.
fixture.stream.key_mandatory:
  - Stochastic constructor missing key → compile error with repair inserting key.
fixture.stream.seed_sentinel:
  - Omitted seed and explicit null/absent seed canonicalize identically.
```

---

### K3-03 — Evidence digests are neither included in nor excluded from `semantic_digest`

**Severity:** High  
**Class:** specification defect  
**Where:** §11.3; §12 (“Reviewed concept annotations remain evidence/topology
authority…”); §13.3 lines 719–726; §16.1

**Defect.** `semantic_digest` lists include/exclude sets in detail and never
mentions evidence digests, evidence manifests, or evidence logical IDs.
World text says reviewed annotations remain topology authority until an
explicit authoring operation changes topology. Asset text says images are not
topology authority. Without a digest rule, two builds can pin different
evidence under identical source and either:

1. produce different IR declarations while claiming the same `semantic_digest`, or
2. produce identical IR while silently swapping topology authority.

**Counterexample.**

```python
# same .luxel source
world = World(features=reviewed("ridge_01"), ...)
```

Build request A pins `ridge_01 → sha256:aaa…`. Build B pins `ridge_01 → sha256:bbb…`
with different polyline bytes. If evidence is outside `semantic_digest` and
features are materialized only at construction time, certification can diverge
while semantic caches hit. If features are lowered into IR declarations without
hashing the evidence pin, IR lies about its authority.

**Normative repair.**

> When a declaration’s meaning depends on evidence identity, the content digest
> (and logical ID) of every evidence entry it names is part of the semantic
> projection and MUST appear in the `semantic_digest` preimage under a
> dedicated `evidence_pins` field. Evidence that informs only realized/acceptance
> measurements and cannot change IR declarations is listed in provenance and
> construction-plan digests, not in `semantic_digest`. World reviewed-topology
> pins are always semantic. Asset silhouette/texture evidence is semantic only
> when a constructor claims a topology- or hard-constraint dependence; otherwise
> it is construction/certification input.

**Regression fixtures.**

```text
fixture.evidence.semantic_pin:
  - Same source; flip one reviewed-feature digest; semantic_digest MUST change.
fixture.evidence.non_semantic_texture:
  - Same source/topology; flip texture-only evidence; semantic_digest unchanged;
    document/construction digests change; certification input recorded.
```

---

### K3-04 — Open decision §24.3 contradicts normative §5.3 forward-reference rule

**Severity:** High  
**Class:** specification defect  
**Where:** §5.3 lines 223–226; §24 item 3 line 1167

**Defect.** §5.3 is already normative:

> A reference may point only to a prior declaration visible in the current
> module or an imported immutable symbol.

§24.3 still lists “Whether source permits forward references within a module”
as unresolved. Open decisions “must not be implemented as accidental policy”
(header), but here the body already chose. Codex cannot both obey §5.3 and
treat OD3 as open.

**Counterexample.**

```python
sword = asset(parts=[blade, guard])  # forward refs
blade = sweep(...)
guard = sweep(...)
```

Compiler-as-§5.3: reject. Compiler-as-OD3-open: may accept. Both claim
conformance to draft 0.4.

**Normative repair.** Either:

> Delete §24.3 and add to §5.3: “Forward references are forbidden in language
> 1.0. Name binding is single-pass over declaration order.”

or:

> Demote §5.3’s prior-declaration sentence to an Open decision pointer and mark
> the rule non-normative until resolved — not recommended; breaks asset
> examples’ intended discipline.

**Regression fixtures.**

```text
fixture.bind.forward_ref_rejected:
  - Forward name use before assignment → binding error naming both spans.
fixture.bind.order_ok:
  - Prior-declaration reference compiles.
```

---

### K3-05 — `null` literals vs Python-shaped `None`

**Severity:** High  
**Class:** specification defect / usability regression  
**Where:** §5.2 line 188; §1 purpose (“Python-shaped”); §2.13

**Defect.** Allowed literals include “null literals.” Python AST and small-model
priors produce `None`, not `null`. JSON-minded authors may emit `null` as a
Name or as invalid syntax. The spec never equates them.

**Counterexample.**

```python
seed = None
fallback = null   # NameError in Python; unclear in Luxel
```

**Normative repair.**

> The only null literal spelling is the Python `None` keyword. It lowers to IR
> `null` / `Option::None`. The identifier `null` is not a literal; using it as
> a bare name is a binding error unless imported. Diagnostics MAY suggest
> replacing `null` with `None`.

**Regression fixtures.**

```text
fixture.lit.none_ok / fixture.lit.null_name_rejected / fixture.repair.null_to_none
```

---

### K3-06 — Result type of `/` on integer operands is undefined

**Severity:** High  
**Class:** specification defect  
**Where:** §5.2 lines 189–190; §6.2 lines 287–293

**Defect.** Binary `/` is permitted. Integers and floats are distinct; integer
MAY widen when the target permits. Python 3 semantics give `3/2 == 1.5`. The
spec never states the result type of `Int / Int`, nor whether dimensional
`Int` quantities follow the same rule.

**Counterexample.**

```python
n = 3 / 2
require(thickness(blade.edge) <= n * mm)  # Float or type error?
```

**Normative repair.**

> For dimensionless operands, `/` always yields `Float` (binary64) after any
> needed integer widening. There is no integer division operator in 1.0; `//`
> and `%` are rejected. For quantities, `/` subtracts dimension vectors and
> yields `Quantity`/`Float` per the algebra; integer-valued quantities still
> widen to binary64 for division. Exact rational arithmetic is out of scope.

**Regression fixtures.**

```text
fixture.div.int_int_is_float / fixture.div.floor_op_rejected / fixture.div.length_time_velocity
```

---

### K3-07 — Negative-zero behavior required by conformance but unspecified in §6.2

**Severity:** Medium  
**Class:** specification defect / missing conformance definition  
**Where:** §6.2; §19.1 line 1002

**Defect.** §19.1 requires tests for “negative-zero behavior.” §6.2 never
defines whether typed evaluation preserves `-0.0`, whether it is distinct from
`+0.0` before JCS, or whether JCS collapse is the sole rule.

**Counterexample.**

```python
a = 0.0
b = -0.0
```

Under IEEE they compare equal but differ in sign bit; JCS serializes both as
`0`. If IR retains bits, digests can diverge from JCS bytes.

**Normative repair.**

> During semantic evaluation, floating-point values use IEEE-754 binary64
> including signed zero. Equality predicates use IEEE equality (`+0.0 == -0.0`).
> Canonical JSON encoding MUST follow JCS and therefore emits a single `0` for
> both. Canonical IR in-memory and JCS bytes therefore intentionally differ in
> sign-bit representation; digest preimages use JCS bytes, not host bit
> patterns.

**Regression fixtures.**

```text
fixture.f64.neg_zero_eq / fixture.f64.neg_zero_jcs_digest_identical
```

---

### K3-08 — Source `Int` outside the interoperable JSON range is unspecified

**Severity:** Medium  
**Class:** specification defect  
**Where:** §6.2 lines 301–304

**Defect.** Untagged IR integers are limited to `[-(2^53-1), 2^53-1]`. Source
acceptance of larger integer literals, and the phase that rejects them, are
unspecified.

**Counterexample.**

```python
n = 9007199254740993  # 2^53+1
```

**Normative repair.**

> Source integer literals and exact integer values that cannot be represented
> in the interoperable range are a type/domain error at phase 4 (type check)
> unless the constructor’s declared type is a tagged big-integer encoding
> (none in 1.0 core). Widening to `Float` is NOT an implicit escape hatch for
> out-of-range integers.

**Regression fixtures.**

```text
fixture.int.range_max_ok / fixture.int.range_overflow_rejected / fixture.int.no_silent_float_escape
```

---

### K3-09 — `module_seed` “declares or receives” is undefined for multi-module programs

**Severity:** Medium  
**Class:** specification defect  
**Where:** §5.5 lines 257–259; §8 lines 400–401; §13.2 `module_seed`

**Defect.** Every module “declares or receives” a stable `module_seed`.
Receiving authority, inheritance from parents, and conflict when both declared
and supplied by the build request are unspecified. Multi-module programs are
allowed but have no seed algebra.

**Counterexample.**

```text
build: module_seed=0xABC for package
module land.luxel declares module_seed=0xABC
module props.luxel declares nothing
module props.luxel declares module_seed=0xDEF  # conflict?
```

**Normative repair.**

> Each module has exactly one `module_seed` recorded in IR. It is taken from,
> in order: (1) an explicit `module_seed = ...` declaration of type `Seed` if
> present; else (2) the build-request seed for that module ID; else (3) a
> registry-defined derivation `H("luxel.module_seed"|nul|package_seed|module_id)`.
> A build-request seed that disagrees with an explicit declaration is an error.
> Package-level seeds never silently alias across modules.

**Regression fixtures.**

```text
fixture.seed.explicit_wins / fixture.seed.request_conflict / fixture.seed.derived_stable
```

---

### K3-10 — “Feasibility classes” claimed in the draft changelog but not defined as a closed set

**Severity:** Medium  
**Class:** specification defect  
**Where:** draft header lines 17–18; §9; §10 lines 464–467; §19.1 lines 1032–1036

**Defect.** Draft 0.4 claims incorporation of “feasibility classes.” The body
distinguishes static vs evidence-dependent conditions narratively but never
defines a closed, named enumeration that registries and diagnostics must use.

**Counterexample.** Registry authors invent `dynamic`, `runtime`, `best_effort`
labels; validators disagree on whether a field is allowed to be static-claimed.

**Normative repair.**

> Every registry bound carries a feasibility class in
> `{ static_hard, realized_hard, evidence_dependent, soft_objective }`.
> `static_hard` MUST be decided from source+registry+target alone.
> `evidence_dependent` MUST NOT be advertised as statically buildable.
> Diagnostics and §19 fixtures key off these four classes only in 1.0.

**Regression fixtures.**

```text
fixture.feasibility.static_claimed_but_evidence → registry/spec failure
fixture.feasibility.evidence_reject_preserves_source_valid
```

---

### K3-11 — Extension dependencies claimed but unspecified

**Severity:** Medium  
**Class:** specification defect  
**Where:** draft header line 17; §13.1 extension envelope; §18 versioning

**Defect.** Changelog lists “extension dependencies.” The envelope has
namespace/version/criticality/media type/payload/digest but no dependency
edges onto other extensions, registry versions, or language versions.

**Counterexample.** Critical extension `luxel.x.materials@2` requires
`luxel.x.uv@1`. Reader understands materials, ignores missing UV dependency,
emits wrong artifact while claiming critical-extension compliance.

**Normative repair.**

> Each extension record MAY declare `requires: [{namespace, version_range}]`.
> Unknown or unsatisfied requirements of a critical extension are rejection.
> Ignorable extensions with unsatisfied requires are dropped only if a
> registered policy says so; default 1.0 is reject-on-unsatisfied even for
> ignorable, or require requires=[] for ignorable — pick one and pin it.
> Recommended: ignorable extensions MUST NOT declare requires that affect
> artifact bytes; if they do, reclassify critical.

**Regression fixtures.**

```text
fixture.ext.requires_missing_critical_reject
fixture.ext.artifact_affecting_cannot_be_ignorable
```

---

### K3-12 — §5.4 allows positional “primary operands” while §24.5 leaves the rule open

**Severity:** Medium  
**Class:** specification defect  
**Where:** §5.4 lines 234–236; §24 item 5 line 1169

**Defect.** Same class as K3-04: body policy vs open decision conflict. Small
models will emit positional args freely if the body allows “primary operands.”

**Normative repair.** Resolve OD5 before Codex implements the parser. Preferred
for §2.13:

> Positional arguments are permitted only for the stable identity/`key` slot
> where a constructor declares one. All other operands are keyword-only.

**Regression fixtures.**

```text
fixture.args.positional_non_key_rejected / fixture.args.key_positional_ok
```

---

### K3-13 — Evidence symlink non-follow is softened to “where the host permits”

**Severity:** Medium  
**Class:** specification defect / accepted risk if explicitly waived  
**Where:** §16.1 lines 870–874; §19.2 evidence-resolver attack fixtures

**Defect.** The resolver “opens files without following a replaceable symlink
where the host permits.” That clause makes a security MUST host-conditional.
Conformance on Linux vs a host without `O_NOFOLLOW`/`openat` semantics becomes
unequal; attackers target the weaker host.

**Normative repair.**

> Grade-A and production resolvers MUST provide symlink-replace race resistance
> equivalent to open-without-follow + re-validate through `/proc/self/fd` or
> platform analogue. Targets that cannot meet this MUST NOT claim the
> production evidence-resolver capability; they may expose a Grade-B
> `evidence_resolver.best_effort@1` that cannot certify production artifacts.

**Regression fixtures.**

```text
fixture.evidence.symlink_swap_during_open → distinct structured error, no skip
fixture.evidence.capability_absent_on_weak_host
```

---

### K3-14 — Certified asset package contents gated by “when applicable”

**Severity:** Low  
**Class:** specification defect  
**Where:** §11.5 lines 533–544

**Defect.** “SHALL include, when applicable” lets a backend omit collision,
sockets, or validation reports without a machine-readable applicability rule.

**Normative repair.**

> Each product profile (`asset.render@1`, `asset.gameplay@1`, …) declares a
> closed required/optional artifact set. “Not applicable” is allowed only when
> the profile marks the item optional or the IR explicitly opts out with a
> typed absence reason recorded in the manifest.

**Regression fixtures.**

```text
fixture.product.gameplay_requires_collider / fixture.product.explicit_lod_absence
```

---

### K3-15 — Import vs local binding collision / “shadowing” underspecified

**Severity:** Medium  
**Class:** specification defect / missing conformance test definition  
**Where:** §5.2–5.3; §5.5; §19.1 “shadowing”

**Defect.** Rebinding is forbidden. Imports bind names. §19.1 requires
shadowing tests, but the language never defines whether importing `line` and
later assigning `line = ...` is rebinding, shadowing, or a distinct error.
Collection of imported names vs declaration names is unspecified.

**Counterexample.**

```python
from luxel.geometry import line
line = sweep(path=line, profile=diamond(width=1*mm, depth=1*mm))
```

**Normative repair.**

> The module namespace is a single immutable map. Import bindings occupy names.
> A later declaration using an imported name is a rebinding error
> (`E_REBIND_IMPORT`) with a repair renaming the local declaration. There is
> no shadowing in 1.0; conformance “shadowing” fixtures assert rejection.

**Regression fixtures.**

```text
fixture.bind.import_rebind_rejected / fixture.bind.rename_repair
```

---

### K3-16 — Default resolution is sequenced before capability negotiation

**Severity:** Medium  
**Class:** specification defect  
**Where:** §14 phases 5 and 7; §13.3 semantic projection includes target and
capabilities; §17 negotiation paragraph

**Defect.** Phase 5 resolves defaults before phase 7 negotiates capabilities.
If defaults are capability- or backend-profile-dependent, phase 5 either
guesses or freezes wrong defaults that later negotiation cannot ethically
rewrite without violating “no silent repair.”

**Counterexample.** Constructor default `method=sdf_exact` requires capability
absent on target; fallback default `method=hull` exists as explicit strategy.
Phase 5 materializes `sdf_exact`; phase 7 rejects. Author sees hard failure
instead of the registered explicit fallback selected only when IR names it.

**Normative repair.**

> Target identity and the requested capability set are build-request inputs
> available from phase 1. Phase 5 may resolve only defaults conditioned on
> that declared target/capability *request* set. Phase 7 verifies a backend
> can satisfy the request; it MUST NOT reinterpret defaults. Capability-conditioned
> defaults are registry functions of `(constructor, target, requested_capabilities)`
> and are recorded explicitly in IR.

**Regression fixtures.**

```text
fixture.defaults.target_conditioned_stable
fixture.defaults.no_post_hoc_rewrite_after_negotiation
```

---

### K3-17 — Nominal-angle algebra claims closure but omits additive operators and conversion set

**Severity:** Medium  
**Class:** specification defect  
**Where:** §6.3 lines 316–327; §24 item 7; deepseek FM-01 residual

**Defect.** Text says the algebra is “closed and explicit” then lists only
`Angle/Angle`, `Angle*Ratio`, and `radians(Angle)`. Missing: `Angle±Angle`,
`Ratio*Angle` commutativity, `degrees`, comparisons, and whether `Angle` shares
the dimensionless exponent vector (deepseek’s paradox) or has a nominal tag
over a zero vector.

**Counterexample.**

```python
a = 30*deg
b = 15*deg
c = a + b          # allowed?
r = a / deg        # Ratio or still Angle?
q = radians(a) / 2
```

**Normative repair.**

> `Angle` is a nominal type whose dimensional vector is the zero vector but
> which does not unify with `Ratio`/`Dimensionless`. Closed ops:
> `Angle±Angle→Angle`, `Angle*Ratio→Angle`, `Ratio*Angle→Angle`,
> `Angle/Angle→Ratio`, `Angle/Ratio→Angle`,
> `radians(Angle)→Ratio`, `degrees(Angle)→Ratio`,
> `from_radians(Ratio)→Angle`, `from_degrees(Ratio)→Angle`.
> No op yields bare `Float` without an explicit conversion. Comparisons are
> defined on `Angle`×`Angle` only.

**Regression fixtures.**

```text
fixture.angle.add_ok / fixture.angle.plus_ratio_rejected / fixture.angle.radians_ratio
```

---

### K3-18 — Changelog claims “selected-component readiness” with no body definition

**Severity:** Low  
**Class:** specification defect (editorial / false completeness)  
**Where:** draft header lines 11–12; §17 bakeoff table; §25

**Defect.** Draft 0.3 changelog asserts “selected-component readiness” is fully
specified. Aside from §25’s bakeoff-selected component gate (good fix for
RT02-3), there is no definition of readiness criteria, interface contracts, or
pass/fail for a selected component beyond “reproducible build.”

**Normative repair.**

> Either remove the changelog claim, or add §21.1: a selected component is
> ready when it has a versioned capability manifest, Grade declaration,
> reproducible build, fuzz surface for its parsers/decoders, and a boundary
> fixture pack against the reference IR.

**Regression fixtures.** Documentation/consistency check; optional
`fixture.bakeoff.component_ready_checklist`.

---

## Prior-finding verification matrix

Statuses against **draft 0.4 text** (not against changelogs alone):

| Prior ID | Title (short) | Draft 0.4 status | Notes / residual |
|---|---|---|---|
| RT01-1 | Stream keyed on program digest | **Resolved** | Tuple no longer uses program digest; see K3-02 residual on `operation_path` |
| RT01-2 | Envelope omits registry/compiler | **Resolved** | Present in §13.2 |
| RT01-3 | Artifact byte determinism demoted | **Resolved** | Grades A/B/C + §17.1 profiles |
| RT01-4 | Legal-means-buildable SHOULD | **Resolved** | MUST + evidence-dependent marking |
| RT01-5 | Lenient rebind repair | **Resolved** | §15 confidence window |
| RT01-6 | Units enumerated not algebra | **Resolved** | Exponent vectors; residual K3-17 on angle |
| RT01-7 | World domain thin | **Resolved** | §12.1–12.4 |
| RT01-8 | Unknown fields closed only in cert mode | **Resolved** | Closed every mode + extensions |
| RT01-9 | §4/§17 prejudge bakeoff | **Resolved** | Hypotheses table; §25 uses selected components |
| RT01-10 | No conditional placement | **Resolved** | §12.2 masks |
| RT01-11 | Single-cause gate diagnostics | **Resolved** | local/ranked/global + invariant 14 |
| RT01-12 | Content-derived IDs unify seeds | **Resolved** | Stable keys; strengthened again in 0.3/0.4 |
| RT01-13 | Repair edits not batchable | **Resolved** | Atomic disjoint batches |
| RT01-14 | Evidence resolver unspecified | **Mostly resolved** | §16.1 exists; residual soft-escape K3-13 |
| RT01-15 | Usability threshold absent | **Resolved as open decision** | §24.16–17 / metrics pinned in §19.3 without magic numbers |
| RT02-1 | Conditional nested key rerolls | **Mostly resolved** | Keys mandatory in §7; residual K3-02 (`operation_path`) |
| RT02-2 | “Normalized artifact” undefined | **Resolved** | §17.1 closed profiles |
| RT02-3 | §25 pre-bakeoff host list | **Resolved** | Selected-component wording |
| RT02-4 | Extension vs digest relationship | **Regressed / incomplete** | Three digests exist, but K3-01 makes `document_digest` preimage contradictory |
| DS-01 | Grade-A underspecified | **Mostly resolved** | JCS + profiles; still depends on K3-01 integrity |
| DS-02 | Stochastic identity contradictions | **Partially resolved** | Module+declaration+key refined; K3-02 still Critical |
| DS-03 | Evidence trust boundaries | **Mostly resolved** | Caller-provided manifest; residual K3-13, K3-03 |
| DS-04 | Cache vs provenance | **Resolved** | Phase schemas + reuse events |
| DS-05 | Unsound repairs | **Resolved** | Repair classes + postcheck |
| DS-06 | Solver success certifies | **Resolved** | Independent realized-hard validation |
| DS-07 | Model-usability metrics | **Resolved (refined)** | Pinning required; thresholds remain OD |
| DS-08 | Extension critical vs ignorable | **Mostly resolved** | Artifact-affecting⇒critical; residual K3-11 deps |
| DS-09 | Source map / round-trip | **Resolved** | Many-to-many + semantic_digest round-trip |
| DS-10 | Buildable region impossible | **Mostly resolved** | State split; residual K3-10 taxonomy |
| DS-11 | Silent repair vs lenient | **Resolved** | No certifiable IR from lenient |
| DS-12 | Cross-domain transforms | **Resolved** | Explicit typed transforms |
| DS FM-01 | Angle vs dimensionless | **Open residual** | Partial algebra in §6.3; K3-17 |
| DS FM-03 | Extension cache poisoning | **Mostly resolved** | Phase digest deps; needs K3-01 correct |
| DS FM-04 | Capability negotiation | **Mostly resolved** | Versioned contracts; residual K3-16 ordering |
| DS FM-05 | Policy inheritance | **Accepted deferral** | Explicitly not 1.0; migration still must not invent it |

### Classification of this review’s findings vs priors

| This ID | Novelty |
|---|---|
| K3-01 | **New regression** on the RT02-4 / DS digest design |
| K3-02 | **Residual** of RT01-1 / RT02-1 / DS-02 |
| K3-03 | **New** (adjacent to DS-03, different digest-authority angle) |
| K3-04 | **New** normative↔OD contradiction |
| K3-05 | **New** usability/syntax |
| K3-06 | **New** |
| K3-07 | **New** (conformance orphan) |
| K3-08 | **New** (edge of DS-01 numeric work) |
| K3-09 | **New** |
| K3-10 | **Residual** of DS-10 changelog claim |
| K3-11 | **Residual** of DS-08 changelog claim |
| K3-12 | **New** normative↔OD contradiction (same pattern as K3-04) |
| K3-13 | **Residual** of RT01-14 / DS-03 |
| K3-14 | **New** (low) |
| K3-15 | **New** (conformance term without rule) |
| K3-16 | **New** / adjacent DS FM-04 |
| K3-17 | **Residual** of DS FM-01 |
| K3-18 | **New** changelog false-completeness |

---

## Freeze recommendation

**Do not freeze draft 0.4 as language 1.0.**  
**Do not treat draft 0.4 as a green light for Codex to implement the compiler
kernel without a 0.5 patch of the Critical/High set.**

### Blockers before Codex semantic-compiler / IR work

Must land in a **draft 0.5** (normative text + fixtures named, even if fixtures
are not yet executed):

1. **K3-01** — pick and write one `document_digest` preimage rule  
2. **K3-02** — delete/define away `operation_path`; pin the 4-tuple  
3. **K3-03** — evidence pin participation in `semantic_digest`  
4. **K3-04** and **K3-12** — close or demote conflicting open decisions  
5. **K3-05** and **K3-06** — literal and `/` typing (parser surface)

### Can follow immediately after 0.5 without blocking a vertical slice

K3-07, K3-08, K3-09, K3-10, K3-11, K3-13, K3-15, K3-16, K3-17 — still required
before freeze; may proceed in parallel with early parser scaffolding if the
blockers above are merged first.

### Explicitly non-blocking for early exploration

K3-14, K3-18 — fix before freeze; do not stall spike work.

### Freeze gate statement (recommended addition to §25)

> Language 1.0 shall not freeze while any Critical finding from an accepted
> red-team report remains open, or while any High finding that affects digest
> domains, stochastic identity, evidence authority, or parser literal/operator
> typing remains open.

---

## What was not verified

- No execution of `pipeline/worldbuilder_dsl.py` or conformance suites  
- No fuzzing, no backend builds, no model-usability runs  
- Prior red-team **resolutions** were checked against draft 0.4 prose only  
- Line numbers cite the draft 0.4 file as read on 2026-08-03; drift after
  subsequent edits is out of scope for this report

---

## Summary table

| ID | Severity | One-line |
|---|---|---|
| K3-01 | Critical | `document_digest` preimage self-contradicts |
| K3-02 | Critical | `operation_path` undefined; stream axes overlap |
| K3-03 | High | Evidence pins missing from semantic digest rules |
| K3-04 | High | OD3 vs §5.3 forward-ref contradiction |
| K3-05 | High | `null` vs `None` unspecified |
| K3-06 | High | `Int/Int` division type unspecified |
| K3-07 | Medium | Neg-zero required in tests, undefined in §6.2 |
| K3-08 | Medium | Out-of-range source Int phase undefined |
| K3-09 | Medium | `module_seed` receive/conflict rules missing |
| K3-10 | Medium | Feasibility classes not a closed enum |
| K3-11 | Medium | Extension dependencies unspecified |
| K3-12 | Medium | OD5 vs §5.4 positional-args contradiction |
| K3-13 | Medium | Symlink non-follow host escape hatch |
| K3-14 | Low | “When applicable” certified package hole |
| K3-15 | Medium | Shadowing/import rebind undefined |
| K3-16 | Medium | Defaults before capability negotiation |
| K3-17 | Medium | Angle algebra incomplete vs “closed” claim |
| K3-18 | Low | Changelog readiness claim without body |

**Bottom line:** Prior rounds fixed the large architectural holes. Draft 0.4’s
remaining Critical debt is concentrated in **digest preimage honesty** and
**stochastic stream completeness**. Resolve those before Codex builds the IR
and evaluation kernel; otherwise every later conformance fixture will argue
with the spec.
