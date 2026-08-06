# Red team 02 — WGE model-native language specification

**Reviewing:** `docs/DSL docs/WGE_LANGUAGE_SPEC.md` draft 0.2, 2026-08-03
**Reviewer:** Claude (Opus), 2026-08-03
**Requested by:** Matt
**Follows:** `LANGUAGE_SPEC_REDTEAM_01.md` (15 findings, all accepted into 0.2)
**Against:** §20 "Red-team requirements before 1.0 freeze"

Second pass over the specification, run against draft 0.2 rather than the
original. Two purposes: verify that the round-one resolutions actually landed
as claimed, and attack the new text they introduced.

**Resolution:** accepted into `docs/DSL docs/WGE_LANGUAGE_SPEC.md` draft 0.3 on
2026-08-03. All four findings were incorporated. Extension integrity was
resolved with separate semantic, extension, and composed document digest
domains; Grade-A artifacts now use closed, registry-owned canonicalization
profiles rather than an undefined backend-selected normalization.

Classification uses the §20 taxonomy. Each finding carries a proposed
resolution and a regression fixture.

---

## Part A — verification of round-one resolutions

All fifteen round-one findings are resolved in draft 0.2. This was checked
against the text, not against the changelog. Specifically confirmed:

| # | Round-one finding | Where resolved | Verified |
|---|---|---|---|
| 1 | Random stream keyed on `program_digest` | §8 | Tuple is now `(module_seed, declaration_id, explicit_nested_key, explicit_seed, operation_path)`; whole-program digests, spans, and ambient ordering explicitly excluded |
| 2 | Envelope omits registry/compiler identity | §13.2 | `compiler_version`, `registry_digest`, `module_seed` added; §13.2 states hashes are computed over an encoding including them |
| 3 | Artifact byte determinism demoted | §2.4, §17 | Grade A/B/C introduced; Grade A mandatory for reference compiler, world backend, and artifact packager |
| 4 | §2.3 MUST backed only by a SHOULD | §10 | Raised to MUST, with derivation/validation evidence recorded in the registry and evidence-dependent conditions marked as such rather than claimed static |
| 5 | Lenient identifier repair | §15 | Never automatic with multiple candidates in the confidence window or when the token resolves to a compatible binding; threshold is registry-versioned |
| 6 | Dimensions enumerated, not an algebra | §6.3 | Integer exponent vectors over base dimensions; derived dimensions computed; angle given a distinct nominal type with an explicit `arc_length` mapping |
| 7 | World domain unspecified | §12 | Expanded from three paragraphs to §12.1–12.4: vocabulary, masks, generated policy inventory, typed solver products |
| 8 | Unknown IR fields closed only in certification | §13.1 | Closed in every mode; forward compatibility moved to an explicit extension envelope |
| 9 | §4/§17 prejudge the bakeoff | §4, §17 | Diagram genericized; table reframed as bakeoff hypotheses; Rust added as a candidate semantic compiler |
| 10 | No mask/predicate vocabulary | §12.2 | `Mask<Space>` from field comparison, bounded boolean composition, optional mask on placement constructors, hard-mask exclusion required of backends |
| 11 | Gate attribution assumes one declaration | §2.3, §15 | `attribution: local\|ranked\|global`; diagnostics may not invent a single cause; new invariant 14 forbids satisfying a gate by degrading declared intent |
| 12 | Content-derived IDs unify seeded declarations | §7 | Resolved by a stronger rule than proposed — see note below |
| 13 | Repair edits cannot be batched | §15 | Disjoint-range batches against one base digest, applied atomically, all-or-none |
| 14 | Evidence resolver unspecified | §16.1 | Full subsection: root confinement, traversal/symlink/device rejection, per-entry and aggregate limits, digest-before-decode, race handling, no silent skips |
| 15 | Model-usability threshold absent from §24 | §24.18 | Added, expressed against the §19.3 metrics |

§19 now carries a conformance fixture for each of these, which is what §20
requires of a resolution.

**On finding 12, the deviation is an improvement.** Draft 0.2 rejected the
proposed source-span/occurrence-index identity on the grounds that both are
themselves unstable under unrelated edits, and replaced it with persistent
semantic identity derived from the owning declaration plus explicit nested
keys. That reasoning is correct and the round-one proposal was a partial fix.
The residual in Part B is a detail of the replacement rule, not a return to
the original position.

---

## Part B — findings against draft 0.2

Ordered by what they cost if left wrong. None reaches the severity of the
round-one set; these are residuals, one undefined term, and one
propagation miss.

---

## 1. The nested-key condition reintroduces the reroll it prevents

**Classification:** specification defect
**Where:** §7, lines 304–306, with §8, lines 340–341

§7 requires, for stochastic constructors, "the owning declaration ID plus an
explicit nested key **when more than one such operation appears beneath that
declaration**."

The conditional is the problem. A declaration containing one stochastic
operation has identity `declaration_id` with no nested key. Add a second
stochastic operation beneath the same declaration and the rule now requires
explicit keys on both — so the first operation's identity changes shape from
`declaration_id` to `declaration_id + key`. Because §8 feeds
`explicit_nested_key` directly into the stream tuple, the first operation's
random stream moves and its realized output rerolls.

That is round-one finding 1 one level down, and it violates §8's own closing
sentence: "Adding an unrelated declaration MUST NOT perturb any pre-existing
declaration's realized output." Adding a nested operation is not literally
adding a declaration, but it is the same class of edit and produces the same
surprise — the author changes one thing and unrelated content moves.

**Resolution:** make the nested key mandatory for every stochastic operation
regardless of how many appear beneath a declaration, so identity never changes
shape. §12.2 already lists an identity key among the explicit parameters of
`scatter`, and scaffold generation (§2.8) can emit it, so the authoring cost
under §2.13 is close to zero — and strictly lower than the cost of a silent
reroll.

**Fixture:** a declaration containing one seeded placement; add a second
seeded placement beneath the same declaration; assert the first placement's
realized instances are byte-identical before and after.

---

## 2. "Normalized artifact" is load-bearing and undefined

**Classification:** specification defect
**Where:** §2.4, lines 54–55, and §17, lines 711–712

Both statements of the strongest guarantee in the specification are phrased in
terms of *normalized* artifacts:

- §2.4: "Reference production backends MUST also produce byte-identical
  normalized artifacts across working directories and processes."
- §17 Grade A: "normalized artifacts are byte-identical for equal declared
  inputs across paths and processes on supported targets."

The term is never defined. `normaliz*` appears elsewhere in the document only
for normalized coordinate spaces, resolver path normalization, and
soft-objective normalization semantics (§24.12) — none of which is this.

As written, a backend defines its own normalization, and a backend that
normalizes away exactly the bytes that differ can claim Grade A truthfully.
The guarantee is unfalsifiable, which matters more here than elsewhere because
Grade A is what round-one finding 3 was raised to protect: an existing,
already-achieved WGE property.

**Resolution:** define a canonical artifact encoding for Grade A — what is
excluded from the byte comparison (embedded timestamps, absolute path
fragments, tool version strings, archive member ordering and metadata) and what
must be bit-exact. Require each backend to declare its normalization in the
capability manifest rather than choosing it privately, and forbid excluding any
field that carries semantic content.

**Fixture:** build the same batch from two working directories in two
processes; assert the normalized artifacts are byte-identical *and* that the
normalization removed only fields on the declared exclusion list.

---

## 3. §25 still gates readiness on the pre-bakeoff host list

**Classification:** specification defect
**Where:** §25, line 953, against §17 lines 729–743 and §21

Round one's finding 9 was resolved in §4 and §17: the diagram is generic, the
table is explicitly "a set of bakeoff hypotheses, not architecture," components
"may be combined, removed, or replaced," and Rust joins as a candidate semantic
compiler.

§25 did not follow. Its readiness gate still reads: "Odin, Taichi, Julia,
Python, and Blender boundaries have reproducible builds."

So 1.0 readiness is gated on reproducible builds for a specific component set
that the §21 bakeoff is explicitly free not to select, while omitting Rust,
which §17 now names as a candidate. If the bakeoff picks Rust and drops Odin,
the readiness criterion becomes both unsatisfiable and wrong.

**Resolution:** restate the gate against the outcome rather than the
hypothesis — every component selected by the §21 bakeoff, and every boundary
between them, has reproducible builds.

**Fixture:** none; this is an internal-consistency fix.

---

## 4. The extension envelope's relationship to the digest is unspecified

**Classification:** specification defect
**Where:** §13.1, lines 559–563, with §13.2, lines 595–597

Draft 0.2 resolves round-one finding 8 well: IR is closed to unknown fields in
every mode, and forward-compatible data moves into an explicit extension
envelope where "readers preserve declared ignorable extensions byte-for-byte."

What is not stated is whether those preserved extensions participate in the
canonical digest. §13.2 says hashes are calculated over the normative canonical
encoding "including compiler version and registry digest," and says nothing
about extensions. Both readings are defensible and both are harmful:

- **Inside the digest:** two IR documents that are semantically identical but
  carry different ignorable extensions hash differently. They miss cache
  (§14), and content-addressability stops tracking semantics.
- **Outside the digest:** the content address does not cover content the reader
  is required to preserve, so an ignorable extension can be altered or
  substituted without changing the digest — and §2.11 requires provenance to
  survive compilation.

**Resolution:** state it explicitly. The defensible position is that ignorable
extensions are covered by a separate, recorded extension digest, with the
semantic digest computed over the canonical encoding excluding them — so cache
keys track semantics while preserved content is still tamper-evident. Whichever
is chosen, §13.2 must say so.

**Fixture:** two IR documents differing only in an ignorable extension; assert
the semantic digest matches, the extension digest differs, and a semantic cache
keyed on the former hits.

---

## Summary

| # | Finding | Class |
|---|---|---|
| 1 | Conditional nested key lets a sibling edit reroll an existing stream | spec defect |
| 2 | "Normalized artifact" undefined, making Grade A unfalsifiable | spec defect |
| 3 | §25 readiness gate hardcodes the pre-bakeoff component list | spec defect |
| 4 | Extension envelope's participation in the canonical digest unstated | spec defect |

Findings 1 and 2 are the ones that matter: both undermine guarantees that
round one was specifically raised to establish, and both are cheap to close
now. Findings 3 and 4 are consistency and definition work.

Draft 0.2 is a substantially stronger document than 0.1. The invariant set is
coherent, the mechanisms now discharge the MUSTs they are attached to, and §19
turns each resolution into a test rather than a promise.
