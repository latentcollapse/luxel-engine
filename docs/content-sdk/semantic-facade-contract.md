# Luxel Semantic Facade Contract

Status: **read-only discovery slice implemented; execution remains staged**, 2026-09-30  
Scope: give an autonomous model a small, semantic vocabulary for Luxel without pretending that planned machinery is already callable.

## Why this exists

The low-level transaction surface is necessary authority machinery, but it is
not a useful model-native mental model by itself. A fresh agent should be able
to ask Luxel:

```text
What can you do?
What is callable now?
What is partial or planned?
What inputs and outputs does the operation bind?
What legal operation comes next if it succeeds or fails?
```

The Rust-owned `SemanticFacadeCatalog` answers those questions. It is a
discovery contract, not a second semantic authority plane and not an execution
plugin registry.

## Status semantics

```text
available = a bounded transport operation exists and is validated
partial    = a bounded slice exists, but the complete semantic operation is not closed
planned    = the vocabulary is intentional, but no callable transport is exposed
deferred   = deliberately outside the current campaign or blocked by an explicit decision
```

An operation marked `planned` or `deferred` must not carry a transport mapping.
The catalog validator rejects that ambiguity. An `available` operation must
have a transport mapping, and every capability ID it names must exist in the
current Rust capability registry.

## Current discovery commands

```bash
cargo run --manifest-path world_core/Cargo.toml --offline -p luxel-control-plane -- \
  facade
cargo run --manifest-path world_core/Cargo.toml --offline -p luxel-control-plane -- \
  facade --status partial
cargo run --manifest-path world_core/Cargo.toml --offline -p luxel-control-plane -- \
  facade-explain project.plan/v1
```

The bounded agent/MCP surface exposes the same catalog as `facade_list` and
`facade_explain`. These operations do not inspect source files, reveal backend
handles, or execute a planned verb.

## Initial vocabulary

The catalog covers the intended path:

```text
project.intake → project.plan → capability/style resolution
    → world.construct → scene.compose → asset/character preparation
    → gameplay.compose → runtime.launch
    → quality.inspect → repair.propose/apply → project.verify
    → project.package
```

Only the currently implemented discovery, style compilation, construction
planning, and existing transaction/verification slices are mapped as callable.
This is deliberate. A model seeing `world.construct` in the catalog learns
that it is the intended next capability and also that it is not yet a command
it may invoke successfully.

## Ownership

Rust owns the catalog identity, status, schemas, capability references, and
content digest. Python/MCP transports requests and returns the catalog. Julia,
Lava, Blender, and other providers do not register themselves or promote their
own outputs. A future execution slice must add a typed Rust contract and an
independent validator before changing a verb from planned/partial to available.

## Recognition-first rule

Facade entries must expose enough context for the next decision without
requiring repository archaeology:

- semantic purpose rather than backend jargon;
- typed input/output schema identities;
- explicit current status;
- current transport mapping, if any;
- registered capability dependencies;
- legal next steps;
- named failure modes;
- source contract for deeper reading.

The catalog is therefore part of Luxel's ergonomics and part of its correctness
boundary. A misleading “available” entry is a contract defect, not merely bad
documentation.
