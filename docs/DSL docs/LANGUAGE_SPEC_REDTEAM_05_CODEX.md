# Red team 05 — Luxel model-native language specification

**Reviewed document:** `LUXEL_LANGUAGE_SPEC.md`, draft 0.5, 2026-08-03  
**Reviewer:** Codex  
**Date:** 2026-08-03  
**Ticket:** `REVIEW-1`  
**Scope:** Current normative specification and its stated migration contract; no
canonical specification or implementation was edited.

## Verdict

**Do not freeze draft 0.5.** Red-team 04's parser, ordering, digest, identity,
and numeric blockers remain applicable until a normative 0.6 resolves them.
This pass found three additional freeze-relevant gaps and one migration
contradiction demonstrated by the live prototype tests.

The central architectural decision remains sound: source is parsed rather than
executed, typed IR is authoritative, and numerical backends do not get to
reinterpret intent. The remaining work is not a change of direction. It is the
last mile that prevents independent compilers/backends from making different
silent decisions.

Severity follows §20: **High** means a conforming implementation can diverge
on load-bearing behavior; **Medium** means an implementation must invent a
policy the specification has not selected. Classifications are specification
defect (**SD**), implementation defect (**ID**), and missing conformance test
(**MCT**).

## Findings

### C5-01 — Product profile is absent from the canonical build identity (High, SD)

§11.5 makes the selected product profile a semantic contract: `asset.render@1`
and `asset.gameplay@1` can require different closed artifact inventories,
validators, absence reasons, collision representation, sockets, and LODs. Yet
the required IR envelope in §13.2 has no `product_profile` or
`product_profile_digest`; §13.3 does not include one in `semantic_digest`; and
§14 phase 1 validates target, requested capabilities, seeds, and evidence but
does not identify the profile as an immutable build input.

Minimal counterexample:

1. Compile the same `Asset` to target `blender.glb` under
   `asset.render@1` and `asset.gameplay@1`.
2. The latter requires collider/socket/LOD products while the former may not.
3. Both builds can have the same source, target, requested capabilities, and
   semantic IR digest, despite different certified artifact manifests and
   acceptance obligations.

This breaks the claim that the IR expresses the complete request consumed by
construction/certification. A cache can also reuse an artifact-assembly or
certification result produced under the wrong profile unless each backend
silently introduces its own non-canonical key.

**Normative repair:** add `product_profile_id` and
`product_profile_digest` to the required IR envelope. The build request MUST
select exactly one profile before default resolution; phase 1 validates it;
`semantic_digest` and every profile-sensitive construction, assembly, and
certification cache key include its digest. A profile change MUST alter
`semantic_digest` whenever it changes required products, allowed absence
reasons, validators, or acceptance thresholds. If profiles are intentionally
non-semantic packaging choices, §11.5 must narrow their authority so they
cannot alter any of those things.

**Fixture:** `fixture.profile.identity_and_cache_isolation` — identical asset
source compiled under render and gameplay profiles has the specified distinct
IR/build identities; cross-profile cache reuse is rejected; an equivalent
profile alias with the same pinned digest is accepted.

### C5-02 — Compiler provenance and semantic identity are conflated (Medium, SD)

§13.2 requires `compiler_version` in every IR document. §13.3 says that
`semantic_digest` covers compiler versions and is the semantic cache key that
changes only when compiled meaning or a meaning-producing version changes.
§18, however, defines patch releases as defect fixes that do **not** change the
meaning of valid programs.

Minimal counterexample: a compiler `1.0.1` and `1.0.2` differ only in a
diagnostic or parser robustness fix. They emit the same typed declarations,
defaults, constraints, and construction plan for a valid source file. If the
raw producer `compiler_version` enters `semantic_digest`, the semantic cache
misses and the IR semantic identity differs even though §18 says meaning did
not. If it does not enter, the literal §13.2/§13.3 wording is false.

This is not a reason to hide compiler provenance. It is a reason to separate
the two concepts.

**Normative repair:** define a registry-pinned
`semantic_compiler_compatibility_id` (or equivalent semantic frontend digest)
that enters `semantic_digest`. Keep the exact producer `compiler_version`,
build hash, and toolchain in provenance/document identity. A patch release may
reuse a compatibility ID only when a conformance proof establishes unchanged
valid-program semantics; otherwise it issues a new compatibility ID. Replace
the unqualified "compiler version" inclusion wording in §13.3 accordingly.

**Fixture:** `fixture.digest.compiler_patch_compatibility` — two producer
versions with one compatibility ID produce equal semantic digests and distinct
document provenance; a semantic compiler change produces a distinct semantic
digest and invalidates semantic caches.

### C5-03 — Compiler-controlled module dependencies have no authoring or build-request contract (Medium, SD)

§5.1 says a source file describes one module. §5.3 permits dependency cycles
only through compiler-controlled modules or typed registry references. §5.5
says imports select compiler-owned registry names, while relative/user package
imports are deferred; it also says a 1.0 program may span compiler-controlled
modules whose complete dependency graph is resolved and content-hashed.

Nothing specifies how a source module or build request names one of those
other modules, how symbols are imported from it without violating the
registry-only import rule, which identity/digest is bound, or how a module's
`module_id` survives a graph relocation. `dependency_digests` appears in the
IR envelope but has no schema beyond an untyped array.

Consequently two implementations can make incompatible choices: one treats a
build request's ordered file list as the graph, another uses a registry package
map, and a third disallows multi-module sources despite the promise. All can
claim the current text.

**Normative repair:** either defer multi-source programs from 1.0 explicitly,
or define a closed build-request module manifest: stable `module_id`, source
digest, declared registry-versioned exports, imports by `(module_id, symbol)`,
and a total dependency ordering/cycle rule. Split `dependency_digests` into
typed semantic and document/provenance identities consistent with the repair
for K4-03. The source subset must state the one approved cross-module import
form, or state that only the authorized build manifest supplies such bindings.

**Fixture:** `fixture.modules.two_module_binding_and_rename` — a two-module
program binds an exported immutable declaration, records the required typed
dependency identities, rejects an undeclared import and a dependency cycle,
and has specified behavior when a source file moves while its stable module ID
does not.

### C5-04 — The stated migration acceptance condition conflicts with the live prototype suite (High, SD + ID + MCT)

§15 says lenient mode emits candidate repaired source and a repair manifest,
**does not emit certifiable IR**, and that only strict IR may be consumed by
certification or scaffold generation. §23 requires the replacement to capture
every current `worldbuilder_dsl.py` accepted/rejected fixture and ends with:
"No replacement is accepted until it passes the prototype's tests plus the new
conformance requirements."

The current test `tests/test_worldbuilder_dsl.py::LenientAuthoringTests::test_lenient_output_still_applies_to_a_zone`
asserts the opposite operational behavior: it compiles an invalid input with
`lenient=True`, passes that result directly to `apply_intent`, and asserts the
clamped value is realized. The preceding test likewise asserts clamping of
out-of-range values. This is not merely historical documentation: it is an
executable current-prototype contract.

No replacement can both pass that test unchanged and conform to §15's strict
promotion boundary. The current prose leaves a team free either to weaken the
security/certification boundary or quietly delete a behavior it promised to
preserve.

**Normative repair:** revise §23 to distinguish *characterization fixtures*
from *must-preserve conformance fixtures*. The migration manifest MUST list
each prototype test with one of: `preserve`, `replace-with-strict-promotion`,
or `retire`, together with rationale and a new Luxel conformance fixture. Mark
the lenient direct-application test as `replace-with-strict-promotion`:
lenient output may be previewed only in a clearly non-certifiable sandbox;
application/certification must consume the acknowledged strict recompilation.
Do not claim the new implementation passes the unchanged prototype suite.

**Fixture:** `fixture.migration.lenient_preview_requires_strict_promotion` —
a clamped candidate can be displayed with its manifest, but is rejected by
artifact/world application and certification until its edits are acknowledged
and strict compilation succeeds. The migration manifest test asserts that
every prototype fixture has an explicit disposition.

## Prior-finding status relevant to this pass

The following red-team 04 findings still block a 1.0 freeze because draft 0.5
is the reviewed canonical document and has not yet received their normative
repair: K4-01 through K4-06. In particular, positional operand legality,
array ordering, digest-preimage membership, mask syntax, comparison typing,
and module/seed assignment must be settled before the parser whitelist or IR
schema is pinned. K4-07 through K4-13 remain required before final freeze.

This report does not re-litigate their proposed wording. C5-01 through C5-04
are additive findings discovered while checking the authority flow from source
through profile selection, module resolution, migration, and certification.

## Freeze recommendation

1. Cut draft 0.6 to resolve K4-01 through K4-06 and C5-01.
2. Pin a typed build-request schema at the same time as the IR envelope; do
   not leave profile and module dependencies as backend-local state.
3. Write the prototype-fixture migration manifest before claiming compatibility
   with `worldbuilder_dsl.py`.
4. Run a fresh independent red team against 0.6, then make C5-02 through C5-04
   executable conformance/migration tests before freezing 1.0.

## Verification performed

- Read current draft 0.5 normative sections 2, 5–8, 11–19, 23–25.
- Inspected current `pipeline/worldbuilder_dsl.py` lenient behavior and
  `tests/test_worldbuilder_dsl.py` migration-relevant tests.
- Compared against red-team 04 only to avoid duplicating already-known K4
  blockers.
- Did not modify canonical Luxel specification, compiler, registry, or tests.
