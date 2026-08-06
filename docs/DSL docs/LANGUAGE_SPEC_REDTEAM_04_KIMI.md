# Red team 04 — WGE model-native language specification (Kimi/Cursor)

**Reviewed document:** `WGE_LANGUAGE_SPEC.md`, draft 0.5, dated 2026-08-03  
**Review date:** 2026-08-03  
**Reviewer identity:** cursor (Kimi K3), fourth-round follow-up to
`LANGUAGE_SPEC_REDTEAM_03_KIMI.md`  
**Method:** full read of draft 0.5; verification of every K3 resolution against
the normative body (not the changelog); adversarial pass over the text that is
new or rewritten in 0.5; cross-check against `pipeline/worldbuilder_dsl.py`
as the migration baseline. No code was executed and no fixtures were run.

---

## Executive verdict

Draft 0.5 is the first draft where the previously accumulated Critical debt is
actually paid in normative text. All eighteen K3 findings are resolved in the
body, not just claimed in the changelog, and the §25 freeze-gate language this
reviewer recommended was adopted verbatim in spirit.

However, the 0.5 rewrite introduced new defects of the same species it was
fixing, and two of them sit squarely inside the adopted freeze gate
("digest domains" and "parser literal/operator typing"):

1. The spec's own illustrative source code is illegal under its own §5.4
   keyword-only rule (K4-01).
2. "Dependency-stable" ordering — load-bearing for `semantic_digest` and
   Grade-A byte determinism — is used twice and defined nowhere (K4-02).
3. The digest-domain membership of the envelope's *other* digest fields
   (`evidence_manifest_digest`, `dependency_digests`, and the ambiguous
   phrase "the digest fields") is unspecified, and one consistent reading
   contradicts the unused-manifest-entry cache-stability rule (K4-03).
4. §12.2 names mask combinators (`and`, `or`, `xor`, `not`) that cannot be
   spelled in the §5.2 syntax subset at all — `xor` is not even a Python
   token (K4-04).

None of these require architectural change. All of them require normative
text before Codex freezes the IR schema or the parser whitelist, because each
one would otherwise be decided silently by the first implementation — exactly
what §24's closing sentence forbids.

**Recommendation:** cut a draft 0.6 containing K4-01 through K4-06 before the
semantic-compiler kernel work pins the parser surface or any digest preimage.
K4-07 through K4-13 may land in parallel with early implementation.

---

## Part A — verification of round-three resolutions

Statuses are against draft 0.5 **body text**. "Resolved" means the normative
rule exists, is internally consistent, and has a named conformance hook in
§19.

| K3 ID | Title (short) | Draft 0.5 status | Notes |
|---|---|---|---|
| K3-01 | `document_digest` preimage self-contradicts | **Resolved** | §13.3: only `document_digest` omitted from its own preimage; semantic/extension digests are ordinary included fields; §19.1 pins a vector. Residual ambiguity about *other* digest-named fields is split out as K4-03. |
| K3-02 | `operation_path` / stream axes | **Resolved** | §8 pins the closed 4-tuple `(module_seed, fully_qualified_declaration_id, stochastic_key, explicit_seed_or_absent)` and explicitly excludes ordinals, sibling positions, spans, and program digests. §19.1 has the no-reroll fixtures. |
| K3-03 | Evidence pins vs `semantic_digest` | **Resolved** | §11.3 + §13.2/§13.3 define `semantic` vs `construction` pin classes, digest participation, and mutation behavior, with fixtures in §19.1. |
| K3-04 | Forward-reference OD contradiction | **Resolved** | §5.3 forbids forward references normatively; the open decision is gone; rejection is a §19.1 fixture. |
| K3-05 | `null` vs `None` | **Resolved** | §5.2 final paragraph: `None` is the sole spelling; `null` is an unresolved name with a suggested repair. Fixture named in §19.1. |
| K3-06 | `Int / Int` result type | **Resolved** | §6.2: `/` always produces binary64 `Float`; `//` and `%` rejected; division by zero is a semantic-evaluation error. Fixture in §19.1. |
| K3-07 | Negative zero | **Resolved** | §6.2: `+0.0 == -0.0`, JCS encodes both as `0`, hashes over JCS bytes, sign-of-zero-dependent operations excluded from the constant-evaluation vocabulary. |
| K3-08 | Out-of-range source `Int` | **Resolved** | §6.2: rejected during type checking; no silent float escape; negation evaluated before the range check. Fixture in §19.1. |
| K3-09 | `module_seed` resolution | **Resolved** | §7: three-step precedence with explicit conflict failure and package-seed domain separation. Residual: the *syntax* of the reserved declaration is unspecified (folded into K4-06). |
| K3-10 | Feasibility classes not closed | **Resolved** | §9: closed four-label set; backend-local labels forbidden; fixture in §19.1. |
| K3-11 | Extension dependencies | **Resolved** | §13.1: extension records self-contained; inter-extension dependency graphs deferred in §22; fixture rejects implicit dependencies. |
| K3-12 | Positional-operand OD contradiction | **Resolved** | §5.4: keyword-only with one reserved identity/key positional slot, declared per constructor signature. New contradiction with the spec's own examples is K4-01. |
| K3-13 | Symlink non-follow escape hatch | **Resolved** | §16.1: race-resistant descriptor-relative resolution required; targets that cannot provide it must reject the production resolver capability; race fixture in §19.1. |
| K3-14 | "When applicable" package hole | **Resolved** | §11.5: versioned product profiles with closed required/optional sets and typed absence reasons; §19.2 fixtures. |
| K3-15 | Shadowing/import collision | **Resolved** | §5.5: one immutable namespace, idempotent re-import of the same symbol, error with rename repair otherwise; fixtures in §19.1. |
| K3-16 | Defaults before negotiation | **Resolved** | §14: defaults are pure functions of `(constructor, target, requested_capabilities)`; negotiation verifies and MUST NOT rewrite; fixture in §19.1. |
| K3-17 | Angle algebra incomplete | **Resolved** | §6.3: closed operator set plus the four conversion functions and `arc_length`; complete-fixture line in §19.1. |
| K3-18 | Readiness claim without body | **Resolved** | §21.1 exists with a concrete checklist evaluated per component and per boundary; referenced from §25. |

The draft 0.5 changelog makes thirteen claims; all thirteen have supporting
body text. No changelog false-completeness this round.

---

## Part B — new findings against draft 0.5

Severity scale as before: Critical (spec cannot be implemented as written or
guarantees are silently false), High (a conforming implementation can diverge
on a load-bearing behavior), Medium (underspecified in a way implementations
will decide silently), Low (defect with bounded blast radius).

Classification per §20: **SD** specification defect, **MCT** missing
conformance test, **UR** usability regression, **AR** accepted-risk candidate.

---

### K4-01 — The spec's own examples are illegal under §5.4 (High, SD + UR)

§5.4 is normative: "Standard constructor operands are keyword-only in
language 1.0. A constructor MAY reserve its first positional slot solely for
a stable identity or stochastic `key` … all other positional arguments are
errors."

The illustrative source in §5.3 and §9 violates this rule four times:

```python
sword = asset(parts=[blade, guard], constraints=[connected(blade, guard)])
require(connected(blade, guard))
require(thickness(blade.edge) <= 1.5*mm)
prefer(match_silhouette(sword, front_view), weight=0.8)
```

- `connected(blade, guard)` passes two positional references. Neither is a
  stable identity nor a stochastic key.
- `require(...)` and `prefer(...)` take a positional constraint/objective
  value in the reserved slot, which §5.4 says is *solely* for identity/key.
- `match_silhouette(sword, front_view)` passes two positional references.

§19.1 explicitly requires fixtures for "keyword-only operands, the reserved
identity/key positional slot" — meaning a conforming test suite must reject
the spec's own examples.

This is not cosmetic. Examples are the primary training signal for the small
models this language is designed for (invariant 13), and the contradiction
hides a genuine design decision: constraint predicates and `require`/`prefer`
wrappers are unbearable in mandatory-keyword form
(`connected(a=blade, b=guard)`, `require(constraint=...)`). The rule, not the
examples, is probably what needs to change.

**Resolution options, in preference order:**

1. Extend §5.4: a registry signature MAY declare a fixed arity of positional
   *reference-typed* operands when the constructor is a symmetric or
   order-significant predicate/wrapper (`connected`, `require`, `prefer`,
   comparison-producing measurers). The registry remains the single source of
   truth for which slots exist; arbitrary positional use stays an error.
2. Or rewrite every example to mandatory-keyword form and accept the
   usability cost — then re-run the §19.3 small-model evaluation, because
   this materially raises the skill floor.

Either way, add the chosen form to the §19.1 fixtures and make the examples
compile under the final rule.

**Fixture:** `fixture.syntax.positional_reference_operands` — every §5.3/§9
example compiles under strict mode exactly as printed in the spec.

---

### K4-02 — "Dependency-stable" ordering is load-bearing and undefined (High, SD)

§13.2 uses the term twice: "`evidence_pins` is dependency-stably ordered" and
"declaration order is dependency-stable." The term is defined nowhere.

This ordering sits directly under two hard guarantees:

- Canonical IR must be byte-deterministic (invariant 4). JCS sorts map keys
  but does **not** sort arrays; `declarations`, `constraints`, `policies`,
  and `evidence_pins` are arrays whose element order is part of the hashed
  bytes. Without a total order rule, two conforming compilers can emit
  different canonical bytes for the same program and both claim Grade A.
- `semantic_digest` "changes only when compiled meaning … changes" (§13.3).
  Because §5.3 binding is a single source-order pass with no forward
  references, swapping two *independent* declarations is a meaning-preserving
  edit. If "dependency-stable" means source order, that cosmetic swap changes
  `semantic_digest` and busts the semantic cache, quietly weakening the
  §13.3 claim. If it means topological order, ties between independent
  declarations need a specified total tie-break or determinism dies anyway.

Either answer is defensible; the spec must pick one:

- **Source order** (simpler, matches the single-pass binder): then §13.3
  should honestly state that declaration reordering is a semantic-digest
  change even when meaning is preserved, and the scaffolder must emit
  declarations in a canonical order so round-trips are stable.
- **Topological order with a total tie-break** (e.g., lexicographic by fully
  qualified declaration ID within each dependency rank): buys reorder
  invariance at the cost of a specified sort everyone must implement
  identically.

The same decision is needed for `evidence_pins` (suggested: lexicographic by
logical ID, since pins have no inter-pin dependencies) and for `constraints`
and `policies`, which §13.2 does not order at all.

**Fixture:** `fixture.ir.array_order_determinism` — same program, declaration
order permuted; assert the specified digest behavior. Plus a cross-compiler
byte-equality fixture over all four arrays.

---

### K4-03 — Digest-domain membership of the remaining digest fields is unspecified (High, SD)

§13.3 resolved K3-01 for the three primary digests, but the envelope carries
four more digest-valued members: `registry_digest`, `source_digest`,
`dependency_digests`, and `evidence_manifest_digest`. Their preimage
membership is either unstated or stated ambiguously:

1. The `semantic_digest` exclusion list ends with "and the digest fields."
   Read literally, that excludes `registry_digest` — but the inclusion list
   says `semantic_digest` covers "registry." One phrase must yield. The
   exclusion needs to enumerate exactly which fields it means
   (`semantic_digest`, `extension_digest`, `document_digest`) instead of the
   open-ended plural.
2. **`evidence_manifest_digest`:** §13.2 says the manifest digest "covers the
   complete authorized manifest, including unused entries." If that field is
   part of the `semantic_digest` preimage, then adding an *unused* manifest
   entry changes `semantic_digest` — and since `semantic_digest` is "the
   semantic cache key" (§13.3), that contradicts §13.3's "Unused manifest
   entries do not invalidate phase caches." The only consistent reading is
   that `evidence_manifest_digest` is excluded from `semantic_digest` and
   included in `document_digest`, with semantic evidence influence flowing
   solely through the individual `semantic` pins. The spec never says this.
3. **`dependency_digests`:** §13.3 says `semantic_digest` covers "dependency
   semantic identities." If `dependency_digests` holds dependencies'
   `document_digest` values, a provenance-only change in a dependency ripples
   into every dependent's `semantic_digest`, violating the same §13.3 claim.
   The envelope must either state that `dependency_digests` are the
   dependencies' `semantic_digest` values, or split into two lists (semantic
   identities for the semantic preimage, document identities for the document
   preimage).

Under the adopted §25 gate, an accepted High finding affecting digest domains
blocks freeze; this one should be considered exactly that.

**Fixtures:** `fixture.digest.unused_manifest_entry_semantic_stability`;
`fixture.digest.dependency_provenance_isolation` — dependency's provenance-only
change preserves dependent's `semantic_digest`.

---

### K4-04 — §12.2 mask combinators cannot be spelled in the §5.2 subset (Medium, SD)

§12.2: "Masks support bounded `and`, `or`, `xor`, and `not` composition."

The §5.2 whitelist admits only unary numeric negation, dimensional
`+ - * /`, and comparison predicates. Python's `and`/`or` (BoolOp),
`not` (UnaryOp), `^`/`~` (BinOp/UnaryOp) are all outside the subset and
therefore rejected; and `a xor b` is not Python syntax at all — it parses as
a NameError-shaped expression statement, i.e., it is unspellable even before
the whitelist applies.

So the world domain's flagship composition feature ("conifers above 900 m on
north aspects and outside bogs") has no legal surface syntax. An
implementation will silently invent one — keyword operators, `&`/`|`/`~`
overloading, or registered constructors — which is precisely the silent
decision §24 forbids.

**Resolution options:**

1. Registered constructors: `mask_and(a, b)`, `mask_or(a, b)`,
   `mask_xor(a, b)`, `mask_not(a)` (or variadic bounded forms). Zero parser
   changes; ugly but unambiguous, and models handle nested calls fine.
2. Admit `&`, `|`, `^`, `~` into the operator whitelist typed *only* over
   `Mask<Space>` operands. Prettier, but expands the fuzz/typing surface and
   invites models to try them on booleans.

Whichever wins, §12.2 must stop naming Python keywords it cannot use, and
§5.2's accepted/rejected lists must be updated to match.

**Fixture:** `fixture.world.mask_composition_surface_syntax` — each combinator
compiles in the chosen spelling and is rejected in the unchosen spellings.

---

### K4-05 — Comparison result typing is context-dependent and chained comparisons are unaddressed (Medium, SD)

Three places assign different result types to the same surface syntax:

- §5.2: comparisons are "predicates used to construct constraints."
- §12.2: "Comparisons over compatible fields and quantities produce
  `Mask<Space>`."
- §6.2/§6.3 use comparison in value semantics ("compare equal", "Ordering and
  equality compare `Angle` only with `Angle`"), implying a `Bool`-producing
  role during constant evaluation.

So `a <= b` is a `Constraint` when its operands are quantities inside
`require(...)`, a `Mask` when one operand is a `Field`, and apparently a
plain truth value inside the constant evaluator. Nothing states the typing
rule that selects among these, whether a bare comparison expression statement
is legal outside `require`/`prefer`, or whether a `Constraint` value can be
bound to a name and referenced later (§5.3 says declarations are values, so
presumably yes — then its class per §9 must be carried in the type).

Separately, Python parses `a < b < c` as one chained `Compare` node with two
operators. The §5.2 accept/reject lists say nothing about chained
comparisons. A landform author *will* write `0.0 <= wetness <= 0.4`; the spec
must either define it as sugar for a conjunction (which currently doesn't
exist — see K4-04) or reject it with a repair that splits it.

**Fixtures:** `fixture.types.comparison_result_by_operand_class`;
`fixture.syntax.chained_comparison_behavior`.

---

### K4-06 — Module identity assignment and the reserved `module_seed` declaration are unspecified (Medium, SD)

§7 and §8 build the entire stochastic-isolation story on "stable module
identity," and §7 forbids source location as stochastic identity because
"unrelated edits can change them." But the spec never says where `module_id`
comes from. If any implementation derives it from the file path — the obvious
default — then renaming or moving a file rerolls every stochastic stream in
it and changes its `semantic_digest`, reintroducing through the back door
exactly the instability §7 spent three drafts evicting from the front door.

`module_id` needs an explicit assignment rule: an authored in-file
declaration, a build-request mapping, or a registry entry — and a stated
consequence for renames (stable ID preserved vs. deliberate identity change).

Related gaps in the same machinery:

- §7 path (1) is "an explicit reserved `module_seed` declaration in that
  module," but §5.2's allowed syntax has no reserved-declaration form, and
  the literal vocabulary (string/int/float/bool/None) doesn't include a
  `Seed` construction. Is `module_seed = 12345` an ordinary immutable
  assignment whose name is reserved? What literal or constructor produces a
  `Seed` (§24.6 covers representation, not surface syntax)?
- May other declarations *reference* `module_seed` as a value? Presumably
  not, but §5.3's "references to prior declarations" rule as written permits
  it.

**Fixtures:** `fixture.identity.module_rename_stream_stability`;
`fixture.seed.reserved_declaration_syntax`.

---

### K4-07 — Unit-constant arithmetic representation is unspecified and digest-visible (Medium, SD)

§6.3: canonical IR quantities use declared base units; `52 * mm` is Int ×
unit-constant. §6.2 pins per-operation binary64 rounding. What is never
stated is what a unit constant *is* numerically. Two natural implementations
disagree:

1. `mm` is the binary64 value nearest 0.001; `52 * mm` is one binary64
   multiply → `0x3FAA9FBE76C8B439...`-ish, one ulp pattern.
2. The compiler rescales exactly (decimal/rational 52 × 10⁻³) and rounds
   once to the nearest binary64 of 0.052 → potentially a different ulp.

Because canonical hashes are over JCS-encoded numeric values, the two
strategies produce different `semantic_digest`s for identical source. Worse,
whether `52*mm == 0.052*m` and whether `thickness <= 1.5*mm` passes at the
boundary become implementation lottery at the last ulp. §6.2's
per-operation-rounding rule *suggests* strategy 1, but unit application is
plausibly "default resolution" rather than "source arithmetic," so an
implementer can argue either.

The fix is one sentence: unit constants are exact registry-declared rational
scale factors, and quantity canonicalization multiplies the source magnitude
by that factor with a single round-to-nearest binary64 at the end (or,
alternatively: unit constants are binary64 values and §6.2 operation rounding
applies — but then say so). Add cross-host test vectors next to the §7 seed
vectors.

**Fixture:** `fixture.units.canonicalization_ulp_vectors` — pinned digests for
`52*mm`, `0.052*m`, `52000*um`, and a boundary constraint at the disagreeing
ulp.

---

### K4-08 — `Identifier` lexical grammar is unspecified; evidence-derived IDs flow into scaffolds (Medium, SD + MCT)

`Identifier` is a core type (§6.1) and §16 limits identifier *length*, but no
section defines the allowed character set. Meanwhile the scaffold pipeline
(invariant 8, §13.2 round-trip) embeds identifiers that originate outside the
language: reviewed feature IDs arrive from evidence/world data and are
printed into generated source, exactly as the prototype does today
(`world.landform({feature_id!r}, ...)` in `worldbuilder_dsl.py`).

A feature ID containing a quote, newline, or comment marker is a scaffold
injection vector: the scaffolder emits it, the emitted file is strict-valid
by the round-trip guarantee, and the injected text compiles with the
authority of compiler-generated source. The prototype survives because
Python's `repr` escapes correctly — but that is an implementation accident,
not a spec guarantee, and a non-Python scaffolder (per the §21 bakeoff) has
no such accident available.

Specify: (a) a closed identifier grammar (suggest: `[a-z_][a-z0-9_]*` plus a
dotted namespace form, with the §16 length cap), enforced wherever IDs enter
the system — source, registry, evidence interpretation metadata, and build
requests; (b) that scaffolders MUST reject or escape any external ID that
does not satisfy the grammar rather than best-effort quoting it.

**Fixtures:** `fixture.security.scaffold_identifier_injection`;
`fixture.types.identifier_grammar_boundaries`.

---

### K4-09 — Prototype lenient clamping is unclassifiable under §15's repair taxonomy (Medium, SD + MCT)

§23.1 requires capturing every current `worldbuilder_dsl.py` fixture, and §15
now defines the closed repair classes. The prototype's lenient mode does two
things:

- corrects misspelled enum values by nearest match — cleanly **corrective**
  under §15, provided the single-candidate confidence-window rule is
  respected;
- **clamps** out-of-range scalars (`spines` 12 → 8, `cross_jitter` 0.7 → 0.5)
  and records the clamp.

Clamping is not corrective (it does not restore declared intent — the author
declared 0.7), not migratory (no versioned equivalence rule), and reads as
textbook **intent-changing/degrading**: it changes a quality choice to make a
gate easier to satisfy. Under §15, such edits are "never repairs, never
auto-applied" — including, as written, in lenient mode, since lenient mode
"MAY apply only explicitly classified, semantically unambiguous local
repairs" and the classes exclude intent changes.

So the migration required by §23 either (a) drops lenient clamping and
regresses current authoring behavior, or (b) needs §15 to define how a
bounded-range projection may be offered — e.g., as the "clearly labelled
alternative" §15 already allows, surfaced in the lenient repair manifest but
requiring the promotion/acknowledgement step before strict recompilation.
The spec should say which, and the §23 fixture capture should record the
expected classification for each prototype repair so the behavior is decided
by text rather than by whoever ports the code.

**Fixture:** `fixture.migration.prototype_repair_classification` — every
repair emitted by the prototype's lenient mode mapped to a §15 class and an
expected auto-apply/present-only disposition.

---

### K4-10 — Tuple vs list literals and duplicate dictionary keys (Low, SD)

§5.2 admits "bounded list, tuple, and string-keyed dictionary literals" and
§6.1 provides `List<T, N>` and `Record`. Unstated:

- Do `(1, 2)` and `[1, 2]` denote the same IR value? JCS JSON has one array
  type, so if tuples and lists are distinct nominal types the distinction
  needs an explicit encoding; if they are interchangeable sugar, say so and
  have the scaffolder emit one canonical spelling.
- Python syntax permits duplicate keys in a dict literal
  (`{"a": 1, "a": 2}`) with silent last-wins semantics. §5.4 rejects
  duplicate *arguments* but nothing rejects duplicate *record keys*. Under
  invariant 6 this must be an error with a repair, not an inherited Python
  behavior. (The prototype currently inherits last-wins via
  `ast.literal_eval`-style folding — a captured-fixture candidate for §23.)

**Fixture:** `fixture.syntax.record_duplicate_key_rejection`;
`fixture.types.tuple_list_equivalence`.

---

### K4-11 — Soft-objective promotion target label is ambiguous (Low, SD)

§9 closes the validation classes to four machine labels, then says a policy
may promote a soft objective "to a realized hard gate" — a phrase that names
none of the four labels while resembling two of them (`realized_hard`,
`acceptance_gate`). Diagnostics and registries must use the closed labels, so
the promotion rule must state which label a promoted objective carries and
whether its diagnostic attribution follows the promoting policy or the
original objective declaration.

**Fixture:** `fixture.constraints.soft_promotion_label`.

---

### K4-12 — Cache reuse events vs entry immutability (Low, SD)

§14: "Producer provenance stored in a cache entry is immutable. A cache hit
retains that original producer record and appends a separate reuse event…"
Appends it *where*? If to the cache entry, the entry is not immutable and
concurrent builds race on it; if to the consuming build's provenance, say so.
The intent is obviously the latter; one clause fixes it.

---

### K4-13 — The empty-extension-set constant digest is never specified (Low, SD + MCT)

§13.3: "An empty extension set has one specified constant digest." The
constant is not specified in this document and no registry location is named
for it. Pin the preimage (suggested: the domain-tagged JCS encoding of the
empty array, consistent with the general rule, rather than a magic value) and
add it to the §19.1 pinned-vector fixtures alongside the `document_digest`
vector.

---

## Freeze recommendation

**Do not freeze draft 0.5 as language 1.0**, per the spec's own §25 gate:
K4-02 and K4-03 are High findings inside the digest-domain surface, and
K4-01/K4-04/K4-05 sit on the parser operator/typing surface.

**Blockers before the parser whitelist or IR schema is pinned (draft 0.6):**

1. K4-01 — decide the positional-operand rule; make the spec's examples legal
2. K4-02 — define "dependency-stable" as a total order; state reorder-digest
   behavior
3. K4-03 — enumerate digest-field preimage membership for all seven
   digest-valued envelope members
4. K4-04 — pick the mask-composition surface syntax
5. K4-05 — comparison result typing and chained-comparison rule
6. K4-06 — module-identity assignment and `module_seed` declaration syntax

**May proceed in parallel with early implementation:** K4-07, K4-08, K4-09,
K4-10, K4-11, K4-12, K4-13 — all still required before freeze.

---

## What was not verified

- No execution of `pipeline/worldbuilder_dsl.py` or any conformance suite
- No fuzzing, backend builds, digest-vector computation, or model-usability
  runs; the K4-07 ulp divergence is argued from IEEE-754 semantics, not
  measured on hosts
- Prior-resolution verification is against draft 0.5 prose only
- Line-level citations omitted intentionally; drafts are moving fast enough
  that section anchors are the stable reference

---

## Summary table

| ID | Severity | Classification | One-line |
|---|---|---|---|
| K4-01 | High | SD + UR | Spec's own examples violate §5.4 keyword-only rule |
| K4-02 | High | SD | "Dependency-stable" array ordering undefined, digest-visible |
| K4-03 | High | SD | Remaining digest fields' preimage membership unspecified |
| K4-04 | Medium | SD | §12.2 mask `and/or/xor/not` unspellable in §5.2 subset |
| K4-05 | Medium | SD | Comparison result type context-dependent; chained compares unaddressed |
| K4-06 | Medium | SD | `module_id` assignment and `module_seed` syntax undefined |
| K4-07 | Medium | SD | Unit-constant arithmetic representation digest-ambiguous |
| K4-08 | Medium | SD + MCT | `Identifier` grammar unspecified; scaffold injection via external IDs |
| K4-09 | Medium | SD + MCT | Prototype lenient clamping unclassifiable under §15 |
| K4-10 | Low | SD | Tuple/list equivalence and duplicate record keys |
| K4-11 | Low | SD | Soft-objective promotion label ambiguous |
| K4-12 | Low | SD | Cache reuse-event location vs entry immutability |
| K4-13 | Low | SD + MCT | Empty-extension-set constant digest never pinned |

**Bottom line:** the architectural debt is gone; what remains is precision
debt in the newest text. Draft 0.5 fixed everything it aimed at and then
wrote four new sentences that two conforming implementations would read
differently. One more focused patch round (0.6) covering the six blockers
closes the gap between "resolved in prose" and "implementable without silent
decisions."
