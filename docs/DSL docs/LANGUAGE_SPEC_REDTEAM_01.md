# Red team 01 — Luxel model-native language specification

**Reviewing:** `docs/DSL docs/LUXEL_LANGUAGE_SPEC.md` draft 0.1, 2026-08-03
**Reviewer:** Claude (Opus), 2026-08-03
**Requested by:** Matt
**Against:** §20 "Red-team requirements before 1.0 freeze"

This is the first of the two independent frontier-model red teams §25 requires.
It is a review of the specification only — no implementation was exercised.

**Resolution:** accepted into `LUXEL_LANGUAGE_SPEC.md` draft 0.2 on 2026-08-03.
All fifteen findings were incorporated. Finding 12's proposed source-span or
occurrence-index identity was replaced with a stronger stable semantic-key
rule, because source spans and ambient indexes also move under unrelated edits.
No implementation work is implied by acceptance; every resolution remains
subject to the conformance fixtures specified in draft 0.2.

Findings are classified with the spec's own §20 taxonomy: **specification
defect**, **implementation defect**, **missing conformance test**, **backend
limitation**, **usability regression**, **accepted risk**. Each carries a
proposed resolution and, per §20, a regression fixture that would catch it.

Ordered by what it costs if it ships wrong.

Nothing here argues against the design. The invariants in §2 are the right
invariants, and §2.13 — authoring difficulty as a compatibility dimension — is a
better idea than anything Luxel asked for. All three properties worth carrying
from the prototype survive: parsed-never-executed (§2.1, §5.2), actionable
repairs (§2.7, §15), round-trip scaffolding (§2.8). The findings below are
places where a normative rule contradicts another normative rule, where a MUST
has no mechanism, or where the spec quietly gives up something Luxel already has.

---

## 1. The random-stream rule contradicts itself

**Classification:** specification defect
**Where:** §8, lines 308–310

Streams derive from `(program_digest, declaration_id, explicit_seed,
operation_path)`. The next sentence: "Adding an unrelated declaration MUST NOT
perturb an existing declaration's random stream."

`program_digest` is a digest of the whole program. Adding any declaration
changes it, and therefore reseeds every stream in the module. The stated
requirement and the stated mechanism are exact opposites.

The authoring consequence is severe and immediate: adding one bridge rerolls
the forest, the scree, the snow, and every scattered instance in the world.
Under §2.13 that is a language regression on its own — it makes incremental
editing, which is the whole point of scaffold-first authoring, impossible.

**Resolution:** derive from `(module_seed, declaration_id, explicit_seed,
operation_path)`, where `module_seed` is explicitly authored and stable.
Distinctness between different programs is already carried by `declaration_id`
and the explicit seed; `program_digest` buys nothing that is not already
covered and costs stream isolation.

**Fixture:** compile a module; append an unrelated declaration; assert every
pre-existing declaration's realized instances are byte-identical.

---

## 2. The IR envelope cannot reproduce itself

**Classification:** specification defect
**Where:** §13.2 against §2.4 and §5.4

§2.4 conditions determinism on "source, inputs, compiler version,
**capabilities**, and seeds." §5.4 places every constructor's "signature,
defaults, units, bounds, and capability requirements" in versioned registry
data, and resolves defaults into canonical IR so that "backend behavior never
depends on an implicit default."

Canonical IR content therefore depends on the registry version. The required
envelope in §13.2 carries `schema_version`, `language_version`, `source_digest`,
`dependency_digests` and `required_capabilities` — but **no registry digest and
no compiler version**.

Two compilers with different registry versions produce different IR from
byte-identical source, and the resulting document records nothing that explains
the difference. The IR is content-addressable (§13.1) over content that is not
fully described by its own envelope. Every downstream cache keyed on that digest
(§14, "incremental compilation MAY cache any phase by content digest") inherits
the hole — this is the stale-cache-poisoning case §20 asks reviewers to find,
reachable without any adversary.

**Resolution:** add `registry_digest` and `compiler_version` to the required
envelope. State that the canonical digest is computed over an encoding that
includes them.

**Fixture:** resolve one constructor default differently across two registry
versions; assert the two IR digests differ and that a cache keyed on the digest
does not hit across them.

---

## 3. Artifact-level byte determinism is silently demoted

**Classification:** usability regression
**Where:** §2.4, §19.1, open decision 11, against current Luxel behavior

Luxel has this property today: the same batch compiled from two different working
directories produces **byte-identical artifacts**. It was established
deliberately on 2026-08-02 and it is what makes a rebuild trustworthy.

The spec does not preserve it. §2.4 requires byte-equivalence of canonical *IR*
only. Artifacts are covered by §19.1's "determinism across process and device
runs **within declared tolerances**" and by open decision 11, "hard versus
optional backend determinism grades." A working guarantee has become an
unresolved question, and §17 lets a backend publish a weak determinism grade and
still conform.

That sits badly against §25's closing gate, "no backend silently drops, clamps,
mirrors, substitutes, or **rerolls** intent."

**Resolution:** require a hard determinism grade for the reference backends, and
close open decision 11 in that direction. If tolerance-based determinism is
genuinely needed for a GPU field backend, scope it to that backend explicitly
and record the loss, rather than leaving the default open.

**Fixture:** build the same batch from two paths on two processes; assert
byte-identical artifacts, not tolerance-equal ones. This fixture exists in
substance already and should be carried in as conformance input per §23.

---

## 4. "Legal means buildable" is a MUST with only a SHOULD behind it

**Classification:** specification defect
**Where:** §2.3 against §10, line 354; and §23.5

§2.3 is normative and strong: a strict program accepted for a declared target
"MUST satisfy all statically knowable target bounds."

The only mechanism the spec offers is §10: "Legal bounds **SHOULD** be derived
from, or conservatively imply, the actual buildable region." A SHOULD cannot
discharge a MUST. As written, a registry may publish bounds looser than the
buildable region and remain conformant while §2.3 is violated.

This is D1 arriving intact. The current prototype's own docstring admits that
its documented bounds are not its buildable bounds, and §23.5's migration item —
"represent documented versus buildable bounds explicitly" — makes the gap
*visible* rather than closing it. Visible and closed are different states, and
only one of them satisfies §2.3.

**Resolution:** raise §10 to MUST, and require every registry bound to be
accompanied by either a proof or a conservative derivation. Where a bound is
genuinely evidence-dependent and cannot be statically known, it must be declared
as such and excluded from §2.3's claim, so that the invariant stays true.

**Fixture:** for every registry parameter, assert that a program at each bound
extreme compiles and builds for the declared target. A parameter whose bound
cannot be built is a failing test, not a documentation note.

---

## 5. Lenient repair can rebind to a different valid declaration

**Classification:** specification defect
**Where:** §15, lines 532–534, with §7, lines 282–284

§15 permits lenient mode to apply "explicitly classified local repairs such as a
high-confidence spelling correction." §7 requires the compiler to "suggest the
nearest legal identifier when confidence is high enough." Neither defines the
threshold, and §24 does not list it as an open decision.

The dangerous case is not repair into an error — that is caught. It is repair
into **validity**. Where both `blade_path` and `blade_paths` exist, a
nearest-identifier correction produces a program that parses, type-checks,
certifies, and builds the wrong asset. §15's precondition digest guards the
source *range*; it does not guard semantic identity, so it does not catch this.

This is §20's "diagnostic edits that alter the wrong source," in the form where
nothing downstream can detect it.

**Resolution:** forbid automatic identifier repair when more than one candidate
lies within the edit-distance window, and forbid it unconditionally when the
candidate resolves to an existing binding of a compatible type. Both cases
become a diagnostic listing alternatives, never an applied edit. Define the
confidence threshold normatively or add it to §24.

**Fixture:** a module binding two similar identifiers of the same type, with a
third misspelled reference; assert no automatic repair is applied and that the
diagnostic enumerates both candidates.

---

## 6. The unit system is an enumerated list where it needs an algebra

**Classification:** specification defect
**Where:** §6.3, line 251, and open decision 8

§6.3 says the registry "SHALL include at least length, angle, time, mass, and
dimensionless ratio." Open decision 8 asks which dimensions are required
"beyond length, angle, time, and mass." Both sentences treat dimension as a set
to be enumerated.

§5.2 admits dimensionally typed `*` and `/`. Those operators generate
dimensions: `52*mm * 3*mm` is an area, `m/s` is a velocity, `kg/m^3` is a
density. The closure of a multiplicative group over base dimensions cannot be
enumerated in a registry list. Either the type checker rejects legal arithmetic,
or it accepts it and produces a quantity whose dimension the registry cannot
name — and §6.3's own example rule, rejecting `52*mm + 3*deg`, requires knowing
the dimension of both operands.

Angle needs a rule of its own. It is dimensionless-but-typed, so the spec must
state how `radius * angle → length` is admitted, or arc-length arithmetic is a
type error under §6.3 as written.

**Resolution:** specify dimensions as integer exponent vectors over a small set
of base dimensions, with derived dimensions computed rather than registered.
Open decision 8 then reduces to "which bases," which is a genuinely small
question. Add an explicit rule for angle in products with length.

**Fixture:** property test that `(a*b)/b` type-checks to the dimension of `a`
for all registry unit pairs; assert `mm*mm` yields area and `mm+deg` is
rejected with a dimension diagnostic.

---

## 7. This is an asset specification with a world domain attached

**Classification:** specification defect
**Where:** §11 against §12, and §10 line 350

§11 gives the geometry/asset domain a full section and a fifteen-item minimum
construction vocabulary (§11.1). §12 gives the world domain — the domain that
exists, ships, and carries roughly 484 green tests — three paragraphs and **no
vocabulary at all**.

§10 promises typed policies covering `acceptance_policy`, `traversal_policy` and
`border_policy`. Luxel's actual policy surface also includes hydrology,
vegetation, siting and surfacing. None of those appear anywhere in the document.

§25 nonetheless gates 1.0 readiness on "one Blender asset pipeline **and one Luxel
world pipeline** consume canonical IR." A domain that is not specified cannot be
a freeze gate.

This reads as the spec faithfully reflecting where the work currently is — the
2D-image-to-3D-asset pipeline — which is legitimate as a statement of sequence.
It is not legitimate as a statement of scope, because the world domain is the
half with users.

**Resolution:** give §12 the same treatment §11 receives: a minimum construction
vocabulary, the full policy inventory, and typed forms for the existing solver
outputs. Until then, either the §25 world-pipeline gate or the §12 section is
overstated.

**Fixture:** every policy field currently accepted by the JSON policy blocks has
a typed registry entry with type, unit, default, bounds, owner and affected
gates, per §10. Assert the two sets are equal.

---

## 8. Unknown IR fields are only closed in certification mode

**Classification:** specification defect
**Where:** §13.1, line 455, against §2.6 and §25

§13.1 requires the IR to be "closed to unknown fields **in certification
mode**." The scoping implies unknown fields are tolerated outside it — which
means dropped. §2.6 forbids silent omission; §25 forbids a backend that
"silently drops."

§18's "losslessly migratable between supported minor versions" covers migration,
not reading. A reader built against 1.0 consuming 1.1 IR loses fields with no
diagnostic, and the loss is invisible precisely because it happens outside
certification.

**Resolution:** close the IR to unknown fields in all modes. If forward
compatibility is wanted, add an explicit extension mechanism with a declared
ignorable namespace, so that dropping is an authored decision rather than a mode
side effect.

**Fixture:** feed IR containing an unrecognized field to the reference reader in
every mode; assert a structured error in each.

---

## 9. §4 prejudges the §21 bakeoff

**Classification:** specification defect
**Where:** §4, lines 96–110, and §17's table, against §21

§21 states the semantic compiler host "SHALL be selected using implemented
vertical slices, not language preference," listing Odin, Rust, Python, Julia and
a split architecture as candidates, and §17 calls its table "provisional
architecture, not a host-language decision."

But §4 already draws Odin, Taichi, Julia and Blender as *the* pipeline, in a
diagram positioned as architecture, and §17's table already assigns Odin the
compiler driver, cache and plugin ABI. Readers implement diagrams.

There is substance under the editorial point. §16 requires the parser, binder,
type checker, canonicalizer, migrators and all boundary decoders to be **fuzzed**,
and §21 measures "fuzzing and property-testing maturity" — a criterion on which
the candidates differ by an order of magnitude, and not in Odin's favor. §21
also concedes that stable Taichi "requires a pinned supported Python
environment, isolated from both system and Blender Python versions." The drawn
architecture is a four-runtime build — Odin, Taichi's pinned Python, Julia, and
Blender's Python — and §25 gates on all of those boundaries having reproducible
builds.

**Resolution:** mark §4 explicitly as one candidate realization pending §21, or
redraw it with the backend row unnamed. Treat the four-runtime cost as a
first-class bakeoff criterion rather than an assumption, given the team size
maintaining it.

**Fixture:** none — this is a documentation and sequencing finding.

---

## 10. No vocabulary for conditional placement

**Classification:** specification defect
**Where:** §5.2 and §8, line 305, with §11.1 and §12

§5.2 rejects conditionals and comprehensions. §8 routes all iteration through
bounded constructors such as `repeat`, `scatter` and `sample`. Both rules are
correct and worth keeping.

What the spec does not then supply is any way to express a *predicate over a
field*. The authoring task Luxel actually has is "conifers above 900 m on north
aspects, none in the bog." §11.1 offers "signed-distance fields and bounded
field modifiers," which is a geometry facility, not a mask language, and §12
defines no world vocabulary at all.

This is load-bearing on near-term work. Forestry placement is one of four
systems currently solved in Python and unobeyed at the Rust boundary; it is the
next thing to be fixed. If the language cannot express a masked scatter, the DSL
cannot own the surface it is being designed to own.

**Resolution:** specify a typed mask/predicate vocabulary in the world domain —
field comparison producing a `Mask`, boolean combination of masks, and mask as a
required or optional argument to the bounded placement constructors.

**Fixture:** author a masked scatter against elevation and aspect fields; assert
zero instances outside the mask and deterministic instance identity across
rebuilds.

---

## 11. Gate diagnostics assume a single responsible declaration

**Classification:** specification defect
**Where:** §2.3, lines 43–44

§2.3 permits a later gate to reject evidence-dependent output, "but its
diagnostic MUST identify the responsible declaration or parameter."

Acceptance gates are measurements over a realized artifact, and their failures
are frequently emergent with no single cause. The live example: Luxel's
black-pixel gate reads 0.094 against a 0.08 threshold, and the responsible
parameter is arguably the 62 m vertical budget — a global aesthetic decision —
and arguably nothing local at all. Under §2.3 as written the compiler must
either name a declaration and be wrong, or violate a MUST.

**Resolution:** permit a diagnostic to carry a *set* of contributing
declarations ranked by sensitivity, and require it to state explicitly when
responsibility is global rather than local. Attribution by sensitivity is
implementable; attribution to one declaration is not always true.

**Related, and worth a normative answer rather than a bullet:** §20's final item
asks reviewers to find "incentives where acceptance metrics reward worse art or
gameplay." Nothing in the spec counters that incentive. Luxel has hit it twice
already — a gate that can be passed by flattening the mountains rewards
flattening the mountains, and the correct action was to refuse the gate and log
the defect. A specification that makes gates normative should say, normatively,
that a gate may not be satisfied by degrading the declared aesthetic intent, and
that an unpassable gate is a specification finding rather than an authoring
failure.

**Fixture:** a world failing a whole-map gate with no single responsible
declaration; assert the diagnostic reports global responsibility and a ranked
contributor set rather than naming one declaration.

---

## 12. Content-derived IDs unify declarations the author wanted distinct

**Classification:** specification defect
**Where:** §7, line 277, with §8

§7 gives compiler temporaries "deterministic content-derived IDs." Two
structurally identical calls therefore hash to one identity.

For pure geometry that is common subexpression elimination and harmless. It
stops being harmless where the declaration carries a random stream: §8 derives
streams from `operation_path`, so two identical `scatter(...)` calls unify into
a single node with a single stream. The author who wrote two independently
varied clusters gets one cluster rendered twice — a mirrored back, in the same
family of defect §2.10 exists to prevent.

**Resolution:** for any declaration whose type carries a random stream, include
occurrence index or source span in the content-derived ID, so structural
equality does not imply stream equality.

**Fixture:** two structurally identical seeded scatters in one module; assert
their realized instance sets differ and that both are stable across rebuilds.

---

## 13. Repair edits cannot be batched

**Classification:** usability regression
**Where:** §15, lines 529–531

A repair edit states "the source range, replacement text, and precondition
digest." If the digest covers the source document, applying edit A invalidates
edit B's precondition, so a model holding five diagnostics must apply one,
recompile, and repeat.

§19.3 then measures "compilation attempts" and "human correction time" as
usability metrics, against a repair protocol that guarantees a serial loop.
Under §2.13 that is a self-inflicted regression.

**Resolution:** specify batch application: a set of edits with disjoint ranges
against a single base digest may be applied together, with the precondition
evaluated once. Overlapping ranges remain serial.

**Fixture:** a module producing several independent diagnostics; assert all
repairs apply in one pass and the result compiles.

---

## 14. The evidence resolver is unspecified

**Classification:** specification defect
**Where:** §16, lines 550–551, with §5.5

§16 correctly bars compilation from filesystem, environment, network, clocks,
processes, dynamic libraries and host reflection. External evidence then enters
"only through a caller-provided, digest-pinned manifest and an authorized
resolver" — and that is the entire specification of the resolver.

§20 asks reviewers to break "malicious evidence manifests and path traversal."
The resolver is the whole of the remaining filesystem attack surface, and it has
no normative requirements: no rule on path confinement, symlink handling, digest
verification order, size limits, or what "authorized" means.

**Resolution:** give the resolver its own normative subsection — root
confinement, no symlink escape, verify digest before use rather than after, hard
size and count limits, and a requirement that a manifest entry failing
verification is a structured error rather than a skip.

**Fixture:** manifests containing `..` traversal, an escaping symlink, a
digest mismatch, and an oversized entry; assert a distinct structured error for
each and no filesystem read outside the root.

---

## 15. §25 gates on a threshold §24 never sets

**Classification:** missing conformance test
**Where:** §25, line 768, against §24

§25 requires that "at least one small model completes the benchmark at the
agreed threshold." §24's open decisions do not include that threshold; item 17
covers product-level acceptance for the four asset families, which is a
different quantity. Per §24's closing line, an open decision is not permission
to choose silently — but this one is not even recorded as open.

**Resolution:** add the model-usability threshold to §24, expressed against the
§19.3 metrics: first-pass parse rate, first-pass semantic validity, repair
success, collateral edits.

---

## Relationship between Luxel and the language

Not a finding — an agreement that should be written into the spec because it
binds both sides.

Luxel **pins a released language version and feeds requirements upstream rather
than forking**. §18 approaches this ("no ambient latest in reproducible builds")
and §23.8 protects the running build, but the pinning relationship itself is not
stated. It belongs in §18 or §23, because it is the clause that keeps a language
change from becoming an unscheduled Luxel migration.

---

## Summary

| # | Finding | Class |
|---|---|---|
| 1 | Random stream keyed on `program_digest` contradicts stream isolation | spec defect |
| 2 | IR envelope omits registry digest and compiler version | spec defect |
| 3 | Artifact byte determinism demoted to an open decision | usability regression |
| 4 | §2.3 MUST backed only by a §10 SHOULD (D1 inherited) | spec defect |
| 5 | Lenient identifier repair can rebind to a valid wrong target | spec defect |
| 6 | Dimensions enumerated rather than an exponent algebra | spec defect |
| 7 | World domain unspecified while gating 1.0 readiness | spec defect |
| 8 | Unknown IR fields closed only in certification mode | spec defect |
| 9 | §4 diagram and §17 table prejudge the §21 bakeoff | spec defect |
| 10 | No mask/predicate vocabulary for conditional placement | spec defect |
| 11 | Gate diagnostics assume single-declaration responsibility | spec defect |
| 12 | Content-derived IDs unify distinct seeded declarations | spec defect |
| 13 | Repair edits cannot be batched | usability regression |
| 14 | Evidence resolver has no normative requirements | spec defect |
| 15 | Model-usability threshold gates §25 but is absent from §24 | missing test |

Findings 1, 2 and 12 are mechanical and cheap to fix now. Findings 4, 7 and 11
are the ones that decide whether the language is honest about what it
guarantees. Finding 3 is the only place the spec is currently worse than what
Luxel already has.
