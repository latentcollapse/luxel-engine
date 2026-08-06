# Final integrity review — WGE model-native language specification

**Reviewed document:** `WGE_LANGUAGE_SPEC.md`, draft 0.5, 2026-08-03  
**Review date:** 2026-08-03  
**Reviewer:** Codex (GPT-5.6 Sol, xhigh)  
**Ticket:** `REVIEW-3`  
**Scope:** Integrity consolidation of every prior red-team report, the current
normative text, the prototype compiler, and its unit tests. The canonical
specification and implementation were not edited.

## Executive verdict

**Do not freeze draft 0.5. Cut one normative draft 0.6, then run conformance
fixtures and one verification-only review.**

The architecture survives the complete red-team sequence. The source is parsed
rather than executed, typed IR is authoritative, stochastic identity is
explicit, backend authority is bounded, and certification remains separate
from construction. No unresolved Critical finding was substantiated.

Four unresolved High findings can still make independent conforming
implementations disagree about semantic identity or build meaning:

1. digest-visible arrays have no normative total order;
2. digest-field membership and dependency identity are incomplete;
3. module identity and the authoring form of `module_seed` are unspecified;
4. the selected asset product profile is absent from semantic/build identity.

This report is the authoritative consolidation of unresolved findings. It
does not preserve an earlier severity merely because an earlier reviewer used
it. Duplicate findings are merged, weak threat claims are narrowed, and one
previous High finding is explicitly downgraded.

Severity follows §20. **High** means independently conforming implementations
can diverge on load-bearing semantics, identity, or certification. **Medium**
means the specification forces implementations to invent a policy or exposes
an important usability/migration ambiguity. **Low** means bounded precision or
fixture work remains but does not block the main architecture.

## Historical resolution ledger

| Source | Integrity result against draft 0.5 |
| --- | --- |
| Red team 01, findings 1–15 | Normatively resolved before draft 0.5. |
| Red team 02, findings 1–4 | Normatively resolved before draft 0.5. |
| DeepSeek red team, findings D1–D12 | Normatively resolved before draft 0.5. |
| Red team 03, K3-01–K3-18 | Normatively resolved in draft 0.5; independently checked by red team 04 Part A. |
| Red team 04, K4-01–K4-13 | Unresolved in draft 0.5; consolidated and reclassified below. |
| Red team 05, C5-01–C5-04 | Unresolved in draft 0.5; consolidated and reclassified below. |

“Normatively resolved” does not mean every rule already has an executable
conformance vector. Draft 0.6 still needs the fixtures named below.

## High freeze blockers

### I-01 — Digest-visible arrays lack a normative total order (High, SD)

**Consolidates:** K4-02.

§13.2 says declaration order is “dependency-stable,” while §13.3 makes the
result digest-visible. The term is undefined. A topological graph can have
multiple valid linearizations, and arrays not derived from declarations—such
as extensions, dependencies, capabilities, products, evidence, and
diagnostics—also need an order.

**Required repair:** define a total ordering for every canonical array. For
declarations, specify the complete tie-break tuple after dependency order. For
each other array, name its semantic key and bytewise collation rule. State
whether source reorderings that preserve the dependency graph preserve or
change each digest.

**Required fixture:** `fixture.canonical.all_array_orderings`, including a DAG
with two simultaneously ready nodes and semantically identical documents
constructed from different insertion orders.

### I-02 — Digest-domain membership and typed dependency identities are incomplete (High, SD)

**Consolidates:** K4-03 and the dependency-identity portion of C5-03.

§13.2 contains several digest-named envelope fields, while §13.3 does not give
a complete field-by-field preimage table for `semantic_digest`,
`extension_digest`, and `document_digest`. `dependency_digests` is also an
untyped array, so it cannot distinguish semantic dependency identity from
document/provenance identity. That leaves cache invalidation and provenance
ripple behavior open to backend policy.

**Required repair:** replace prose inference with a normative digest-domain
matrix covering every envelope field. Give dependencies typed records such as
`{module_id, semantic_digest, document_digest?}` and state which identity
enters which digest and cache class. Include unused manifests, evidence,
extensions, compiler identity, and product profile explicitly.

**Required fixture:** `fixture.digest.complete_domain_matrix`, with one-field
mutations and expected equality/inequality for all three primary digests.

### I-03 — Module identity and `module_seed` authoring are unspecified (High, SD)

**Consolidates:** K4-06 and the module-manifest portion of C5-03. **Severity is
raised from Medium to High** because `module_id` and `module_seed` participate
in stochastic stream identity.

§7 defines precedence for a reserved explicit `module_seed`, but the allowed
syntax does not define that declaration. The specification also does not say
whether `module_id` is authored, manifest-assigned, registry-assigned, or
derived from a path. Multi-module promises in §5.3/§5.5 consequently lack a
closed binding contract.

Two compilers can relocate an identical file and either preserve or reroll
every stochastic stream while both claiming conformance.

**Required repair:** choose one stable module-manifest authority. Define the
source or build-request syntax for module identity, the exact reserved seed
declaration, conflict behavior, relocation behavior, exports/import bindings,
and cycle rules. Domain-separate user-visible seed values from the resolved
module seed recorded in IR.

**Required fixture:** `fixture.identity.module_move_and_two_module_binding`,
covering relocation, manifest rename, explicit seed conflict, undeclared
binding, and a dependency cycle.

### I-04 — Product profile is absent from canonical semantic/build identity (High, SD)

**Consolidates:** C5-01.

§11.5 lets a versioned product profile change required products, absence
reasons, collision representation, sockets, LODs, validators, and acceptance
thresholds. The §13.2 envelope and §13.3 digest domains do not identify the
selected profile. Identical semantic/cache keys can therefore authorize
different certified packages.

**Required repair:** add `product_profile_id` and pinned
`product_profile_digest` to the build request and canonical IR. Include the
profile digest in `semantic_digest` and all profile-sensitive assembly and
certification cache keys. Resolve and validate exactly one profile before
construction.

**Required fixture:** `fixture.profile.identity_and_cache_isolation`, comparing
render and gameplay profiles plus two aliases pinned to identical profile
content.

## Medium repairs required before freeze

### I-05 — Mask composition and comparison typing lack one legal surface (Medium, SD)

**Consolidates:** K4-04 and K4-05.

§12.2 requires composed masks, but §5.2 does not admit the named logical
operators. Comparison expressions also lack a single result type, and chained
comparison semantics are unaddressed.

**Required repair:** choose one explicit mask-combinator surface (constructors
are simplest), define comparisons as producing exactly one predicate/mask type,
and either reject chained comparisons with a repair or define their lowering.
Do not inherit Python truthiness.

### I-06 — Constructor positional-operand rule conflicts with examples (Medium, SD + UR)

**Consolidates:** K4-01. **Downgraded from High to Medium.**

§5.4’s keyword-only rule is deterministic, so conforming implementations need
not diverge. The defect is that several normative examples are illegal under
that rule and the intended small-model authoring surface is unclear.

**Required repair:** either rewrite every example to keyword form or publish a
registry-declared positional operand class in constructor signatures. The
latter is reasonable for relational constructors such as `connected` and
`require`, but it must not become arbitrary Python-style positional binding.

### I-07 — Unit-constant arithmetic is not numerically canonical (Medium, SD)

**Consolidates:** K4-07.

§6.3 does not pin whether unit literals are exact decimal/rational values or
binary floats during constant folding. Equivalent spellings can differ by an
ulp before entering digest-visible typed IR.

**Required repair:** define exact parsing and conversion, rounding mode,
overflow behavior, and the canonical numeric representation stored in IR.

### I-08 — Compiler provenance and semantic compatibility identity are conflated (Medium, SD)

**Consolidates:** C5-02.

The raw `compiler_version` is both producer provenance and part of a digest
described as changing only with meaning, while §18 permits non-semantic patch
releases.

**Required repair:** retain exact compiler build/version in provenance and use
a registry-pinned semantic frontend compatibility ID in semantic identity. A
release may reuse that ID only after its valid-program semantics pass the same
conformance suite.

### I-09 — Collection literal equivalence and duplicate map keys are unspecified (Medium, SD)

**Consolidates:** K4-10. **Severity is raised from Low to Medium** because a
duplicate map key can cause silent semantic loss before canonicalization.

**Required repair:** decide whether tuple and list literals are distinct typed
values or normalize to one sequence form. Reject duplicate dictionary keys at
parse/validation time; do not use last-write-wins behavior.

### I-10 — Lenient prototype behavior needs an explicit non-certifiable boundary and migration disposition (Medium, SD + ID + MCT)

**Consolidates:** K4-09 and C5-04. **C5-04 is downgraded from High.**

The live prototype directly applies clamped lenient output in a test, while
§15 correctly forbids lenient output from certification and scaffold
generation. Direct preview application is not itself proof of a certification
contradiction; the earlier High claim was too broad. The missing contract is a
typed boundary preventing preview values from leaking into strict world or
artifact application and a migration disposition for the old test.

**Required repair:** define separate `LenientCandidate` and `StrictIR` result
types. Permit the former only in an explicitly non-certifiable preview sandbox.
Create a migration manifest marking every prototype test `preserve`,
`replace-with-strict-promotion`, or `retire`, with rationale.

## Low precision and fixture work

### I-11 — External identifier safety needs canonical escaping, not a universal lexical grammar (Low, SD + MCT)

**Consolidates:** K4-08. **Downgraded from Medium to Low and narrowed.**

The earlier report conflated language identifiers with identifier-valued
strings derived from evidence. External IDs do not need to obey one source
identifier grammar if scaffold generation uses a canonical escaped string
literal and never emits them as binding names.

**Required repair:** distinguish source binding names, registry identifiers,
and identifier-valued strings. Pin canonical escaping and add hostile Unicode,
quote, newline, and delimiter round-trip fixtures.

### I-12 — Three bounded canonical details remain (Low, SD + MCT)

**Consolidates:** K4-11, K4-12, and K4-13.

- Name the exact hard-constraint target created when a soft objective is
  promoted.
- State that immutable cache entries retain original producer provenance while
  reuse events are appended to the build/receipt log, not the entry.
- Pin the canonical digest vector for an empty extension set.

## Findings merged or rejected as standalone blockers

- **C5-03 is not retained as an independent finding.** Its actionable module
  binding issue is I-03; its digest issue is I-02. A second parallel module
  architecture finding would create conflicting repair authority.
- **C5-04 is not High as previously written.** `apply_intent` may represent a
  non-certifiable preview. The valid defect is the missing typed isolation and
  migration disposition, retained as I-10.
- **K4-01 is not High.** The rule is normatively deterministic; its examples
  and ergonomics conflict. It remains required as I-06.
- **K4-08 does not establish scaffold code injection.** Canonical string
  escaping is sufficient if external identifiers never become source binding
  names. It remains as I-11.
- No additional defect was established merely from choosing Python-shaped
  syntax or heterogeneous execution backends. Those risks are already bounded
  by parsed-only source, typed IR, capability manifests, and certification.

## Draft 0.6 repair sequence

1. **Identity envelope:** I-01 through I-04 together. Ordering, digest
   membership, module identity, dependencies, and profile identity must form
   one internally consistent schema change.
2. **Source and type surface:** I-05 through I-07 and I-09. Pin syntax before
   implementing the parser whitelist.
3. **Compiler compatibility:** I-08 and its cache/conformance vector.
4. **Repair/migration boundary:** I-10 and the prototype-test disposition
   manifest.
5. **Precision pass:** I-11 and I-12.
6. Run all named fixtures plus existing prototype characterization tests. Then
   perform a verification-only review: it should prove each item landed, not
   invent a new architecture.

An exploratory parser or backend spike may proceed behind an explicitly
unstable namespace, but draft 0.5 must not pin the public grammar, IR schema,
digest schema, or stable cache format.

## Execution-backend boundary, including Taichi

Nothing in these findings requires the WGE source language to become an
executable numerical language. The stable boundary should remain:

```text
model-authored WGE source
    -> parsed and validated semantic IR
    -> bounded construction/spatial plan
    -> selected numerical backend (Taichi/Odin/Julia/other)
    -> artifact postchecks and certification
```

Taichi is a strong candidate for parallel field, voxel, SDF, projection,
constraint-loss, and optimization kernels. It is not a replacement for the
model-facing semantic language because executing model-authored Taichi/Python
would violate the parsed-never-executed invariant and would expose backend
execution details as product meaning. Backend choice and capability/determinism
grade belong in the execution manifest; semantic asset and world intent remain
in WGE IR.

## Verification performed

- Read draft 0.5 and all red-team reports through red team 05.
- Rechecked every K4 and C5 finding against the current normative sections.
- Inspected `pipeline/worldbuilder_dsl.py` and migration-relevant tests.
- Ran `python3 -m unittest tests.test_worldbuilder_dsl`: **27 tests passed**.
- Did not edit `WGE_LANGUAGE_SPEC.md`, the prototype compiler, registries, or
  tests.
- Did not execute the proposed new conformance fixtures; they do not exist yet.

