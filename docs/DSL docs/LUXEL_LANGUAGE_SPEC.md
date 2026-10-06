# Luxel model-native language specification

**Status:** frozen core implementation contract (2026-08-04); language 1.0 release gates remain open  
**Draft:** 0.6 — 2026-08-03  
**Target:** Luxel language 1.0  
**Existing implementation:** `pipeline/worldbuilder_dsl.py`, intent schema
`codeweald.world-intent/v1`

The frozen file set and verification command are pinned by
`tests/dsl_conformance/freeze_v06.json`. “Frozen” has the scoped meaning in
§25: implementations may target the core source/IR contract, while §24 release
decisions and unstable registry vocabulary remain outside this freeze.

Draft 0.2 incorporated all fifteen findings from
`LANGUAGE_SPEC_REDTEAM_01.md`. Draft 0.3 incorporated all four findings from
`LANGUAGE_SPEC_REDTEAM_02.md`. Draft 0.4 incorporated the accepted and refined findings from
`deepseek_redteam_01.md`: numeric canonicalization, stable identity scope,
trusted evidence manifests, cache/provenance semantics, repair classes,
independent constraint certification, model-evaluation reproducibility,
extension isolation and cache dependencies, source-map lineage, feasibility classes, strict
promotion of lenient output, and explicit cross-domain transforms.

Draft 0.5 incorporates the accepted and refined findings from
`LANGUAGE_SPEC_REDTEAM_03_KIMI.md`: unambiguous digest preimages, closed
stochastic identity, evidence-pin authority, binding and literal semantics,
numeric edge behavior, module-seed resolution, feasibility labels,
self-contained extensions, race-resistant evidence resolution, product
profiles, target-conditioned defaults, complete nominal-angle operations, and
selected-component readiness. Historical changelog claims describe the intent
of each resolution; the normative body and conformance fixtures remain the
authority.

Draft 0.6 incorporates the consolidated closure ledger in
`LANGUAGE_SPEC_REDTEAM_06_SOL_FINAL.md`: total canonical array ordering,
complete digest-domain membership, stable manifest-owned module identity,
reserved module-seed syntax, product-profile identity, legal mask composition,
operand-determined comparison types, deterministic unit conversion,
semantic-compiler compatibility identity, collection and identifier rules,
typed strict/lenient boundaries, and the prototype migration-disposition
contract. Findings merged or downgraded by that integrity review retain the
review's final classification rather than their earlier provisional severity.

This document defines the intended boundary of Luxel's model-native authoring
language. It is normative where it uses **MUST**, **MUST NOT**, **SHOULD**, and
**MAY**. Sections marked **Open decision** are deliberately unresolved and must
not be implemented as accidental policy.

The 1.0 specification is complete when every normative rule has a conformance
test, every standard constructor has a typed IR mapping, and every supported
backend either implements or explicitly rejects each required capability.

## 1. Purpose

The Luxel language describes world, asset, geometry, gameplay, and policy intent
in a compact form that humans and language models can author safely. It does
not expose a general-purpose runtime. The compiler turns source into a typed,
versioned intermediate representation (IR); deterministic backends turn that
IR into certified artifacts.

The language exists to move exact work out of model weights. A model chooses
semantic structure and aesthetic intent. Parsers, type checkers, solvers,
geometry kernels, validators, and engine adapters enforce the consequences.

The source syntax is intentionally Python-shaped because small models already
know how to produce and repair it. Familiar syntax does not imply Python
execution or Python semantics beyond the subset specified here.

### 1.1 Normative terminology

- **Canonical** means the unique byte encoding selected by this specification
  for one typed value; it does not mean merely equivalent after parsing.
- **Evidence** is digest-pinned external input plus its declared interpretation,
  trust state, coordinate metadata, and provenance. Evidence can inform intent
  but cannot silently redefine it.
- **Source-valid** means parsing, safety, binding, and type/domain checks passed.
  **IR-valid** additionally means canonical semantic IR and statically decidable
  hard constraints passed. **Constructed** means a backend produced a complete
  realized artifact. **Certified** means every applicable independent artifact
  validator and acceptance gate passed. These states MUST NOT be conflated.
- A **scaffold** is compiler-generated source that is syntactically valid,
  contains stable explicit identities, and round-trips to the same semantic IR
  under its pinned language, compiler, and registry versions.
- A **stable key** is an authored identifier whose meaning and stochastic stream
  do not depend on source location, declaration order, or unrelated edits.
- **Ranked attribution** is a bounded, ordered contributor set with measurement
  evidence and confidence; it is not a claim of singular causality.
- A **conservative bound** rejects some potentially buildable inputs if needed
  but MUST NOT accept an input claimed statically buildable that violates the
  stated bound on a conforming target.
- **Fuzz-tested** means exercised by a versioned, reproducible fuzz campaign with
  recorded corpus, engine/version, seed or replay inputs, budgets, sanitizer or
  equivalent failure instrumentation, and retained minimizing regressions.

## 2. Normative design invariants

These invariants are harder constraints than surface convenience:

1. **Parsed, never executed.** Model-authored source MUST NOT be imported,
   evaluated with `eval`/`exec`, or passed to a Python interpreter as code.
2. **IR is authoritative.** Backends MUST consume typed IR, never source text.
3. **Legal means buildable.** A strict program accepted for a declared target
   MUST satisfy all statically knowable target bounds and cross-parameter
   feasibility rules. A later gate may reject evidence-dependent output, but
   its diagnostic MUST report either the responsible declaration/parameter or
   a ranked set of contributors and an explicit `global` attribution when no
   single local cause is truthful.
4. **Deterministic by default.** Equal source, inputs, compiler and registry
   versions, capabilities, and seeds MUST produce byte-equivalent canonical IR.
   Reference production backends MUST also produce byte-identical canonically
   encoded artifacts across working directories and processes. A backend that can
   provide only tolerance-bounded numerical determinism must declare that lower
   grade and cannot satisfy a byte-deterministic target.
5. **Randomness is explicit.** No backend may use ambient or time-derived
   randomness.
6. **No silent repair or omission.** Unknown, unsupported, clamped, ignored, or
   dropped declarations are errors in strict mode.
7. **Every rejection is actionable.** When a local repair is known, the
   diagnostic MUST include a machine-readable edit that produces syntactically
   and semantically valid source.
8. **Recognition beats recall.** The toolchain MUST be able to scaffold a valid,
   round-trippable source file from an existing IR/artifact. Models SHOULD edit
   scaffolds rather than begin from a blank file.
9. **Coordinates and units are typed.** Bare numbers MUST NOT silently cross
   coordinate spaces or physical dimensions.
10. **Symmetry is explicit.** Geometry MUST NOT acquire mirrored backs, sides,
    or parts unless the source requests a symmetry operation.
11. **Provenance survives compilation.** Derived artifacts MUST retain their
    source, evidence, compiler, backend, and parameter lineage.
12. **Capability failure is explicit.** A backend MUST reject unsupported IR;
    it MUST NOT substitute a vaguely similar operation.
13. **Authoring difficulty is a compatibility dimension.** A change that makes
    representative tasks materially harder for small models is a language
    regression even if it increases theoretical expressiveness.
14. **Gates serve declared intent.** An acceptance gate MUST NOT be considered
    satisfied by degrading declared aesthetic, physical, or gameplay intent.
    When a gate rewards such degradation, the gate is defective; the result is
    a specification/instrument finding, not an authoring failure.

## 3. Scope of Luxel language 1.0

Version 1.0 SHALL standardize:

- the safe Python-shaped language kernel;
- immutable declarations, typed references, modules, and source locations;
- primitive types, physical units, coordinate spaces, collections, and enums;
- deterministic expressions and explicit seeded variation;
- hard constraints, bounded soft objectives, and solver budgets;
- typed policies and backend capability requirements;
- the canonical IR, diagnostics, provenance, and versioning contracts;
- a world domain sufficient to subsume the current WorldBuilder landform
  authoring surface;
- an asset/geometry domain sufficient to describe the initial benchmark
  families: swords, rocks, bottles, and trees;
- material assignments, collision intent, sockets, and LOD intent;
- Blender asset construction and Luxel world compilation as reference backends;
- conformance, security, fuzz, round-trip, and model-usability tests.

Version 1.0 SHALL NOT attempt to standardize arbitrary gameplay scripting,
general simulation code, shader languages, arbitrary user-defined types,
unbounded procedural generation, package distribution, or every possible
geometry algorithm. These can be added through versioned domains after their
requirements are demonstrated.

## 4. Architecture and authority boundaries

```text
Luxel source (Python-shaped text; extension selected by §24)
        │ validate build request; parse, validate syntax, bind names
        ▼
typed bound model (compiler-internal, not interchange authority)
        │ resolve defaults, units, constraints; negotiate capabilities
        ▼
canonical typed semantic IR (language authority)
        │ lower without semantic reinterpretation
        ▼
resolved construction plan
        ├── semantic compiler/driver (host selected by §21 bakeoff)
        ├── registered spatial/numerical backends
        └── registered engine/artifact adapters
        ▼
certified artifacts + manifests + diagnostics
```

The language frontend owns meaning. Numerical backends own computation, not
interpretation. Engine adapters own realization, not semantic invention.

Odin, Taichi, Julia, Python, Rust, and Blender form one candidate realization,
not mandated architecture. Python MAY remain an implementation language during
migration. No host implementation detail is part of the language contract.

## 5. Source model

### 5.1 Files

A source file describes one authored module. Source MUST be UTF-8. Tabs in
indentation are forbidden. Newlines and comments follow Python lexical
conventions.

Every compilation begins from a closed build-request manifest. The manifest
contains a stable `package_id`, exactly one authored `root_module_id`, the
root's `source_digest`, a required package seed, the selected target and product
profile, requested capabilities, the authorized evidence manifest, and every
compiler-controlled module dependency. `module_id` is assigned by this
manifest and MUST NOT be derived from a filename, absolute path, working
directory, source order, or content digest. Moving a source file while
preserving its manifest entry therefore preserves semantic and stochastic
identity; changing `module_id` is an explicit semantic migration.

A dependency entry contains its stable `module_id`, kind (`registry` in 1.0),
pinned semantic digest, optional document digest, exported immutable symbols,
and dependency module IDs. The complete graph is validated and content-hashed
before source binding. Additional user-authored source modules and imports
between them are deferred beyond 1.0; a 1.0 source module may import only
compiler-controlled registry modules named by the manifest. Registry cycles
are errors and include the ordered cycle in the diagnostic.

The language 1.0 source extension is `.luxel`. `.py` is forbidden for canonical
source and scaffolds because Luxel source is unsafe to execute and is not Python.
Migration tooling MAY read explicitly identified legacy `.py` fixtures but must
not emit them as Luxel source.

### 5.2 Allowed syntax

The parser SHALL initially accept only these Python AST concepts:

- module;
- restricted `from ... import ...` statements;
- immutable single-target name assignment;
- expression statements containing approved declaration calls;
- calls with explicit positional slots defined by the constructor and named
  keyword arguments;
- names bound by approved imports or prior immutable declarations;
- restricted attribute access used only for typed domain declarations;
- string, integer, finite floating-point, boolean, and the Python `None`
  literal;
- unary numeric negation;
- dimensionally typed binary `+`, `-`, `*`, and `/` expressions;
- typed `<`, `<=`, `==`, `!=`, `>=`, and `>` predicates used to construct
  constraints or masks;
- bounded list and string-keyed dictionary literals.

The parser MUST reject:

- arbitrary imports, aliases, and wildcard imports;
- function or class definitions;
- loops, comprehensions, generators, conditionals, pattern matching, lambdas,
  exceptions, context managers, and asynchronous syntax;
- mutation, augmented assignment, deletion, attribute/subscript assignment,
  and rebinding;
- decorators;
- `*args`, `**kwargs`, and unpacking;
- arbitrary attribute traversal;
- f-strings and runtime formatting;
- arbitrary operator overloading or operators outside the specified subset;
- tuple literals, chained comparisons, and Python boolean operators `and`,
  `or`, `not`, and `^`; registered constructors provide the corresponding
  domain operations;
- calls not registered in the selected language/domain version;
- non-finite floating-point values;
- any construct whose resource use cannot be bounded before execution.

`None` is the sole null spelling in source and lowers to the canonical IR null
for `Option::None`. Bare `null` is an ordinary unresolved name and produces a
binding diagnostic that MAY suggest `None`; it is not a second literal.

List literals lower to the sole 1.0 sequence value `List<T, N>` and preserve
authored order. Tuple literals are rejected with a corrective edit to a list
when the expected type is a list. Dictionary literals lower to immutable
`Record`; keys MUST be string literals and duplicate decoded keys are a parse
error even when their source spellings differ. Implementations MUST detect
duplicates before constructing a host-language map and MUST NOT use
last-write-wins behavior.

### 5.3 Immutable declarations

Unlike the current landform-only prototype, 1.0 MUST permit references to prior
immutable declarations. Asset construction requires named parts:

```python
blade = sweep(path=blade_path, profile=diamond(width=52*mm, depth=7*mm))
guard = sweep(path=guard_curve, profile=oval(width=18*mm, depth=9*mm))
sword = asset(
    parts=[blade, guard],
    constraints=[connected(first=blade, second=guard)],
)
```

Each name is assigned exactly once. A reference may point only to a prior
declaration visible in the current module or an imported immutable symbol.
Forward references within a module are forbidden in language 1.0; binding is a
single source-order pass. Cycles are therefore possible only through compiler-
controlled module dependencies or typed registry references; they are compile
errors and MUST include the reference cycle in the diagnostic.

Declarations are values, not host objects. Attribute access is allowed only
where the type registry defines it, such as `blade.centerline`; there is no
general object model or reflective lookup.

### 5.4 Calls and arguments

Standard constructor operands are keyword-only in language 1.0. A constructor
MAY reserve its first positional slot solely for a stable identity or
stochastic `key` when that slot is declared by its versioned signature; all
other positional arguments—including relational operands to `connected`,
`require`, or comparison helpers—are errors. The signature, positional identity
slot if any, defaults, units, bounds, and capability requirements of every
constructor are versioned registry data. Normative examples use keyword form.

Unknown arguments and duplicate arguments are errors. Defaults are resolved
into canonical IR so backend behavior never depends on an implicit default.

### 5.5 Imports and domains

Imports select names from a compiler-owned registry; they do not load files or
execute modules. The standard 1.0 namespaces are `luxel.core`, `luxel.geometry`,
`luxel.units`, `luxel.asset`, and `luxel.world`; submodules require a versioned
registry addition. The intended shape is:

```python
from luxel.core import World, asset, require
from luxel.geometry import line, diamond, sweep, connected
from luxel.units import mm, m, deg
```

The current `from worldbuilder import ...` form SHALL remain supported through
a migration frontend until its source and scaffold corpus has been upgraded.

Imports and declarations occupy one immutable module namespace. An imported
name cannot be rebound, shadowed, or reused as a local declaration; attempting
to do so is a binding error with both spans and, when collision-free, a rename
repair. Multiple imports of the same canonical symbol under the same name are
idempotent; imports that bind the same name to different symbols are errors.

User packages, relative imports, and imports between user-authored source files
are deferred beyond 1.0. A 1.0 program may depend on compiler-controlled
modules only through registry imports authorized by the build-request manifest
in §5.1. A registry import binds the manifest-pinned `(module_id, symbol,
semantic_digest)` triple; a missing export, undeclared dependency, digest
mismatch, or dependency cycle is a binding error rather than a resolver
fallback.

## 6. Type system

### 6.1 Core types

The 1.0 core SHALL include:

- `Bool`, `Int`, `Float`, `String`, `Identifier`, `Digest`;
- `Enum<T>` and closed tagged unions defined by a domain registry;
- `Option<T>`;
- bounded `List<T, N>` and immutable string-keyed `Record`;
- `Quantity<Dimension>`;
- `Vec2<Space, Dimension>`, `Vec3<Space, Dimension>`;
- `Rotation<Space>` and `Transform<From, To>`;
- `Reference<T>`;
- `Seed`;
- source/evidence types including `Image`, `Camera`, `Mask`, and `Landmark`;
- geometry types including `Curve`, `Profile`, `Field`, `Solid`, `Surface`, and
  `Mesh`;
- world types including `World`, `Entity`, `Feature`, `Asset`, `Part`,
  `Material`, `Collider`, `Socket`, `LOD`, `Policy`, and `Constraint`.

Domain types are nominal. A `Curve` is not accepted where a `Profile` is
required merely because both contain points.

#### 6.1.1 Names and identifier-valued strings

Source binding names are ASCII identifiers matching
`[A-Za-z_][A-Za-z0-9_]*`, excluding Python keywords and registry-reserved names.
`module_seed` is the only language-reserved metadata binding in 1.0. Registry
symbol names obey the same grammar within their namespace.

An `Identifier` value is not a source binding name. Evidence logical IDs,
manifest IDs, registry-qualified IDs, and other externally supplied identifiers
are NFC-normalized Unicode strings with registry- or schema-declared length and
byte limits. They MUST remain quoted values and MUST NOT be emitted as source
bindings. When a scaffold needs a binding for an external ID, it generates a
separate collision-free ASCII binding and records the relationship in the
source map.

Canonical scaffolds emit string and `Identifier` values as double-quoted,
JSON-compatible literals: quote, reverse solidus, and U+0000–U+001F are escaped
using the JCS string rule; other NFC text is preserved. Parsing the emitted
literal MUST reproduce the identical Unicode scalar sequence. This rule, not a
universal identifier grammar, prevents external evidence text from becoming
source syntax.

### 6.2 Numbers and coercion

Integers and floating-point values are distinct in source. An integer MAY widen
to floating point when the target type permits it. Floating point MUST NOT
implicitly narrow to integer. Booleans are never numbers.

Source integer literals and exact integer results are limited to
`[-(2^53-1), 2^53-1]` in language 1.0. A value outside that range is rejected
during type checking unless a future registered tagged exact-integer type is
the expected type; widening an out-of-range integer to `Float` is not an
implicit escape. Unary negation is evaluated before this range check so the
negative endpoint is accepted symmetrically.

For dimensionless operands, `/` widens integer operands as needed and always
produces `Float` using binary64 division. Language 1.0 has no integer-division
or remainder operator; `//` and `%` are rejected. Quantity division subtracts
dimension vectors and uses binary64 for the resulting magnitude. Division by
zero and any arithmetic result that is NaN or infinite are semantic-evaluation
errors before IR emission.

Compiler and IR arithmetic MUST define precision and rounding. The 1.0 rule is
IEEE-754 binary64 during semantic evaluation and explicit backend
narrowing recorded in the construction manifest.

Each primitive semantic arithmetic operation rounds to IEEE-754 binary64 using
round-to-nearest, ties-to-even; implementations MUST NOT contract multiple
source operations into a fused operation when that changes the rounded result.
Subnormal values are preserved. `+0.0` and `-0.0` compare equal under language
equality, and JCS encodes both as the single number `0`; canonical hashes are
over JCS bytes and never over host floating-point bit patterns. Operations
whose result depends on the sign of zero are not part of the 1.0 constant-
evaluation vocabulary.

Canonical JSON uses the
[JSON Canonicalization Scheme (JCS), RFC 8785](https://www.rfc-editor.org/rfc/rfc8785),
including its ECMAScript-compatible finite binary64 number serialization.
Source forms
that evaluate to the same typed binary64 value, such as `10`, `10.0`, and
`1e1` after a permitted integer widening, therefore have one canonical numeric
encoding. NaN and infinities are forbidden before canonicalization. Untagged IR
integers use the same exactly interoperable source range; future domains
requiring larger exact integers MUST use a separately specified tagged string
representation. JCS governs encoding and
member ordering; the Luxel schema governs semantic field order, types, and digest
preimages.

### 6.3 Units

Physical quantities carry dimensions represented as integer exponent vectors
over a closed set of base dimensions. Multiplication adds exponents, division
subtracts them, and compatible addition/subtraction requires identical vectors.
Derived dimensions such as area, velocity, acceleration, and density are
computed by this algebra; they are not an enumerated registry list.

The 1.0 base-dimension set SHALL include at least length, time, and mass. Angle
is dimensionless in dimensional analysis but retains a distinct nominal type:
trigonometric functions accept angle, ordinary dimensionless ratios do not
implicitly become angles, and registered geometric operations such as
`arc_length(radius, angle)` explicitly map length × angle to length. Units are
immutable typed constants, allowing `52 * mm` while rejecting
`52 * mm + 3 * deg`.

`Angle` has the zero physical-dimension vector but is a nominal type that does
not unify with `Ratio` or ordinary dimensionless `Float`. Its 1.0 algebra is
closed and explicit: `Angle + Angle -> Angle`, `Angle - Angle -> Angle`,
`Angle * Ratio -> Angle`, `Ratio * Angle -> Angle`, `Angle / Ratio -> Angle`,
and `Angle / Angle -> Ratio`. Ordering and equality compare `Angle` only with
`Angle`. `radians(Angle) -> Ratio` and `degrees(Angle) -> Ratio` extract numeric
measures; `angle_from_radians(Ratio) -> Angle` and
`angle_from_degrees(Ratio) -> Angle` construct angles. There is no implicit
`Length * Angle` operation; arc length uses the registered `arc_length`
constructor so dimensional analysis cannot accidentally erase the angle type.

Domain constructors MUST reject unqualified bare numbers for dimensional
parameters. Ratios and normalized coordinates remain dimensionless and MUST
state their valid interval.

All canonical IR quantities use declared base units. Source spelling is kept in
provenance but does not affect semantics.

Each unit registry entry stores a typed base-unit scale as a canonical finite
binary64 JCS number. Applying a unit constant is one ordinary binary64
multiplication under §6.2 after the source numeric operand and registry scale
have each been converted to binary64 using round-to-nearest, ties-to-even. It
is not host decimal arithmetic, extended precision, or a backend-selected
conversion. The resulting canonical quantity is a record containing its JCS
binary64 magnitude, closed dimension vector, and nominal type where applicable.
Registry conformance vectors pin every standard unit scale and boundary
conversion.

### 6.4 Coordinate spaces

Vectors and transforms carry coordinate-space types. The initial registry SHALL
distinguish at least:

- normalized source-image space;
- camera/view space;
- asset-local space;
- world space;
- engine/backend space.

Cross-space operations require an explicit `Transform`. Axis orientation,
handedness, up axis, and scale conversion MUST appear in backend manifests.
An asset declares its local origin, basis, and unit scale. Placement requires a
typed `Transform<AssetLocal, WorldSpace>` even when its numeric value is the
identity; a scaffolder MAY emit that explicit identity transform. Conversion
from world space to engine/backend space is likewise a registered explicit
transform. No two named spaces are implicitly interchangeable.

## 7. Identity, references, and ownership

`Seed` is exactly 256 bits. Its sole source and canonical IR text form is
`seed256:` followed by 64 lowercase hexadecimal digits. It is constructed with
`seed(value="seed256:...")`; uppercase hex, integers, omitted prefixes, short or
long payloads, and arbitrary strings are rejected without normalization. Seed
bytes are the 32 decoded octets in display order. This pins representation and
interchange independently of the core registry's still-open stream
key-derivation and pseudorandom algorithm.

Stable user-visible entities and assets require explicit identifiers. Pure,
stateless compiler temporaries may receive deterministic content-derived IDs.
Any stochastic/stateful constructor instead requires stable semantic identity:
the fully qualified owning declaration ID plus a mandatory explicit key for
every stochastic operation, including when it is currently the only such
operation beneath its declaration. A fully qualified declaration ID includes
stable module identity and an authored declaration name. Source line, raw
occurrence index, and whole-program digest are forbidden as stochastic identity
because unrelated edits can change them. Scaffolds generate stable keys. A key
is unique within its owning declaration; duplicate keys under one owner are a
compile error. The same spelling in another declaration or module does not
alias or reuse a stream. Structurally equal stochastic declarations remain
distinct unless the author explicitly shares one declaration/reference or a
future registered named-stream value. IDs are unique within their declared
scope and MUST NOT be inferred from collection order.

Each module has exactly one `module_seed` in canonical IR. Source may provide it
only through this reserved top-level metadata assignment:

```python
module_seed = seed(
    value="seed256:0000000000000000000000000000000000000000000000000000000000000001"
)
```

It MUST appear after imports and before every ordinary declaration, may appear
at most once, is not an exportable declaration, and may not use a positional
operand. Any other assignment to `module_seed`, or use of that name as an
import, parameter, ordinary reference, or external binding, is a syntax or
binding error with a corrective diagnostic when unambiguous.

Resolution order is: (1) the explicit reserved assignment; otherwise (2) a
build-request module seed addressed to the stable manifest `module_id`;
otherwise (3) the domain-separated derivation of the build request's required
package seed and stable `module_id`. If explicit source and build-request module
seeds are both present and differ, compilation fails rather than choosing one.
Package seeds never act directly as module seeds, and equal local
declaration/key spellings in different modules remain isolated. Seed
representation and derivation test vectors are part of the core registry and
canonical IR schema.

References are resolved during semantic compilation. Missing, ambiguous,
wrong-type, and cyclic references are errors. The compiler MUST suggest the
nearest legal identifier when confidence is high enough and otherwise list the
available identifiers in bounded form.

Each declaration has one semantic owner. Backends may derive products but may
not redefine the declaration's meaning.

## 8. Evaluation and determinism

Luxel evaluation is declaration resolution, not general computation. Evaluation
MUST terminate under statically declared resource bounds.

Permitted computation consists of:

- literal construction;
- pure registered constructors;
- typed field access;
- dimensionally valid arithmetic;
- deterministic collection construction;
- explicit seeded variation;
- constraint and objective graph construction.

Iteration is expressed through bounded domain constructors such as `repeat`,
`scatter`, or `sample`, not source-language loops. Their maximum output count
is part of their signature and compilation budget.

Every module resolves the stable `module_seed` by §7 and records it in canonical
IR. Random streams derive solely from `(module_seed,
fully_qualified_declaration_id, stochastic_key, explicit_seed_or_absent)`.
`stochastic_key` is the mandatory key defined in §7. The explicit seed is a
typed `Seed` value or one distinguished absent sentinel with one canonical IR
encoding. Whole-program digests, source spans, AST paths, operation/lowering
ordinals, sibling positions, and ambient ordering are excluded. Adding an
unrelated declaration or non-stochastic operation MUST NOT perturb any pre-
existing declaration's realized output. The core registry pins the stream key-
derivation and pseudorandom algorithm plus cross-host test vectors; a backend
cannot substitute its ambient RNG and claim conformance.

## 9. Constraints and objectives

The language distinguishes:

- **static hard constraints** (`static_hard`), proven before IR acceptance from source, types,
  registry facts, and declared target capabilities;
- **realized hard constraints** (`realized_hard`), including construction- and evidence-dependent
  predicates that must pass on the completed artifact before certification;
- **soft objectives** (`soft_objective`), measured and optimized within explicit tolerances and
  budgets but never certification requirements unless a policy explicitly
  promotes them to a realized hard gate;
- **acceptance gates** (`acceptance_gate`), measurements performed on realized artifacts.

These four machine labels are the closed 1.0 validation/feasibility classes.
Evidence dependence is recorded as an input-dependency property of a
`realized_hard` constraint or `acceptance_gate`, not invented as a fifth class.
Registries and diagnostics MUST use these labels and MUST NOT substitute
`dynamic`, `best_effort`, or backend-local categories.

Comparison result type is determined only by operand types, never by expected
context or the consumer. Comparing two compatible non-field values produces
`Predicate<Scalar>`. Comparing `Field<Space, T>` with a compatible field or
scalar/quantity produces `Mask<Space>`. Mixed spaces or incompatible dimensions
are type errors. Comparisons do not coerce to `Bool`, Python truthiness does not
exist, and chained comparisons are rejected with a repair that expands them
into separately named predicates and an explicit registered composition
constructor where legal.

Illustrative source:

```python
require(predicate=connected(first=blade, second=guard))
require(predicate=thickness(part=blade.edge) <= 1.5*mm)
prefer(objective=match_silhouette(asset=sword, view=front_view), weight=0.8)
```

Constraints are declarative values. They do not run arbitrary callbacks.
Solver selection is a construction-plan concern, while tolerances, limits,
budgets, and reproducibility requirements are semantic IR.

Tolerance is part of each constraint's semantic IR, not ambient solver
configuration. Solver success is advisory: certification MUST independently
re-evaluate every realized hard constraint against the completed artifact with
a registered, version-pinned validator and its declared tolerance. Static hard
rules remain binding and cannot be overridden by a solver status. A validator
failure rejects certification even when the producing solver reports success.
An evidence-dependent failure leaves source and IR validity intact but prevents
the artifact from becoming certified.

A solver invocation MUST declare:

- supported constraint classes;
- iteration/time/memory budget;
- deterministic mode and seed behavior;
- numeric precision and tolerance;
- termination status;
- residuals and unsatisfied constraints;
- backend and version.

An unsatisfiable result MUST identify an actionable conflict set when the
solver can derive one. Time or iteration exhaustion is not success.

When a policy promotes a `soft_objective` to a certification requirement, the
compiler emits a new `realized_hard` constraint with stable ID
`promotion:<policy_id>:<objective_id>`, a provenance edge to both inputs, the
policy-pinned tolerance and validator, and no retained soft-only authority. The
label is exactly `realized_hard`; implementations MUST NOT invent a fifth
`promoted` validation class.

## 10. Policies

Policies are typed declarations, not unvalidated dictionaries. Luxel 1.0 SHALL
provide a policy-domain mechanism capable of representing the current
`acceptance_policy`, `traversal_policy`, and `border_policy` without losing
their validation rules.

Every policy field has a type, unit, default, legal bounds, semantic owner, and
affected gates. Legal bounds and cross-parameter predicates MUST be derived
from, or conservatively imply, the statically buildable region for every
declared target. The registry records the derivation or validation evidence.
When feasibility genuinely depends on world evidence and cannot be known
statically, the registry marks that condition evidence-dependent rather than
claiming a static bound; a later rejection MUST report the responsible policy
field or ranked contributor set.

Unknown policy fields are errors. Policy inheritance and merging are deferred
until a demonstrated use case defines unambiguous precedence semantics. The
only 1.0 composition is registry defaults followed by explicit field
replacement within one policy declaration. Defaults and the final resolved
values are materialized in IR. Policy-to-policy inheritance, entity-level
override chains, and implicit recursive dictionary merging are not 1.0
semantics.

## 11. Geometry and asset domain

### 11.1 Minimum 1.0 construction vocabulary

The reference geometry domain SHALL include typed forms of:

- analytic primitives;
- transforms;
- polylines and parametric curves;
- 2D profiles;
- extrusion, revolution, sweep, and loft;
- CSG union, intersection, and difference;
- signed-distance fields and bounded field modifiers;
- explicit symmetry and repetition;
- visual-hull constraints from calibrated silhouettes;
- part attachment and connectivity;
- material regions and projection strategies;
- collider, socket, and LOD declarations;
- mesh extraction and certification requirements.

This list specifies semantic categories, not final constructor names. A
constructor enters 1.0 only after it has a typed IR form, deterministic
reference implementation, validation contract, diagnostics, and fixture.

### 11.2 Semantic parts

Assets are graphs of named parts, attachments, and constraints—not monolithic
meshes. A sword may contain blade, guard, grip, wrap, pommel, fuller, and socket
parts. A tree may contain trunk and connected branch curves plus foliage
clusters. Backends may fuse parts into meshes while preserving part identity in
the manifest.

### 11.3 Visual evidence

Images are evidence, not topology authority. Each evidence item records its
content digest, role, camera model, visibility claim, crop/scale transform, and
review state. Inference that is not directly visible MUST be marked as inferred
and tied to a semantic prior or explicit author decision.

Every resolved evidence item is represented by a logical ID and content digest
in the IR `evidence_pins` inventory. A pin is marked `semantic` when changing
its identity can change declarations, topology authority, hard constraints, or
policy meaning; semantic pins participate in `semantic_digest`. Pins used only
by construction, appearance fitting, measurement, or certification remain
non-semantic inputs, participate in `document_digest`, provenance, and every
downstream cache key that reads them, and MUST still change the certified
artifact/build identity when their bytes change. Reviewed world topology
evidence is always semantic. An asset evidence constructor declares which
dependency class it uses in its versioned registry signature; source cannot
downgrade that class, and a backend cannot reclassify evidence after IR
emission.

Multi-view reconstruction MUST define view orientation and calibration.
Texture projection MUST NOT mirror one view onto an unseen side unless the
source explicitly requests that fallback.

### 11.4 Symbolic and numerical reconstruction

The semantic compiler lowers an asset into a construction graph. Taichi MAY
evaluate dense/sparse fields, visual hulls, projections, and differentiable
losses. Julia MAY solve parameter-fitting and inverse problems. Those backends
receive bounded typed plans; neither may reinterpret source semantics.

Optimization against evidence MUST preserve declared topology and hard
constraints. A lower image loss never authorizes breaking connectivity,
collision, scale, or gameplay requirements.

### 11.5 Asset product

A certified asset package is built under a versioned product profile such as
`asset.render@1` or `asset.gameplay@1`. Each profile declares a closed set of
required and optional products, validators, and absence reasons. A backend may
report a product as not applicable only when the selected profile marks it
optional or the IR contains a typed opt-out and profile-approved absence reason.

The build request selects exactly one registry-owned `product_profile_id` and
pinned `product_profile_digest` before default resolution. Both fields are
recorded in canonical IR. The digest covers the profile's product inventory,
absence reasons, defaults, validators, thresholds, collision/LOD/socket rules,
and any other artifact or certification authority. It participates in
`semantic_digest` and every profile-sensitive construction, assembly, and
certification cache key. Two profile aliases MAY share semantic identity only
when they resolve to the same canonical profile ID and pinned profile digest;
the authored alias is provenance only. A changed profile digest is a changed
semantic build request even when its canonical display ID is unchanged.

The profile manifest SHALL account for every standard product category below,
either with its required artifact/data or with a permitted typed absence:

- render mesh and material assignments;
- collision representation;
- sockets and semantic part map;
- LODs or a profile-approved explicit reason they are absent;
- unit, origin, orientation, and bounds;
- source program and canonical IR digest;
- evidence and input digests;
- compiler/backend versions and capabilities;
- topology, UV, material, and physical validation reports;
- warnings, unresolved evidence conflicts, and repair history.

## 12. World domain

The 1.0 world domain SHALL preserve the current ability to author landform
composition against reviewed feature IDs and semantics. It SHALL migrate
current policy authoring from hand-validated JSON into typed declarations in
increments without requiring Luxel development to stop.

### 12.1 Minimum 1.0 world vocabulary

The standard world domain SHALL provide typed semantic forms for:

- world bounds, scale, playable envelope, and coordinate transforms;
- reviewed source features, evidence claims, visibility, uncertainty, and
  reconciliation across images;
- terrain/landform composition, elevation fields, silhouette character,
  erosion intent, and border enclosure;
- scalar/vector fields for site conditions including elevation, slope, aspect,
  wetness, exposure, drainage, and accessibility;
- hydrology sources, flow/catchment fields, channels, water bodies,
  classification, and physical limits;
- masks/predicates and deterministic bounded placement;
- ecology including vegetation niches, canopy/groundcover density, snow/ice,
  scree/talus, and surface-material regions;
- route, navigation, and traversal intent tied to typed actor profiles;
- structure/settlement/objective siting, clearances, visibility, tactical
  relationships, and gameplay annotations;
- asset roles, affordances, physical envelopes, sockets, collision policy,
  variation, LOD, and runtime budgets;
- acceptance policies, measurements, evidence, and attribution;
- backend-independent render/placement plans and certified solver products.

As with §11.1, these are semantic categories rather than final constructor
names. A category is conformant only when its registry types and required
capabilities are specified.

### 12.2 Fields, masks, and conditional placement

World fields are typed functions over a declared coordinate space, for example
`Field<WorldSpace, Length>` for elevation or
`Field<WorldSpace, Dimensionless>` for normalized wetness. Comparisons over
compatible fields and quantities produce `Mask<Space>`. Masks support bounded
composition while retaining their source and sensitivity provenance. The 1.0
source surface is the registered constructor set
`mask_all(items=[...])`, `mask_any(items=[...])`,
`mask_xor(left=..., right=...)`, and `mask_not(item=...)`. Every input mask
must have the same coordinate space; empty `mask_all`/`mask_any` lists are
rejected rather than receiving an ambient identity value. Python `and`, `or`,
`not`, `^`, truthiness, and implicit chained-comparison lowering are not Luxel
mask syntax.

Bounded placement constructors such as `scatter` accept a required domain and
an optional mask. Their count/density, minimum separation, seed, identity key,
and resource ceiling are explicit. For example, “conifers above 900 m on north
aspects and outside bogs” is expressed as a composed mask, not source-language
control flow. A placement backend MUST emit zero instances outside a hard mask
within its declared numeric tolerance.

### 12.3 Policy inventory

The 1.0 registry migration SHALL inventory and type every policy field currently
accepted by Luxel, including at least acceptance, traversal, border/terrain,
hydrology, ecology/vegetation, surfacing/material, siting, collision,
affordance, LOD/runtime-budget, and evidence-reconciliation policy. The
authoritative inventory is generated from registries and compared against the
legacy JSON validators; prose lists cannot be the source of truth.

### 12.4 Solver products

Existing solver products—including site-condition fields, heightfields,
hydrology, forestry/ecology, siting, boundary, navigation/traversal, collision,
asset, surface/material, and render/placement plans—receive typed IR schemas,
input/output digests, coordinate/unit metadata, solver status, and acceptance
evidence. A solved plan is derived output, never an untracked policy override.

World source expresses semantic intent. Reviewed concept annotations remain
evidence/topology authority until a separately specified authoring operation
explicitly creates or removes topology.

World, asset, and future game domains share one kernel, diagnostic model, IR
envelope, unit system, coordinate-space type system, and versioning scheme.
They are namespaces, not separate dialects.

## 13. Canonical intermediate representation

### 13.1 Requirements

The canonical IR MUST be:

- language-neutral;
- fully typed after semantic compilation;
- deterministic and canonically serializable;
- versioned independently from source syntax;
- closed to unknown fields in every mode;
- source-mapped;
- capability-explicit;
- content-addressable;
- losslessly migratable between supported minor versions.

Forward-compatible data is carried only through a specified extension envelope
whose namespace, owner, critical/ignorable status, and preservation rules are
explicit. Readers preserve declared ignorable extensions byte-for-byte and
reject unknown ordinary fields or unknown critical extensions. There is no
mode in which unrecognized fields are silently dropped.

An extension envelope contains a namespace, version, criticality, media type,
opaque payload bytes, and payload digest. Opaque payload bytes are represented
as unpadded RFC 4648 base64url text in canonical JSON and are preserved exactly.
A known critical extension is decoded by its registered handler and participates in
semantic meaning. An extension is ignorable only when it cannot affect
normative semantics or the artifact for the selected target. A compiler that
recognizes artifact-affecting extension content MUST classify it as critical
for that target or reject the document; material, geometry, policy, gameplay,
or evidence changes can never be hidden in an ignorable envelope.

Extension records are self-contained in language 1.0 after their registered
namespace handler and registry version are selected. They MUST NOT declare or
implicitly depend on another extension record. A handler whose meaning requires
another extension, language feature, registry entry, or capability rejects the
document unless that requirement is part of its own versioned critical
contract. General inter-extension dependency graphs are deferred until a
measured use case justifies their ordering and failure semantics.

Canonical JSON under JCS (RFC 8785) is the required 1.0 interchange/debug
representation. A compact binary encoding MAY be added later, but it MUST
represent identical semantics and carry the canonical JSON digest.

### 13.2 Required envelope

Every IR document contains at least:

```json
{
  "schema_version": "luxel.ir/1.0",
  "language_version": "luxel.lang/1.0",
  "compiler_version": "...",
  "semantic_compiler_compatibility_id": "luxel.frontend/...",
  "registry_digest": "sha256:...",
  "semantic_digest": "sha256:...",
  "extension_digest": "sha256:...",
  "document_digest": "sha256:...",
  "module_id": "...",
  "module_seed": "...",
  "target": "...",
  "product_profile_id": "...",
  "product_profile_digest": "sha256:...",
  "source_digest": "sha256:...",
  "dependencies": [],
  "evidence_manifest_digest": "sha256:...",
  "evidence_pins": [],
  "required_capabilities": [],
  "declarations": [],
  "constraints": [],
  "policies": [],
  "extensions": [],
  "provenance": {},
  "source_map": {}
}
```

`compiler_version` is exact producer provenance. The
`semantic_compiler_compatibility_id` is a registry-pinned identifier for the
valid-program meaning implemented by that frontend; it enters semantic identity
under §13.3. Two compiler builds may share it only after passing the identical
language conformance suite. A diagnostic-only or robustness patch may therefore
preserve semantic identity without hiding which compiler produced the document.

Each `dependencies` entry contains exactly `module_id`, `semantic_digest`, and
optional `document_digest`. The semantic pair enters semantic identity; the
complete record enters document identity. A compiler MUST NOT substitute a
document digest where a semantic dependency identity is required.

`evidence_pins` contains at least logical ID, content digest, declared role,
dependency class (`semantic` or `construction`), media type, and
interpretation/coordinate metadata digest. The manifest digest covers the
complete authorized manifest, including unused entries, while only resolved
pins enter dependency-specific semantic and phase projections.

Defaults are explicit and quantities use canonical base units. Map member order
is JCS order. Every array-valued IR schema field is declared as exactly one of:

- **sequence** — order is semantic and the authored/resolved order is preserved;
  examples include curve points, transform composition, and an explicitly
  ordered LOD chain;
- **setlike** — order carries no meaning and canonical emission sorts by the
  field's registry-declared total key before hashing.

An unclassified array field is invalid IR. The 1.0 envelope uses these total
orders, comparing normalized strings by unsigned UTF-8 byte sequence and then
the remaining tuple members in the listed order:

| Array | Class and canonical key |
|---|---|
| `dependencies` | setlike: `(module_id, semantic_digest, document_digest-or-empty)` |
| `evidence_pins` | setlike: `(logical_id, dependency_class, content_digest, interpretation_metadata_digest)` |
| `required_capabilities` | setlike: `(capability_id, version, contract_digest)` |
| `declarations` | dependency-topological; at each Kahn ready-set step choose `(module_id, fully_qualified_declaration_id)`; duplicate IDs are invalid |
| `constraints` | setlike: stable constraint ID |
| `policies` | setlike: stable policy ID |
| `extensions` | setlike: `(namespace, version, criticality, media_type, payload_digest)` |
| provenance/reuse/repair events | setlike: `(phase_index, event_type, stable_event_id)`; causal parents remain explicit fields |
| source-map spans | setlike: `(source_digest, start_byte, end_byte, node_id)` |

All other arrays, including arrays nested in registry values, MUST receive their
sequence/setlike classification and total key from the pinned schema/registry.
Source reordering that does not change a sequence, dependency edge, or stable ID
preserves the semantic projection. Reordering a semantic sequence changes it;
reordering only a setlike input does not.

All 1.0 digest fields use SHA-256 over UTF-8 JCS bytes with a specified ASCII
domain tag and NUL separator prepended to the preimage. Text form is
`sha256:` followed by exactly 64 lowercase hexadecimal digits. Each digest
domain defines its own tag and included/excluded fields in the IR schema; no
implementation may hash a parsed host object using host-dependent ordering.

The source map is a bidirectional many-to-many index, not a bijection. Stable IR
node IDs/paths map to zero or more source spans, and source spans map to zero or
more IR nodes. A span records source digest, authoritative half-open UTF-8 byte
offsets, and derived line/column coordinates. Generated defaults, migrations,
solver products, and backend products may have no direct source span; they
instead carry typed provenance
edges to their generating operation and parent IR nodes. Transformations append
lineage and MUST NOT fabricate spans by proportional offset shifting. Artifact
part IDs map to IR node IDs and reach source through this lineage.

For 1.0, round-trip means that source → IR → scaffold → IR produces the same
`semantic_digest` under identical pinned language, semantic-compiler
compatibility ID, registry, build-request manifest, and product profile, and
that the scaffold is strict-valid source. Exact producer compiler versions may
differ without changing the semantic digest when they share the verified
compatibility ID. Comment and formatting preservation is outside this
guarantee; a future lossless concrete-syntax-tree mode would be a separate
capability.

### 13.3 Digest domains

The IR defines three domain-separated digests. Their tags are
`luxel.ir.semantic/1`, `luxel.ir.extensions/1`, and `luxel.ir.document/1`. The preimage
is the ASCII tag, one NUL byte, and the JCS encoding of the projection below.

| Field or projection | `semantic_digest` | `extension_digest` | `document_digest` |
|---|:---:|:---:|:---:|
| schema/language versions | yes | no | yes |
| `semantic_compiler_compatibility_id` | yes | no | yes |
| exact `compiler_version` and compiler build provenance | no | no | yes |
| `registry_digest` | yes | no | yes |
| `module_id`, `module_seed` | yes | no | yes |
| `target`, `product_profile_id`, `product_profile_digest` | yes | no | yes |
| `source_digest`, source map, diagnostics, repair history | no | no | yes |
| dependency `(module_id, semantic_digest)` identities | yes | no | yes |
| dependency document identities/provenance | no | no | yes |
| `evidence_manifest_digest` and unused manifest entries | no | no | yes |
| resolved `semantic` evidence pins | yes | no | yes |
| construction-only evidence pins | no | no | yes |
| required capabilities and their contract digests | yes | no | yes |
| declarations, constraints, policies, resolved defaults | yes | no | yes |
| recognized critical extensions | yes | no | yes |
| ignorable extension records and exact opaque bytes | no | yes | yes |
| `semantic_digest` field | excluded | no | yes |
| `extension_digest` field | no | excluded | yes |
| `document_digest` field | no | no | excluded |
| other provenance and histories | no | no | yes |

The semantic projection contains only the rows marked `yes`, emitted under the
canonical ordering rules in §13.2. `semantic_digest` is the semantic cache key
and changes only when compiled meaning or a meaning-producing compatibility,
registry, dependency, evidence, capability, target, or product-profile input
changes.

The extension projection is the canonical array of ignorable extension records
including exact opaque bytes. The empty projection is JCS `[]`; its complete
preimage is `luxel.ir.extensions/1`, NUL, `[]`, and its required digest is
`sha256:7fd00110665a4cced7429003ee7af97a67a28800c9529c6eb7f426d295b16e0b`.

The document projection is the complete canonical IR document with only
`document_digest` omitted. `semantic_digest` and `extension_digest` are ordinary
included fields. No digest appears in its own preimage. `document_digest` is the
tamper-evident identity of the complete IR document.

Changing only source layout/provenance preserves `semantic_digest` but changes
`document_digest`. Changing only an ignorable extension preserves
`semantic_digest`, changes `extension_digest` and `document_digest`, and
therefore may reuse semantic compiler caches without losing full-document
integrity. Changing a known critical extension changes all applicable semantic
and document identities. Readers verify payload digests and the three document
digests before using or preserving extensions.

Changing a semantic evidence pin changes `semantic_digest` and
`document_digest`. Changing only construction/certification evidence preserves
`semantic_digest` but changes `document_digest`; every phase that reads that
evidence includes its exact pin and interpretation metadata digest in its cache-
input schema. Unused manifest entries do not invalidate phase caches.

## 14. Compiler phases

The reference compiler SHALL expose these observable phases:

1. build-request, module manifest, version, target, product-profile,
   requested-capability, seed, and evidence-manifest validation;
2. lexical and syntax parse;
3. safety/AST whitelist validation;
4. import and name binding;
5. type, unit, coordinate, and reference checking;
6. domain validation and target-conditioned default resolution;
7. constraint graph construction;
8. provider capability negotiation;
9. canonical IR emission;
10. construction-plan lowering;
11. numerical/geometry execution;
12. artifact assembly;
13. certification and provenance sealing.

The stable module manifest, selected target, selected product profile, and
requested capability set are immutable build inputs available to default
resolution. Registry defaults MAY be pure versioned functions of
`(constructor, target, product_profile_digest, requested_capabilities)` and are
materialized in IR. Provider negotiation verifies that one backend can satisfy
the already-resolved request; it MUST NOT reinterpret or rewrite defaults. An
unavailable defaulted strategy is an explicit capability failure, not authority
to select a vaguely similar fallback.

Failure in any phase stops dependent phases. Diagnostics identify the phase and
retain source locations through lowering.

Incremental compilation MAY cache any phase by content digest. Cache hits MUST
be semantically indistinguishable from uncached compilation.

Each phase publishes a versioned cache-input schema. A cache key
domain-separates at least phase ID/version, selected compiler component version
or semantic compatibility ID as appropriate, registry digest, target,
product-profile digest, capability-manifest digest, and the exact upstream
semantic, document, extension, source, dependency, evidence, or artifact
digests that the phase declares it reads. Semantic-only phases MAY depend on
`semantic_digest`; an extension-preserving, packaging, or artifact phase MUST
also depend on `document_digest` or the relevant extension digests. Every
profile-sensitive construction, assembly, and certification phase depends
explicitly on `product_profile_digest`. Reusing a cache entry with an
undeclared dependency is a conformance failure.

Producer provenance stored in a cache entry is immutable. A cache hit retains
that original producer record and appends a separate reuse event containing the
consumer build identity, cache key, and verification result to the current
build receipt/event log. The reuse event is not appended to or hashed as part
of the immutable cache entry; a sealed receipt that contains it receives its
own document identity. A cache hit MUST NOT rewrite the entry as if the current
build produced the cached value.

## 15. Diagnostics and repair

Diagnostics are structured records, not strings. Each record SHALL contain:

- stable diagnostic code;
- severity;
- phase;
- message;
- primary source span or IR path;
- expected and actual values/types where applicable;
- related spans/paths;
- zero or more machine-readable edits;
- explanatory notes;
- documentation key.

A repair edit is the closed record
`{source_digest, start_byte, end_byte, replacement_utf8,
precondition_digest}`. Byte offsets are half-open UTF-8 offsets into the exact
`source_digest`; `replacement_utf8` is valid UTF-8 text; the precondition digest
domain-separates the source digest, range, diagnostic code, and expected removed
bytes. Unknown fields are rejected. Applying it to the matching source MUST
yield valid syntax. Semantic validity is required when the compiler claims the
repair is complete.

Every proposed repair is classified as:

- **corrective** — mechanically restores the already-declared intent after a
  local syntactic, binding, unit, or schema error;
- **migratory/adaptive** — applies a versioned equivalence rule while preserving
  semantic intent for the pinned target;
- **intent-changing/degrading** — changes an aesthetic, physical, gameplay, or
  quality choice to make a gate easier to satisfy.

Only corrective and migratory/adaptive repairs with a registered proof rule and
successful strict postcheck are auto-applicable. Intent-changing/degrading
edits may be presented as clearly labelled alternatives but are never repairs,
never auto-applied, and require an explicit new authoring decision.

A batch of auto-applicable edits with disjoint ranges and one shared base digest
may be applied atomically; the precondition is evaluated once and offsets are
interpreted against the common base. The compiler MUST run the combined strict
postcheck because individually valid repairs can interact. Overlapping edits,
mixed repair classes, or a failed combined postcheck reject the batch. Applying
a valid batch either commits every edit or none.

Lenient authoring MAY apply only explicitly classified, semantically
unambiguous local repairs. Identifier correction is never automatic when more
than one candidate lies within the configured confidence window, nor when the
misspelled token already resolves to a compatible existing binding. Those
cases produce a diagnostic listing bounded alternatives. Confidence metric,
threshold, and tie behavior are registry-versioned and covered by conformance
fixtures.

The compiler API exposes disjoint result types. Strict compilation returns
`StrictIR` or diagnostics. Lenient compilation returns `LenientCandidate`,
containing candidate repaired source, its repair manifest, preview values, and
diagnostics; it contains no `StrictIR` value and cannot be decoded as one.
`LenientCandidate` may be consumed only by an explicitly non-certifiable preview
sandbox whose outputs are labelled `preview` and are forbidden from world or
artifact application, scaffold generation, cache publication, and
certification. No common untagged dictionary or record may represent both
types.

Promotion requires the author or authorized tool to commit or acknowledge
those edits into a new source digest followed by a strict recompilation.
Certification, scaffold generation, and normal world/artifact application
consume only `StrictIR` and MUST NOT consume unacknowledged lenient output.
Every promoted repair appears in provenance.

Diagnostics MUST distinguish author error, unsupported capability, solver
failure, backend defect, stale cache, and internal compiler error.

Acceptance diagnostics carry `attribution: local|ranked|global`. Local
attribution names one responsible declaration. Ranked attribution contains a
bounded contributor list with sensitivity/evidence and confidence. Global
attribution states why no honest local repair exists. A diagnostic MUST NOT
invent a single cause merely to satisfy the repair protocol. If the lowest-cost
metric repair would violate declared intent, the diagnostic reports a
gate/specification conflict and does not offer that degradation as an author
repair.

## 16. Security and resource limits

Model-authored source is untrusted input. The frontend MUST enforce limits on
source bytes, AST nodes, nesting depth, identifier length, literal collection
size, declarations, references, constraints, generated instances, and
diagnostics.

Compilation MUST NOT access filesystem, environment, network, clocks,
processes, dynamic libraries, or host reflection through source semantics.
External evidence enters only through a caller-provided, digest-pinned manifest
and an authorized resolver.

### 16.1 Evidence resolver

The resolver receives an explicit immutable root and manifest from an
authorized caller outside source semantics; source cannot select or widen that
root or supply the manifest. The build request content-pins the manifest before
compilation. Source refers only to logical IDs, while trusted manifest entries
provide expected digest, size, media type, and relative location. Logical
manifest identifiers are resolved without string-concatenating paths. Resolved
entries MUST remain beneath the root after normalization and symlink resolution.
Absolute paths, `..` traversal, symlink escape, device files, sockets, and
unsupported file types are rejected.

The resolver enforces per-entry and aggregate byte/count/dimension limits,
opens files with race resistance equivalent to no-follow descriptor-relative
resolution, re-verifies the opened object and containment through a stable
handle, and verifies size and digest before decoded content becomes available
to a compiler/backend. A mismatch, replacement race, missing entry, duplicate
logical ID, or limit violation is a distinct structured error; no failing entry
is skipped. A target that cannot provide those semantics MUST reject the
production evidence-resolver capability and cannot certify artifacts that
consume filesystem evidence; it may expose a separately named non-production
best-effort capability that Luxel 1.0 production profiles never select.
Remote resolution is outside 1.0 unless a separately authorized fetcher first
materializes a digest-pinned local manifest under these rules.

Numerical plans declare CPU/GPU memory, work-item, iteration, and wall-time
budgets. Backend termination produces a structured failure and cannot certify a
partial artifact as complete.

The parser, binder, type checker, canonicalizer, migrators, and all boundary
decoders MUST be fuzzed.

## 17. Backend contract

Every backend publishes a versioned capability manifest containing supported
IR schema versions, domain operations, limits, numeric formats, determinism
grade, devices, and artifact formats. Capability IDs name versioned semantic
contracts (for example `luxel.geometry/sdf_extract@1`), not informal feature
labels. Negotiation matches ID/version, contract and profile digests, target
limits, determinism grade, and required artifact properties; matching a name
alone is insufficient. A capability fallback is legal only when source or IR
selects a registered explicit strategy with its own contract and error bound.

Determinism grades are:

- **A — byte deterministic:** canonically encoded artifacts are byte-identical
  for equal declared inputs across paths and processes on supported targets;
- **B — numerically deterministic:** semantic outputs agree within declared
  component-wise tolerances, but serialized bytes may differ;
- **C — statistically reproducible:** only declared distributional properties
  are guaranteed; this grade is experimental and cannot certify a production
  Luxel 1.0 artifact.

The reference semantic compiler, Luxel world backend, and reference artifact
packager MUST achieve Grade A. A Taichi/GPU operation that achieves only Grade
B must record the loss and cannot satisfy a Grade-A target without a
deterministic fallback. Artifact canonicalization may remove representation-only
variance but MUST NOT disguise numeric or semantic Grade-B differences.

### 17.1 Grade-A artifact canonicalization

Every Grade-A artifact format has a versioned, registry-owned canonicalization
profile and profile digest. The certified artifact bytes are the output of that
profile—not arbitrary raw tool output filtered only during comparison. The
artifact manifest records raw-tool-output digest when raw output exists,
canonical artifact digest, profile ID/digest, tool versions, and every applied
canonicalization step.

A profile may only normalize representation that carries no declared semantic
content, such as:

- embedded timestamps and host-generated archive metadata;
- absolute build-root fragments replaced by content-addressed/relative forms;
- map/object key and archive-member ordering;
- file permissions outside the declared artifact contract;
- equivalent numeric/container encodings under a format-specific canonical
  rule;
- compression settings when decompressed normative content is identical.

A profile MUST NOT remove, mask, or tolerate differences in geometry,
topology, transforms, materials, textures, policies, identities, provenance,
constraints, gameplay data, evidence, or any field declared semantic by the
artifact schema. Its exclusion/rewriting list is closed, machine-readable, and
part of the registry digest. Anything not listed is compared bit-for-bit.

Grade-A conformance verifies both that canonical artifacts are byte-identical
and that the canonicalizer touched only operations permitted by its pinned
profile. Mutation tests prove that changing any semantic field changes the
canonical artifact digest.

Compilation for a target fails before expensive execution if required
capabilities are absent. Backends MUST NOT silently approximate operations.
Approximation is permitted only through an explicit source/IR strategy with a
declared error bound and provenance entry.

Candidate integration hypotheses to evaluate are:

| Component | Candidate responsibility |
|---|---|
| Odin | candidate semantic driver; production orchestration, native Luxel integration, packaging, cache and plugin ABI |
| Rust | candidate semantic compiler; typed IR, canonicalization, fuzz-heavy parsers/decoders and native validation |
| Taichi | CPU/CUDA/Vulkan field execution, SDF/voxel operations, projections, differentiable objectives |
| Julia | research solvers, inverse problems, parameter fitting, algorithm validation |
| Python | migration frontend and thin Blender `bpy` adapter |
| Blender | geometry/material realization, import/export and artifact inspection |

This table is a set of bakeoff hypotheses, not architecture. Components may be
combined, removed, or replaced when vertical slices expose their total runtime,
build, interoperability, fuzzing, and maintenance cost. The semantic frontend
host is selected only after the 1.0 requirements and host bakeoff are complete.

## 18. Versioning and migration

Source language, IR schema, domain registries, compiler, and backend capability
versions are separate identifiers.

- Patch releases fix defects without changing valid-program meaning.
- Minor releases may add optional syntax, types, or capabilities while
  preserving existing canonical meaning.
- Major releases may change meaning and require explicit migration.

Exact compiler version/build identity is producer provenance. Semantic
compatibility is separately named by the
`semantic_compiler_compatibility_id` in §13.2. A patch release may retain that
ID only when the pinned valid-program conformance suite proves unchanged
semantic IR and diagnostics where diagnostics are normative; otherwise it
publishes a new compatibility ID even if its marketing version is a patch.
Caches and manifests MUST NOT infer semantic compatibility from version-string
ordering.

Programs declare or are compiled under an explicit language version. There is
no ambient "latest" in reproducible builds.

Luxel pins released language, IR, registry, compiler, and backend capability
versions. It submits requirements upstream rather than forking language
semantics. An upgrade is an explicit Luxel migration with conformance and artifact
diff evidence; an upstream release never silently changes a pinned Luxel build.

Migrators operate on typed IR when possible and source only when necessary.
They emit a report of every semantic change. Round-trip tests prove that an
unchanged scaffold compiles to equivalent IR.

The existing `codeweald.world-intent/v1` frontend remains operational while a
compatibility compiler maps it into Luxel IR. Its proven repair and scaffold
behavior becomes conformance input rather than discarded code.

## 19. Conformance and evaluation

### 19.1 Language conformance

The suite SHALL cover:

- every accepted and rejected syntax form, including `None` versus unresolved
  `null`, keyword-only operands, the reserved identity/key positional slot, and
  forward-reference rejection, tuple-literal rejection, duplicate decoded
  record-key rejection, explicit mask constructors, and chained-comparison/
  Python-boolean-operator rejection;
- binding, import/local-name collisions, manifest-authorized compiler-module
  imports, relocation-stable `module_id`, cycles, references, reserved
  `module_seed` syntax, and rejection of all shadowing/rebinding;
- type, unit, coordinate, and capability errors;
- canonical serialization, array classification/total ordering, scaffold string
  escaping, and stable hashes;
- registry, semantic-compiler compatibility, exact compiler provenance, product
  profile, and typed dependency participation in canonical hashes/cache keys;
- JCS numeric equivalence for permitted source spellings, exact-integer range
  rejection without silent float escape, `Int / Int -> Float`, division-by-zero
  rejection, per-operation rounding, and negative-zero equality/JCS behavior;
- Grade-A byte determinism across working directories and processes;
- random-stream isolation under unrelated declarations and distinct stable
  streams for structurally equal stochastic declarations, including adding a
  second keyed stochastic sibling without rerolling the first, duplicate-key
  rejection within an owner, absence of operation/sibling paths in stream
  identity, explicit-seed absent encoding, module-seed precedence/conflict/
  derivation, and module isolation for equal key spellings;
- diagnostic codes, spans, and executable repairs;
- atomic disjoint repair batches and refusal of ambiguous identifier repair;
- strict/lenient separation;
- refusal to autoapply intent-changing repairs, interacting-repair batch
  postchecks, strict promotion of lenient candidates, and provenance retention;
- migration and semantic-digest scaffold round trips with generated-node
  lineage and many-to-many source maps;
- parser and compiler denial-of-service cases;
- backend refusal of unsupported operations;
- rejection of unknown IR fields in every mode and preservation/rejection of
  declared extensions according to criticality;
- semantic/extension/document digest behavior for documents differing only in
  an ignorable extension, with a pinned `document_digest` vector proving that
  only its own field is omitted while semantic/extension digests are included;
- semantic versus construction-only evidence-pin mutations, manifest digest
  behavior, and phase cache invalidation for every evidence consumer;
- rejection or critical reclassification of any purportedly ignorable extension
  that changes selected-target artifacts, plus phase-specific cache dependency
  and immutable producer-provenance tests; 1.0 extension fixtures also reject
  implicit inter-extension dependencies;
- closed validation-class labels and rejection of backend-local feasibility
  labels;
- Grade-A canonicalization profile enforcement, including rejection of a
  canonicalizer that excludes semantic content;
- independent realized-hard-constraint validation after a falsely successful
  solver report, including tolerance-boundary fixtures;
- explicit asset-local/world/engine transform requirements and versioned
  capability-contract negotiation rather than name-only matching;
- target-conditioned default stability and proof that provider negotiation
  cannot rewrite resolved defaults;
- complete nominal-angle operator/conversion fixtures;
- standard-unit scale and one-rounding conversion fixtures;
- production evidence-resolver symlink replacement races and capability
  rejection on targets without equivalent race resistance;
- cross-backend fixture agreement.

The closure fixtures introduced by draft 0.6 have these stable IDs and MUST be
present before the implementation contract freezes:

| Fixture ID | Required assertion |
|---|---|
| `fixture.canonical.all_array_orderings` | all setlike permutations canonicalize identically; semantic sequence reorder changes semantic identity; DAG ready-set tie is deterministic |
| `fixture.digest.complete_domain_matrix` | one-field mutations match every equality/inequality cell in §13.3 |
| `fixture.identity.module_move_and_registry_binding` | source relocation preserves identity; manifest rename changes it; undeclared export, seed conflict, and dependency cycle reject |
| `fixture.profile.identity_and_cache_isolation` | distinct profile digests isolate semantic/build caches; aliases with one pinned digest agree |
| `fixture.syntax.mask_comparison_and_keywords` | legal mask constructors and keyword examples pass; positional operands, boolean operators, and chained comparisons reject |
| `fixture.numeric.unit_scale_rounding` | registry unit factors and conversions match pinned binary64/JCS vectors |
| `fixture.compiler.patch_compatibility` | equal compatibility ID preserves semantic digest while exact producer provenance/document identity differs |
| `fixture.collections.and_identifier_roundtrip` | tuple and duplicate keys reject; hostile external IDs round-trip only as escaped values |
| `fixture.migration.lenient_preview_requires_strict_promotion` | `LenientCandidate` previews but cannot apply/certify/scaffold/cache; acknowledged strict recompilation can |
| `fixture.extensions.empty_set_digest` | empty extension projection equals the constant in §13.3 |
| `fixture.cache.reuse_event_location` | cache entry provenance remains byte-identical; current receipt records reuse |
| `fixture.policy.soft_promotion_label` | promotion emits the specified `realized_hard` ID and provenance |

Every registry parameter and cross-parameter predicate SHALL have boundary
fixtures against a certified baseline target. A claimed static bound that does
not build is a registry/specification failure. Evidence-dependent feasibility
uses separate realized-output fixtures and does not masquerade as a static
guarantee.

### 19.2 Geometry/asset conformance

Fixtures SHALL test product-profile required/optional artifact inventories,
typed absence reasons, watertightness, manifold topology, connected components,
physical dimensions, coordinate conventions, non-mirrored directional
textures, part connectivity, sockets, colliders, LOD validity, deterministic
seeds, evidence provenance, and export/re-import equivalence. World fixtures
also test mask exclusion, stable placement identity, typed policy/legacy-
validator inventory equality, solver-product schemas, global/ranked gate
attribution, and evidence-resolver traversal, symlink, digest, and size attacks.

### 19.3 Model-usability evaluation

At least one small local model and two frontier models SHALL be evaluated on:

- editing a scaffold rather than recalling an API;
- constructing and repairing representative worlds/assets;
- responding to structured diagnostics;
- changing one semantic property without collateral changes;
- preserving IDs, units, constraints, and provenance.

Metrics include first-pass parse rate, first-pass semantic validity, repair
success, task correctness, collateral edits, tokens, compilation attempts, and
human correction time. A vocabulary addition fails review if it materially
raises the small-model skill floor without compensating measured value.

Every published result pins a versioned task corpus and split, grader and
acceptance-gate digests, prompt/scaffold/instruction digests, model and inference
engine identifiers, quantization and resource envelope, decoding parameters,
sample count, trial seeds, and raw output/diagnostic logs. Reports include
repeated-trial uncertainty or confidence intervals where applicable. Model
generation itself need not be byte deterministic; the evaluation protocol and
all observations MUST be reproducible and independently auditable. “Small local
model” is defined by the benchmark's versioned deployment envelope—including
hardware, memory, latency, and inference-cost ceilings—not by an evergreen
model name or an arbitrary parameter-count slogan.

## 20. Red-team requirements before 1.0 freeze

Reviewers SHALL attempt to break:

- AST safety and host escape prevention;
- type, unit, coordinate, and reference soundness;
- declaration order and cyclic dependencies;
- deterministic hashing and seeded randomness isolation;
- numeric overflow, NaN, infinities, and precision boundaries;
- constraint inconsistency and solver nontermination;
- resource budgets and expansion bombs;
- malicious evidence manifests and path traversal;
- source/IR/backend version confusion;
- migrations and stale cache poisoning;
- diagnostic edits that alter the wrong source;
- accepted programs that generate invalid or useless artifacts;
- backend disagreement or silent approximation;
- ambiguity that small models systematically misunderstand;
- incentives where acceptance metrics reward worse art or gameplay.

Each finding must be classified as specification defect, implementation defect,
missing conformance test, backend limitation, usability regression, or accepted
risk. Resolutions update this document and add a regression fixture.

## 21. Host-language bakeoff

The semantic compiler host SHALL be selected using implemented vertical slices,
not language preference. Candidates include Odin, Rust, Python, Julia, and a
split frontend/backend architecture involving Taichi.

The bakeoff measures:

- parser and typed-IR ergonomics;
- quality of source spans and repair diagnostics;
- canonical serialization and hashing;
- fuzzing and property-testing maturity;
- incremental compilation and cache control;
- Luxel/Odin and Blender/Python integration;
- Taichi and Julia process/C-ABI integration;
- portability, build reproducibility, binary distribution, and startup time;
- contributor burden and dependency stability;
- performance on semantic compilation (not geometry kernels);
- ease of maintaining one authoritative specification.

### 21.1 Selected-component readiness

A component selected by the bakeoff is ready for a pinned Luxel release only
when it has:

- one versioned responsibility and boundary contract;
- a versioned capability manifest and declared determinism grade where it
  produces semantic or artifact output;
- a reproducible build and pinned dependency/toolchain inputs;
- canonical IR or typed-plan boundary fixtures, including rejection cases;
- fuzz coverage for every parser, decoder, migrator, and untrusted boundary it
  owns;
- resource-limit, cancellation, and structured-failure behavior;
- provenance/version reporting and cache-key inputs for its outputs;
- a maintainer/upgrade path that does not fork language semantics.

Readiness is evaluated per selected component and per boundary between selected
components. A successful standalone demo or reproducible build alone is not a
readiness claim.

Taichi is separately evaluated as the spatial execution host. It MUST NOT win
or lose the semantic-host decision merely because its kernels are
Python-shaped. Stable Taichi currently requires a pinned supported Python
environment, isolated from both system and Blender Python versions.

## 22. Explicitly deferred beyond 1.0

The following require separate proposals with measured use cases:

- general source-language loops, functions, conditionals, or metaprogramming;
- user-defined classes or arbitrary domain types;
- runtime gameplay scripting;
- inheritance and implicit policy merging;
- network package resolution;
- arbitrary plugins loaded by model-authored source;
- inter-extension dependency graphs;
- implicit learned geometry generation;
- source-level differentiable programming;
- shader-language embedding;
- distributed compilation;
- live mutable scene semantics.

Backends and experiments may implement these behind nonstandard capability
names, but their artifacts cannot claim Luxel language 1.0 conformance.

## 23. Migration from the current prototype

The first migration milestone SHALL:

1. inventory every current `worldbuilder_dsl.py` accepted/rejected fixture in a
   machine-readable migration manifest;
2. encode `WorldBuilderError` message, line, and suggestion as structured
   diagnostics;
3. map `codeweald.world-intent/v1` into the canonical Luxel IR envelope;
4. eliminate the `spines`/`spine_count` dual naming at the IR boundary;
5. represent documented versus buildable bounds explicitly;
6. generate a source scaffold that compiles and applies without semantic
   drift;
7. expose current acceptance, traversal, and border policies through typed
   registry metadata before changing their authoring syntax;
8. keep Claude's existing Luxel build operational throughout migration.

Every prototype fixture receives exactly one disposition:

- `preserve` — behavior is part of the replacement contract and the original or
  equivalent test must pass;
- `replace_with_strict_promotion` — legacy lenient behavior is retained only as
  non-certifiable preview and replaced by the §15 strict-promotion fixture;
- `retire` — behavior is intentionally absent, with rationale and a replacement
  conformance assertion when the old behavior was safety- or meaning-relevant.

`LenientAuthoringTests.test_lenient_output_still_applies_to_a_zone` is
`replace_with_strict_promotion`; it is not an unchanged acceptance test for the
new compiler. The manifest MUST account for every collected test and fails
closed when a prototype test has no disposition. The current prototype is
evidence, not baggage. No replacement is accepted until every `preserve`
fixture and every named replacement conformance fixture passes while the
existing Luxel build remains operational.

## 24. Open decisions for collaborative review

These must be resolved before the 1.0 freeze:

1. Semantic frontend implementation host.
2. Maximum collection/declaration/constraint budgets.
3. Final base-dimension vector beyond length, time, and mass.
4. Stream key-derivation and pseudorandom algorithm selected for the core
   registry (`Seed` representation is resolved by §7).
5. Coordinate-space registry and canonical handedness/up axis.
6. Exact minimum geometry constructor set and names.
7. Supported target/device matrix for mandatory Grade-A versus optional
    Grade-B execution.
8. Soft-objective composition and normalization semantics.
9. Solver conflict reporting requirements by constraint class.
10. Canonical representation of policies and domain registries.
11. Compatibility duration for `worldbuilder` source.
12. Product-level acceptance thresholds for the four initial asset families.
13. Small-model deployment envelope, corpus size and split, trial count, and
    thresholds for first-pass parsing, semantic validity, repair success,
    collateral edits, and maximum compilation attempts.

The source extension/import namespace, diagnostic edit format, and the absence
of a binary 1.0 IR are resolved by §§5.1, 5.5, 15, and 13.1 respectively. The
remaining items are registry, implementation-host, target, or release-profile
decisions. They do not reopen the frozen 0.6 source-kernel and canonical-IR
contracts, but no implementation may choose them silently or claim Luxel 1.0
release conformance until they are pinned.

## 25. Definition of language 1.0 readiness

The draft 0.6 **core implementation contract** may freeze before the complete
1.0 product release when every closure fixture in §19.1 exists, its
machine-readable contract validates, and a verification-only review maps I-01
through I-12 to exact normative text and passing evidence with no unresolved
Critical or High finding. That freeze pins the parsed source kernel, type and
identity rules, canonical IR envelope/order/digest domains, strict/lenient
boundary, and backend authority boundary. It does not claim that the remaining
§24 registries, targets, constructors, thresholds, host bakeoff, or model
benchmark are complete, and implementations expose unfinished surfaces only
under an unstable namespace.

Luxel language 1.0 is ready only when:

- all open decisions are resolved or explicitly deferred;
- grammar, type rules, evaluation, IR, diagnostics, security, and versioning are
  normative and testable;
- the reference compiler passes conformance and fuzz testing;
- current Luxel landform scaffolds migrate without drift;
- one Blender asset pipeline and one Luxel world pipeline consume canonical IR;
- sword, rock, bottle, and tree fixtures meet declared certification gates;
- certification independently validates every realized hard constraint rather
  than trusting producer/solver success;
- model capability is treated as an efficiency/stress-test axis, while product
  success is minimizing the model capability required for professional
  game-development work without sacrificing output quality;
- every component selected by the §21 bakeoff, and every boundary between those
  components, satisfies §21.1 selected-component readiness;
- two independent frontier-model red teams have been resolved into tests;
- no accepted Critical red-team finding remains unresolved, and no accepted
  High finding affecting digest domains, stochastic identity, evidence
  authority, or parser literal/operator typing remains unresolved;
- no backend silently drops, clamps, mirrors, substitutes, or rerolls intent.
