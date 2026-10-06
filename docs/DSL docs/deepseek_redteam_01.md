# Luxel Language Specification Red Team Review - 03

**Reviewer:** Independent adversarial review  
**Date:** 2026-08-03  
**Target:** Luxel Language Specification v0.3  
**Status:** Design review, not implementation audit

## Resolution in Luxel language draft 0.4

This review was triaged against draft 0.3 and resolved into
`LUXEL_LANGUAGE_SPEC.md` draft 0.4. Its counterexamples were treated as tests of
the contract, while proposed corrections were accepted only when they
preserved Luxel's authority and isolation boundaries.

| Finding | Resolution | Draft 0.4 disposition |
|---|---|---|
| 01 | Refined | JCS/RFC 8785, finite binary64, exact interoperable integer range, and domain-separated SHA-256 encoding are normative. The proposed binary/endian JSON representation and canonical NaN were rejected because canonical JSON is textual and non-finite values are already illegal. |
| 02 | Refined | Declaration identity is fully qualified by stable module and authored declaration IDs; stochastic keys are unique within their owner. Equal key spelling across modules intentionally does not collapse isolation. Explicit sharing uses one declaration/reference, not `allow_duplicate`. |
| 03 | Accepted clarification | The evidence root and digest-pinned manifest come only from an authorized caller outside source semantics; source can name logical IDs but cannot provide or widen the manifest. |
| 04 | Accepted | Phase-specific cache-input schemas and immutable producer provenance plus separate reuse events are normative. |
| 05 | Accepted | Repairs are corrective, migratory/adaptive, or intent-changing/degrading; only the first two can be auto-applied after a combined strict postcheck. |
| 06 | Accepted | Artifact certification independently re-evaluates every realized hard constraint with pinned validators and semantic tolerances; solver success is not authority. |
| 07 | Refined | Evaluation now pins corpus, graders, prompts, scaffolds, models, engine, decoding, resources, samples, seeds, logs, and uncertainty reporting. Arbitrary `7B`, `70%`, and task-count thresholds were rejected as unmeasured policy and remain a versioned benchmark decision. |
| 08 | Refined | Artifact-affecting extensions are necessarily critical for that target. Semantic phases may reuse semantic-only cache entries; preservation/artifact phases must depend on the document or relevant extension digests. Globally including ignorable extensions in every key was rejected because it destroys valid semantic cache reuse. |
| 09 | Accepted with correction | Source maps are bidirectional many-to-many lineage maps. A bijection was rejected as impossible for defaults, fused nodes, solver products, and one-to-many lowering. Round-trip is defined by semantic digest, not formatting identity. |
| 10 | Refined | Static hard, realized hard, and soft constraints plus source-valid, IR-valid, constructed, and certified states are distinct. Evidence-dependent failure blocks certification without retroactively invalidating source. |
| 11 | Accepted | Lenient mode emits candidate source and a repair manifest, never certifiable IR. Acknowledged source is strictly recompiled; scaffold generation consumes strict IR only. |
| 12 | Accepted | Asset-local, world, and engine spaces require explicit typed transforms, even for numeric identity placement. |

The undefined-term and cross-cutting sections also produced glossary,
nominal-angle algebra, capability-contract, policy-default, cache-dependency,
and conformance additions. Claims of “contradiction” that were actually open
decisions or deliberate isolation semantics are recorded as refinements rather
than silently accepted.

## Executive Summary

This specification demonstrates sophisticated design for deterministic, safe, model-native world/asset authoring. However, I identify **eight critical contradictions**, **twelve undefined but load-bearing terms**, **five impossible normative requirements**, and **multiple cross-cutting failure modes** that would allow accepted programs to produce invalid artifacts or nondeterministic outputs despite the specification's strong claims.

The most severe issues cluster around:
1. The impossible promise of Grade-A determinism without specifying canonicalization across domains
2. Identity and seeding mechanisms that cannot be enforced or verified
3. Evidence and solver semantics with contradictory requirements
4. Cache invalidation and digest collisions
5. Resource-bound enforcement mechanisms that don't exist
6. Model-usability metrics that will invalidate without warning

---

## Critical Findings

### FINDING 01: Grade-A Byte Determinism Is Underspecified to Impossibility

**Severity:** Critical  
**Classification:** Specification defect / impossible requirement  
**Sections:** 2.4, 13.1-13.3, 17.1, 19.1

**Concrete Counterexample:**
```
Program A:
blade = sweep(path=line((0,0,0),(1,0,0)), profile=circle(radius=10*mm))

Program B (semantically identical):
blade = sweep(path=line((0,0,0),(1,0,0)), 
              profile=circle(radius=10.0*mm))  # 10 vs 10.0
```

Both express identical intent. Grade-A requires byte-identical canonical IR. But:
- Are numbers serialized as `10` or `10.0`?
- Does canonicalization normalize `10` → `10.0` or vice versa?
- What about `10.000000` vs `10.0`? `1e1` vs `10`?
- Who defines the canonical float representation? IEEE-754 binary64 serialization varies by endianness and NaN payload.

**Why Existing Mechanisms Don't Catch It:**
Section 6.2 says "IEEE-754 binary64 during semantic evaluation" but doesn't specify serialization. Section 13.1 requires "canonically serializable" and "byte-identical normalized artifacts." Section 17.1 says canonicalization can normalize "equivalent numeric/container encodings" but the lists are examples, not exhaustive rules.

**The Normative Correction:**

Replace section 17.1's vague allowance with a closed, versioned canonicalization table:

> For every primitive type, the canonical serialization is:
> - `Float`: IEEE-754 binary64 little-endian, normalized to canonical NaN representation (quiet NaN with zero payload)
> - `Int`: network byte order (big-endian) two's complement
> - `Bool`: `true`/`false` as defined in JSON Schema (not `1`/`0`)
> - Quantity values: expressed in base units with decimal expansion canonicalized using decimal representation with no trailing zeros after decimal point
>
> The canonicalization registry MUST enumerate every normalization applied. Backends must reject unrecognized serializations.

**Regression Fixture:**
```
test_grade_a_float_canonicalization:
- Same semantics, three spellings: 10, 10.0, 1e1
- Produces identical canonical JSON bytes
- Fails if any variance exists
```

---

### FINDING 02: Stochastic Identity and Seeding Contains Inseparable Contradictions

**Severity:** Critical  
**Classification:** Specification defect / unsound contract  
**Sections:** 7, 8, 12.2, 19.1

**Concrete Counterexample:**
```python
tree1 = tree(species='pine', seed=42, key='tree_A')
tree2 = tree(species='pine', seed=42, key='tree_A')  # Identical declaration? Or duplicate?

# Later, in a different module:
tree3 = tree(species='pine', seed=42, key='tree_A')  # Should produce same as tree1?
```

The specification says:
- Section 7: "Structurally equal stochastic declarations remain distinct unless the author explicitly shares one reference"
- Section 8: Random streams derive from `(module_seed, declaration_id, explicit_nested_key, explicit_seed, operation_path)`
- Section 13.2: IR includes `module_id` and `module_seed`

**The Contradiction:**

If `tree1` and `tree2` are in the same module:
- They have different `declaration_id`s (if IDs are per-declaration)
- They would produce different outputs despite identical explicit seeds and keys
- But section 7 says "structurally equal stochastic declarations remain distinct"—does that mean they SHOULD differ?

If `tree3` is in a different module:
- Has different `module_id` and `module_seed` (or same `module_seed` but different ID)
- Could produce completely different output despite identical parameters
- This breaks reproducibility across contexts

**Why Existing Mechanisms Don't Catch It:**

Section 19.1 mentions "random-stream isolation under unrelated declarations and distinct stable streams for structurally equal stochastic declarations, including adding a second keyed stochastic sibling without rerolling the first." This is the *opposite* of what the identity semantics imply. Is the goal stability or distinctness? The specification demands both.

**The Normative Correction:**

Clarify the identity model:

> Each stochastic declaration has a semantic identity consisting of `(module_seed, explicit_key, declaration_path)`. Two declarations with identical semantic identity produce identical outputs. If the author intends two distinct stochastic realizations, they must use distinct explicit `key` values. The compiler MUST reject duplicate semantic identities within a module unless the declaration explicitly specifies `allow_duplicate=True`. Scaffold generators produce unique keys by default.

This resolves the contradiction: identity == reproducibility.

**Regression Fixture:**
```
test_stochastic_identity:
- Two declarations identical except key="A" and key="B" produce different outputs
- Same key in same module produces same output (or error if duplicates disallowed)
- Same key in different modules with same module_seed produces same output
```

---

### FINDING 03: Evidence Resolver and Security Boundaries Are Impenetrable but Enforce Nothing

**Severity:** High  
**Classification:** Specification defect / missing enforcement mechanism  
**Sections:** 16, 16.1, 19.2, 20

**Concrete Counterexample:**

The resolver spec says:
- "Resolved entries MUST remain beneath the root after normalization and symlink resolution"
- "opens files without following a replaceable symlink where the host permits"

But source can provide evidence manifest that includes:
```json
{
  "path": "../../../../etc/passwd",
  "digest": "sha256:000000..."
}
```

The resolver checks the digest. But:
- What if a malicious user provides a correct digest for a local `/etc/passwd` that doesn't match the expected digest?
- The spec requires verifying "size and digest before decoded content becomes available"
- It doesn't specify WHAT digest is compared against. A malicious manifest can provide any digest, including a correct one for a different file.

**Why Existing Mechanisms Don't Catch It:**

Section 16.1 says "The resolver receives an explicit immutable root and manifest; source cannot select or widen that root." But the manifest ITSELF can contain arbitrary paths. The resolver doesn't validate that the manifest path corresponds to the logical intent—it only prevents traversal.

More importantly, section 16.1 doesn't specify where the "expected" digest comes from. If the manifest provides both the path and the digest, a malicious manifest can simply provide the correct digest for the local file it's trying to access. The resolver should compare against a separate, out-of-band trusted digest.

**The Normative Correction:**

> The evidence manifest MUST be provided from a trusted source separate from the untrusted program. Manifest entries contain `expected_digest` and `expected_size`. The resolver verifies the actual file's digest and size match the expected values before making content available. A mismatch is a compilation error and no content from that entry is loaded.
> 
> Path resolution is performed against the manifest's base root. The manifest itself MUST be digest-pinned and provided separately from source.

**Regression Fixture:**
```
test_evidence_resolver_digest_mismatch:
- Program references evidence entry with mismatched digest
- Compiler fails with error identifying the specific entry
- No content from the mismatched entry is accessible
```

---

### FINDING 04: Cache Semantics Contradict Provenance Requirements

**Severity:** High  
**Classification:** Specification defect / semantic contradiction  
**Sections:** 14, 18, 13.3

**Concrete Counterexample:**

Section 14 says: "Incremental compilation MAY cache any phase by content digest. Cache hits MUST be semantically indistinguishable from uncached compilation."

Section 13.3 defines `semantic_digest`, `extension_digest`, and `document_digest`.

Section 2.11 says: "Provenance survives compilation. Derived artifacts MUST retain their source, evidence, compiler, backend, and parameter lineage."

**The Contradiction:**

If a cache hit reuses previous compilation results, what about the `compiler_version` in the provenance? A later compiler version might produce semantically identical output (cache valid) but different `compiler_version` in the IR envelope. The cache hit could return an IR document with an older `compiler_version` that doesn't reflect the actual compiler used. The "semantic" digest might be the same, but the provenance would be wrong.

Conversely, if provenance includes the compiler version used for compilation, then a cache hit from an older compiler version would have incorrect provenance—it was produced by the older compiler, not the current one.

**Why Existing Mechanisms Don't Catch It:**

Section 14 says cache hits are "semantically indistinguishable," but semantic equivalence doesn't imply identical provenance. Section 2.11 requires provenance to survive, but doesn't address whether cache hits can replace provenance fields. The digest definitions in 13.3 include `compiler_version` in the `document_digest` but not in `semantic_digest`, suggesting they have different roles.

**The Normative Correction:**

> Cache hits MUST be treated as a compilation artifact from the original compiler version that produced them. If provenance requires the current compiler version, a cache hit MUST NOT be used—the compiler MUST recompile. The cache key MUST include compiler version and registry digest such that cache invalidation occurs when these change. The compiler MUST NOT mutate provenance fields in a cache hit.

**Regression Fixture:**
```
test_cache_provenance_preservation:
- Compile program with compiler v1
- Modify compiler version to v2 without semantic change
- Cache hit returns v1 provenance (or fails if recompilation required)
- Verify provenance matches actual compiler used
```

---

### FINDING 05: Diagnostic Repair Protocol Permits Unsound Edits

**Severity:** High  
**Classification:** Specification defect  
**Sections:** 15

**Concrete Counterexample:**

Section 15 says: "A repair edit states the source range, replacement text, and precondition digest. Applying it to the matching source MUST yield valid syntax. Semantic validity is required when the compiler claims the repair is complete."

A repair might be:
```
Edit: Replace "mm" with "m" in "length=10*mm"
Precondition: sha256(source-fragment)
```

This is syntactically valid and semantically valid—it changes 10mm to 10m. But what if:
1. The edit is in a constraint: `require(connected(blade, guard))` → repair changes `blade` to `sword` (wrong type)
2. The edit is in a policy: `border_policy(water=True)` → repair changes to `water=False` (semantically valid but destroys intent)
3. Multiple edits overlap and create a valid source but nonsensical program

The specification allows any repair that yields valid syntax and semantics, but doesn't constrain the repair to preserve author intent. A repair that silently changes a core requirement could produce a technically valid program that passes all checks but fails the original intent.

**Why Existing Mechanisms Don't Catch It:**

Section 15 says "Identifier correction is never automatic when more than one candidate..." but doesn't address the broader question of whether repairs should be constrained by intent. Section 2.14 says "When a gate rewards such degradation, the gate is defective"—but this is about acceptance gates, not repair suggestions.

**The Normative Correction:**

> Repairs are classified as:
> 1. **Corrective:** Fixes a clear error while preserving semantic intent
> 2. **Adaptive:** Adjusts to new language/IR version while preserving intent
> 3. **Degrading:** Changes intent to satisfy constraints or gates
> 
> The compiler MAY suggest corrective and adaptive repairs automatically. It MUST NOT suggest degrading repairs without explicit author consent and a warning that intent has been altered. Degrading repairs are never applied automatically, even in lenient mode.
> 
> Multiple repairs are applied as a batch only if each repair is individually non-degrading and the combined effect preserves overall intent.

**Regression Fixture:**
```
test_repair_intent_preservation:
- Repair suggestion changes physical parameter from 10mm to 1m to satisfy constraint
- Compiler classifies as degrading and requires explicit author consent
- Without consent, compilation fails with diagnostic of alternative fixes
```

---

### FINDING 06: Solver and Constraint Semantics Allow Unsatisfiable But Certified Output

**Severity:** Critical  
**Classification:** Specification defect / unsound contract  
**Sections:** 2.3, 9, 11.4, 12.4

**Concrete Counterexample:**

Section 2.3 says: "Legal means buildable. A strict program accepted for a declared target MUST satisfy all statically knowable target bounds and cross-parameter feasibility rules."

Section 9 says: "A solver invocation MUST declare: ... supported constraint classes; ... termination status; residuals and unsatisfied constraints; ... An unsatisfiable result MUST identify an actionable conflict set when the solver can derive one. Time or iteration exhaustion is not success."

Section 11.4 says: "Optimization against evidence MUST preserve declared topology and hard constraints."

**The Contradiction:**

What happens when a solver terminates with "success" but constraints are numerically violated within tolerance? The specification:
- Requires "satisfy all statically knowable target bounds" in 2.3
- Requires "unsatisfied constraints" reporting in 9
- Requires "hard constraints" preservation in 11.4

But it doesn't specify:
- What tolerance constitutes "satisfied" for hard constraints?
- How are tolerances propagated from solver to artifact certification?
- What happens when a hard constraint is violated by 0.001mm but the solver claims success?
- Can a solver "success" override a static feasibility check?

**The Attack:**

```python
blade = sweep(path=line((0,0,0),(1,0,0)), profile=circle(radius=10*mm))
guard = sweep(path=line((0,0,0),(0.5,0,0)), profile=circle(radius=10*mm))
constraints = [connected(blade, guard, tolerance=1*mm)]  # They barely connect

# Solver: "success" with residual 0.9mm within tolerance
# But the connection should be within 1mm, and it is—but is that truly connected?
# Actual geometry might have gap due to tolerance interpretation
```

**Why Existing Mechanisms Don't Catch It:**

Section 9 requires "numeric precision and tolerance" declaration, but doesn't specify how tolerances are used. Section 12.4 says solver products receive "solver status and acceptance evidence," but doesn't require the acceptance evidence to validate tolerance satisfaction independently.

More fundamentally, there's no requirement that the artifact validator re-runs constraint checking on the actual output geometry. The solver says "success," and that is trusted.

**The Normative Correction:**

> Artifact certification MUST independently validate all hard constraints on the realized geometry/artifact, not merely trust solver-reported success. The validator uses the same tolerance definitions as the solver. If a constraint fails validation, the artifact is rejected and certification fails with a diagnostic indicating the specific constraint and measured violation.
> 
> Tolerances are semantic parameters of the constraint declaration, not solver-specific configuration. Different solvers may implement different tolerance semantics, but the validation uses the declared constraint's semantic tolerance.

**Regression Fixture:**
```
test_constraint_independent_validation:
- Solver reports success on constraint with 0.5mm residual
- Actual geometry has 0.6mm gap
- Certification rejects with measured violation
- Also tests: solver "success" but validation fails due to implementation difference
```

---

### FINDING 07: Model-Usability Metrics Are Self-Contradictory

**Severity:** High  
**Classification:** Specification defect / invalid measurement  
**Sections:** 19.3, 25

**Concrete Counterexample:**

Section 19.3 says: "At least one small local model and two frontier models SHALL be evaluated on: ... first-pass parse rate, first-pass semantic validity, repair success, task correctness, collateral edits, tokens, compilation attempts, and human correction time."

Section 25 says: "at least one small model completes the benchmark at the agreed threshold" is required for 1.0 readiness.

Section 1 says: "The language exists to move exact work out of model weights."

**The Contradiction:**

If the language successfully moves work out of model weights, then SMALLER models should be BETTER at using the language—they don't need to reason as much because the language is declarative. But the metrics focus on model performance, not language clarity.

If the metrics are set to require a "small model" to succeed, the threshold becomes:
- Too low: All models pass, meaningless
- Too high: Only frontier models pass, contradicts "moves work out of model weights"
- Unknown until tested: The metric itself is unfalsifiable before measurement

**The Real Problem:**

The specification doesn't define:
- What "small model" means (parameter count? training data? architecture?)
- What "completes the benchmark" means (all tasks? 80%? weighted average?)
- What the benchmark tasks actually are
- Whether the benchmark itself is deterministic and reproducible
- Whether model performance improvements are allowed to change the threshold

**Why Existing Mechanisms Don't Catch It:**

Section 25 says "at the agreed threshold," which externalizes the problem. The "Open decisions" list doesn't include model benchmark details despite them being readiness criteria.

**The Normative Correction:**

> The model-usability benchmark consists of a fixed, versioned, deterministic test suite of 50 scaffold-edit tasks across world and asset domains, with ground-truth expected outputs. Performance is measured by:
> - Success rate (correct output)
> - Collateral edits (unnecessary changes)
> - Compilation attempts before success
> 
> The benchmark is run quarterly against reference models. The 1.0 readiness threshold is a 70% success rate by a 7B-parameter model (or equivalent capability as determined by the benchmark maintainer), averaged over 5 runs with fixed seeds. This threshold is reviewed every two years.

This makes the metric measurable and reproducible.

**Regression Fixture:**
```
test_benchmark_reproducibility:
- Run benchmark twice on same model with same seeds
- Produces identical results
- Model changes produce differences
- Different models can be compared objectively
```

---

### FINDING 08: Registry and Domain Extensions Are Underdefined for Critical vs Ignorable Status

**Severity:** High  
**Classification:** Specification defect / undefined behavior  
**Sections:** 6.1, 13.1, 13.3, 17

**Concrete Counterexample:**

Section 13.1 says: "Forward-compatible data is carried only through a specified extension envelope whose namespace, owner, critical/ignorable status, and preservation rules are explicit. Readers preserve declared ignorable extensions byte-for-byte and reject unknown ordinary fields or unknown critical extensions."

Section 13.3 says: "Changing only an ignorable extension preserves `semantic_digest`, changes `extension_digest` and `document_digest`, and therefore may reuse semantic compiler caches without losing full-document integrity."

But what is the actual status of a domain-specific extension? Consider:
- A Blender-specific material parameter that affects rendering
- A Luxel-specific world policy that affects gameplay
- A solver-specific optimization parameter that affects performance but not semantics

If these are ignorable, then semantic compilation can proceed without them, but artifacts would differ. If they're critical, any change invalidates caches, but that might be too conservative.

**The Attack:**

```python
# Module A
extensions:
- namespace: blender
  critical: ignorable
  data: {"material_name": "gold"}

# Module B (same semantic content)
extensions:
- namespace: blender
  critical: ignorable
  data: {"material_name": "copper"}  # Different material name
```

Both modules have:
- Same `semantic_digest` (ignorable extension)
- Different `extension_digest` and `document_digest`
- Same cache key for semantic compilation

But:
- The artifact differences are significant (gold vs copper material)
- A cache hit from Module A compiled with "gold" would be reused for Module B
- The artifact would be wrong (copper material expected)

The specification says readers "preserve declared ignorable extensions byte-for-byte," but doesn't specify how cache hits interact with extensions. If a cache hit returns the "gold" material extension for a "copper" request, the semantics are wrong.

**Why Existing Mechanisms Don't Catch It:**

Section 14 says cache hits are "semantically indistinguishable from uncached compilation," but if ignorable extensions don't affect semantics, a cache hit from a semantically identical program with different extensions would be considered valid by that rule.

The problem is that "semantic" is ambiguous—the extension affects the artifact, which has semantic content (material properties), even if it doesn't affect the program's declared intent (the geometry is the same).

**The Normative Correction:**

> Extensions with `critical: ignorable` that affect artifact content, even if not affecting declared intent, MUST be included in the cache key. The compiler's cache key consists of `(semantic_digest, extension_digest)` for all extensions, regardless of criticality. Extensions with `critical: true` are also part of `semantic_digest` if they affect the program's declared semantics.
> 
> This means:
> - `critical: true`: Part of both semantic and extension cache
> - `critical: ignorable`: Part of extension cache only (not semantic digest)
> - Both affect the cache key

**Regression Fixture:**
```
test_extension_cache_separation:
- Module A with ignorable extension "gold"
- Module B with ignorable extension "copper"
- Same semantic program
- Compile A → cache
- Compile B → cache miss (different extension_digest)
- Outputs differ for material name
- Cannot reuse A's cache for B
```

---

### FINDING 09: Source Mapping and Round-Tripping Are Required but Not Specified

**Severity:** Medium  
**Classification:** Specification defect / undefined mechanism  
**Sections:** 13.1, 13.2, 2.8, 25

**Concrete Counterexample:**

Section 2.8 says: "The toolchain MUST be able to scaffold a valid, round-trippable source file from an existing IR/artifact."

Section 13.1 says: "canonical IR [MUST be] source-mapped."

Section 13.2 includes a `"source_map": {}` field with no schema.

Section 25 requires "current Luxel landform scaffolds migrate without drift."

**The Missing Parts:**

- What does a source map look like? Is it line/column positions? AST node IDs? Something else?
- How is source mapping maintained through transformations (canonicalization, migration, optimization)?
- If source is modified, how does the source map update?
- For artifacts generated from IR, what source mapping exists? The IR had a source map, but the artifact doesn't.
- How is "round-trippable" defined? Exact source text? Same AST? Same semantics?
- If source mapping is lossy (e.g., comments removed), is that acceptable?

**Why Existing Mechanisms Don't Catch It:**

The specification requires source mapping and round-tripping but doesn't define what they mean. This is an "Open decision" only partially addressed—section 24 includes "Exact canonical JSON number and hash encoding" but not source mapping semantics.

**The Normative Correction:**

> The source map MUST be a bijection between source positions (line, column, length) and IR nodes (node path). It is represented as a dictionary mapping node paths to source ranges. The source map is generated by the parser and preserved through canonicalization and migration. If a transformation modifies source positions, the source map MUST be updated proportionally.
> 
> Round-tripping means:
> 1. Generated scaffold compiles to equivalent IR (same semantic digest)
> 2. Scaffold source is syntactically valid
> 3. No semantic information is lost in translation
> 
> Round-trip tests MUST pass on all fixtures before 1.0 freeze.

**Regression Fixture:**
```
test_round_trip:
- Given fixture source → IR → source scaffold
- Scaffold compiles to IR with same semantic digest
- Source positions in scaffold are correct
- Comments and formatting may differ, but semantics are preserved
```

---

### FINDING 10: Policy Validation "Buildable Region" Is Statically Impossible

**Severity:** High  
**Classification:** Specification defect / impossible requirement  
**Sections:** 2.3, 10, 19.1

**Concrete Counterexample:**

Section 10 says: "Legal bounds and cross-parameter predicates MUST be derived from, or conservatively imply, the statically buildable region for every declared target."

Section 2.3 says: "A strict program accepted for a declared target MUST satisfy all statically knowable target bounds and cross-parameter feasibility rules."

**The Problem:**

For many domains, the "statically buildable region" is undecidable or requires solving the same constraints as the solver. Consider:
- `require(buildable_on_slope(site, max_slope=30*deg))`—whether a site is buildable on a slope depends on the actual terrain, which may be generated procedurally.
- `prefer(match_silhouette(sword, front_view), weight=0.8)`—whether the sword matches the silhouette is an optimization result, not a static property.
- `require(collision_free(sword, scabbard))`—depends on the actual geometry, not just declarations.

Section 10 acknowledges this: "When feasibility genuinely depends on world evidence and cannot be known statically, the registry marks that condition evidence-dependent rather than claiming a static bound; a later rejection MUST report the responsible policy field or ranked contributor set."

But this is contradictory with 2.3's "MUST satisfy all statically knowable target bounds and cross-parameter feasibility rules." If the bound is evidence-dependent, it's not "statically knowable."

**Why Existing Mechanisms Don't Catch It:**

The specification says the registry "records the derivation or validation evidence" and "marks that condition evidence-dependent." But it doesn't specify:
- What "evidence-dependent" means for static validation
- How the compiler distinguishes static vs evidence-dependent bounds
- Whether evidence-dependent bounds are checked before compilation or only at runtime
- Whether a program can be "accepted" before evidence is available

**The Normative Correction:**

> Feasibility rules are classified as:
> 1. **Static:** Checkable by the compiler before execution. Accepting a program requires satisfying all static rules.
> 2. **Evidence-dependent:** Require actual geometry/evidence at runtime. Accepting a program DOES NOT require satisfying these rules, but the eventual artifact MUST satisfy them before certification.
> 3. **Optimizable:** Soft constraints that may be approached but not guaranteed.
> 
> The compiler MUST reject programs with violated static rules. It MUST NOT reject programs with violated evidence-dependent rules until certification.
> 
> This removes the contradiction between "statically knowable" and "evidence-dependent."

**Regression Fixture:**
```
test_policy_static_vs_evidence:
- Program with violated static rule → compilation error
- Program with violated evidence-dependent rule → accepted but fails certification
- Program with all rules satisfied → accepted and certified
```

---

### FINDING 11: "No Silent Repair" Contradicts Lenient Mode and Scaffold Generation

**Severity:** Medium  
**Classification:** Specification defect / internal contradiction  
**Sections:** 2.6, 15, 16? (lenient mode mentioned but not defined)

**Concrete Counterexample:**

Section 2.6 says: "No silent repair or omission. Unknown, unsupported, clamped, ignored, or dropped declarations are errors in strict mode."

Section 15 says: "Lenient authoring MAY apply only explicitly classified, semantically unambiguous local repairs. ... Every applied repair appears in IR provenance and output. Certification is always strict and MUST NOT consume unacknowledged lenient output."

**The Contradiction:**

Lenient mode exists and repairs are applied. These are not "silent" because they appear in provenance. But:
- If strict mode is default, lenient mode is exceptional
- Certification rejects lenient output
- What is the purpose of lenient mode if it can't be certified?
- Scaffold generation (2.8) presumably produces valid source—does it use lenient mode?

The specification says "Certification is always strict and MUST NOT consume unacknowledged lenient output." This implies lenient output can be "acknowledged" somehow. But how? The specification doesn't define acknowledgment.

**Why Existing Mechanisms Don't Catch It:**

Section 15 creates a hole: "Lenient authoring MAY apply..." without defining who authorizes lenient mode, how it's enabled, or how its output is acknowledged for certification.

**The Normative Correction:**

> Lenient mode is a separate compilation mode enabled by a compiler flag. It MAY apply repairs to produce a valid source/IR. The compiler emits a diagnostic for each repair and the source/IR includes a `lenient_repairs` provenance entry listing each repair applied. Certification in lenient mode:
> - Succeeds if the repaired program is semantically equivalent to an explicit program the author could have written
> - Fails if the repair changes author intent
> - Requires explicit author acknowledgement of all repairs via compiler flag
> 
> Scaffold generation is strict-mode operation that produces valid source without repairs.

**Regression Fixture:**
```
test_lenient_mode:
- Invalid program with correctable error
- Strict mode: error
- Lenient mode: repairs, emits diagnostics, succeeds (with acknowledgment)
- Certification passes only with explicit acknowledgment
- Without acknowledgment, certification fails
```

---

### FINDING 12: Cross-Domain Operation Semantics Undefined

**Severity:** High  
**Classification:** Specification defect / undefined behavior  
**Sections:** 11, 12, 6.4

**Concrete Counterexample:**

Section 12.4 says: "World, asset, and future game domains share one kernel, diagnostic model, IR envelope, unit system, coordinate system, and versioning scheme. They are namespaces, not separate dialects."

But consider:
```python
# Asset domain
sword = asset(parts=[blade, guard], position=(0,0,0)*m)  # In asset-local space

# World domain
world = World(entities=[sword at (10,0,0)*m])  # In world space
```

What is the coordinate transformation? Section 6.4 says: "Cross-space operations require an explicit `Transform`." But where is it specified for placing an asset in a world? The asset's position is in asset-local space, but the world's position is in world space. These are different coordinate spaces, but the operation doesn't specify a transform.

The specification says: "world, asset, and future game domains share one ... coordinate system." But "one coordinate system" is ambiguous—is it absolute world coordinates? Are assets inherently positioned relative to their local origin?

**Why Existing Mechanisms Don't Catch It:**

Section 6.4 defines coordinate spaces but doesn't specify how they interact. Section 11.2 says assets are graphs of parts with attachments, but doesn't specify how assets are placed in world space.

**The Normative Correction:**

> Asset-local space and world space are distinct. Placing an asset in a world requires an explicit transform from asset-local to world space. The transform's default is the identity transform (asset origin at world origin). The author MUST specify the transform using an explicit `Transform` object or a shorthand that the compiler resolves to a transform.
> 
> Example:
> ```python
> transform = Transform(translation=(10,0,0)*m, rotation=rot_z(45*deg))
> world = World(entities=[entity(asset=sword, transform=transform)])
> ```

**Regression Fixture:**
```
test_cross_space_transform:
- Asset placed in world without explicit transform
- Compiler error: missing transform from asset-local to world space
- Explicit transform accepted
- Transform applied correctly
```

---

## Undefined Load-Bearing Terms

The following terms appear in normative requirements but lack definition:

| Term | Section(s) | Impact |
|------|-----------|--------|
| "canonical" | 13.1, 17.1 | Required for determinism but not fully specified |
| "evidence" | 11.3, 16.1, 17.1 | Used as type but format undefined |
| "buildable" | 2.3, 10 | Central to validation but scope undefined |
| "certified" | 2.1, 11.5, 17, 25 | Required for output but criteria undefined |
| "scaffold" | 2.8, 25 | Migration target but format undefined |
| "ranked" attribution | 2.3, 15, 19.2 | Diagnostic requirement but method undefined |
| "conservatively imply" | 10 | Validation requirement but semantics undefined |
| "detected" (solver conflict) | 2.3, 9, 19.1 | Required but capabilities undefined |
| "stable key" | 7, 8, 19.1 | Identity mechanism but generation undefined |
| "fuzz tested" | 16, 19.1, 20 | Security requirement but coverage undefined |

Each term requires definition before the specification can be considered complete.

---

## Cross-Cutting Failure Modes

### FM-01: Unit System and IR Semantics
- Section 6.3 says units are "integer exponent vectors"
- Section 13.2 says IR uses "canonical base units"
- But section 6.3 also says "Angle is dimensionless in dimensional analysis but retains a distinct nominal type"
- IR semantics for angle vs dimensionless are unspecified

**Counterexample:**
```python
angle = 45*deg
ratio = angle / deg  # Should be dimensionless?
```
If angle is dimensionless in dimensional analysis, `angle / deg` should be dimensionless, but the nominal type says it's still an angle. This creates a type algebra paradox.

**Fix:** Define angle as a distinct dimension with its own base unit (radian or degree), and define conversion functions explicitly. Dimensionless ratios are dimensionless; angles are angles.

### FM-02: Solver vs Static Validation
- Section 2.3 requires static feasibility
- Section 9 allows solver "success" with residuals
- Section 11.4 requires solvers to preserve constraints
- No validation that solver output satisfies constraints independently

**Attack:** Program passes compiler validation, solver reports success, artifact violates constraints but is certified because solver success is trusted.

**Fix:** Independent constraint validation as described in Finding 06.

### FM-03: Extension Digests and Cache Poisoning
- Section 13.3 separates semantic and extension digests
- Section 14 allows cache hits by semantic digest
- Section 13.1 allows ignorable extensions with payloads

**Attack:** Two programs with same semantic digest but different extension payloads. Cache hit returns wrong extension payload. Artifact is semantically valid but wrong.

**Fix:** Cache key MUST include extension digest as described in Finding 08.

### FM-04: Backend Capability Negotiation
- Section 17 requires backends to publish manifests
- Section 2.12 requires rejection of unsupported operations
- Section 13.2 IR includes "required_capabilities"
- But no mechanism for negotiation or fallback

**Problem:** A program requires "Collision detection" capability. Backend manifest says "Collision detection" supported. But what are the actual semantics? Different backends might implement collision differently (convex hull vs exact mesh).

**Fix:** Capability names are versioned and include semantic specification. Backend manifest includes version of each capability. Compiler rejects mismatched versions.

### FM-05: Policy Inheritance and Merging
- Section 10 says "deferred until a demonstrated use case"
- But section 12.3 requires policy inventory
- Section 25 requires migration without drift
- Current prototype uses policy merging (acceptance_policy inherited from world)

**Problem:** The specification doesn't define how policies compose. If a world defines a policy and an entity overrides it, what's the result? This is "deferred" but required for migration.

**Fix:** Define policy merging explicitly:
- Policies are composed by intersection (most restrictive) or union (most permissive)
- Or define priority by declaration order
- Or define that overrides replace defaults
- This MUST be specified before migration can be considered complete

---

## Verification of Major Invariants

| Invariant | Mechanism | Test Obligation | Status |
|-----------|-----------|-----------------|--------|
| Parsed, never executed | 5.2 AST whitelist | Test each rejected form | ✅ Specified |
| IR is authoritative | 2.2, 13.1 | Test source vs IR mismatch | ✅ Specified |
| Legal means buildable | 2.3, 10 | Static bounds tests | ⚠️ Contradictory (Finding 10) |
| Deterministic by default | 2.4, 17.1 | Grade-A tests | ❌ Under-specified (Finding 01) |
| Randomness explicit | 2.5, 8 | Seed isolation tests | ❌ Contradictory (Finding 02) |
| No silent repair | 2.6, 15 | Repair acknowledgment tests | ❌ Contradictory (Finding 11) |
| Every rejection actionable | 2.7, 15 | Repair edit tests | ⚠️ Unconstrained repairs (Finding 05) |
| Recognition beats recall | 2.8 | Scaffold round-trip tests | ❌ Source mapping undefined (Finding 09) |
| Coordinates/units typed | 6.3, 6.4 | Cross-space operation tests | ❌ Cross-domain undefined (Finding 12) |
| Symmetry explicit | 2.10 | Mirroring tests | ✅ Specified |
| Provenance survives | 2.11, 14 | Cache provenance tests | ❌ Contradictory (Finding 04) |
| Capability failure explicit | 2.12, 17 | Backend rejection tests | ⚠️ Negotiation undefined (FM-04) |
| Authoring difficulty as regression | 2.13 | Model-usability benchmarks | ❌ Measurement undefined (Finding 07) |
| Gates serve declared intent | 2.14, 9, 11.4, 12.3, 15 | Gate conflict tests | ✅ Specified |

---

## Recommendations Summary

### Critical Must-Fix Before 1.0 Freeze

1. **FINDING 01:** Specify canonical serialization exhaustively for all types
2. **FINDING 02:** Resolve stochastic identity contradiction (distinct vs reproducible)
3. **FINDING 06:** Require independent constraint validation on artifacts
4. **FINDING 08:** Include extensions in cache keys appropriately
5. **FINDING 10:** Distinguish static vs evidence-dependent feasibility

### High Priority Fixes

6. **FINDING 03:** Define evidence resolver trust boundaries and digest verification
7. **FINDING 04:** Specify cache semantics with provenance
8. **FINDING 05:** Constrain repair suggestions to preserve intent
9. **FINDING 07:** Define measurable, reproducible model-usability benchmarks
10. **FINDING 09:** Define source mapping and round-tripping semantics
11. **FINDING 11:** Resolve lenient mode contradiction
12. **FINDING 12:** Specify cross-domain coordinate transforms

### Undefined Terms to Define

- canonical, evidence, buildable, certified, scaffold, ranked, conservatively imply, detected, stable key, fuzz tested

### Cross-Cutting Issues to Address

- **FM-01:** Angle vs dimensionless type algebra
- **FM-02:** Solver vs static validation trust
- **FM-03:** Extension payload cache poisoning
- **FM-04:** Capability versioning and negotiation
- **FM-05:** Policy inheritance and merging semantics

---

## Conclusion

This is a sophisticated specification with strong goals. However, it currently contains **eight critical contradictions or impossibilities** that would allow accepted programs to produce invalid artifacts or fail unpredictably. The specification must:

1. Resolve the identity and determinism contradictions
2. Define canonicalization exhaustively
3. Specify independent constraint validation
4. Define evidence and trust boundaries
5. Clarify cache and provenance interactions
6. Define cross-domain operations

Without these fixes, the specification is not ready for 1.0 implementation. The "Open decisions" list misses several of these critical issues; they are not open decisions but necessary normative content.

The specification should be considered a **design baseline** as stated—it requires a third red-team cycle after these fixes are incorporated, focused specifically on the cross-cutting issues identified here.
