# GCS foundation: capability contracts and kit resolution

Status: bounded semantic foundation. This document covers the registry and resolver in
`luxel-gameplay-contract::gcs`; it does not define gameplay runtime behavior.

## Contract surface

The crate defines typed IDs for capabilities, provided features, validation suites, kits, and
profiles. A `CapabilitySpec` carries a version; required capability IDs; provided feature IDs;
conflicts; optional integration edges; state authority; a categorical runtime-cost estimate;
network and persistence requirements; and validation-suite IDs. The runtime-cost class is metadata,
not a performance measurement or budget guarantee.

`KitManifest` selects profile IDs and additional capability IDs. A `KitProfileSpec` can include
other profiles and declares its required capabilities. `CapabilityRegistry::new` rejects malformed
IDs, invalid metadata, missing validation-suite identifiers, and duplicate registry IDs. It leaves
dependency-reference checks to resolution so errors can name the selected graph path.

## Resolution rules

`resolve_kit` performs these deterministic steps:

1. Reject unsupported schemas, empty manifests, invalid IDs, and duplicate manifest selections.
2. Expand included profiles in sorted ID order, reporting unknown profiles and profile cycles.
3. Expand required capabilities transitively, reporting unknown root capabilities, missing
   dependencies, and dependency cycles with their traversed paths.
4. Check selected capability pairs for conflicts. A conflict declared by either side rejects the
   pair; the diagnostic is emitted once for the sorted pair.
5. Record optional integrations only when both endpoints are selected. Missing optional endpoints
   produce warnings and never trigger implicit selection or invalidate an otherwise valid kit.

Errors leave `ResolutionReport.resolved` empty. Warnings are preserved alongside a resolved kit.
Diagnostics are sorted and deduplicated before return, so their order does not depend on manifest
vector order or hash-table iteration.

IDs are lowercase ASCII, begin with a lowercase letter, and may then contain lowercase letters,
digits, `.`, `_`, or `-`, up to 128 bytes. Duplicate IDs in manifests are errors rather than being
silently normalized away.

## Reference profile compositions

The checked-in foundation registry is intentionally small and contains five composable profiles:

- `action_rpg` supplies the shared action-RPG base; `soulslike` includes it and adds stamina,
  dodge, checkpoints, and encounter reset.
- `shooter` supplies movement, ranged combat, weapons, perception, and objectives;
  `looter_shooter` includes it and adds inventory, progression, equipment, and loot.
- `survival` composes movement, attributes, inventory, persistence, gathering, crafting, and needs.

The integration test resolves `soulslike + looter_shooter + survival` as one kit. Shared
capabilities such as movement, attributes, inventory, and persistence appear once after resolution.
This proves semantic composition only; it does not claim those systems execute in a game runtime.

## Canonical identity

`ResolvedKit` stores profile and capability specifications in `BTreeMap`s and active optional links
in a `BTreeSet`. `canonical_bytes()` uses the crate's existing compact `serde_json::to_vec`
convention. `sha256()` hashes those exact bytes and returns the canonical
`sha256:<64 lowercase hex>` identity form used by the other Luxel contracts. Equivalent manifests
with different input ordering therefore produce identical resolved bytes and digests. The digest
covers selected profile/capability specifications and active optional links, not diagnostics or
unselected optional integrations.

## Deliberate limits and open design choices

This first foundation does not implement abilities, inventory, combat, persistence, networking,
or validation suites. Suite IDs are contract references; this crate does not yet own a suite
registry or execute tests. Capability version constraints, tuning ranges, profile doctrine,
custom capability provider provenance, and runtime budget measurement remain follow-on design
work. The current authority and network fields are descriptive metadata, not a replication or
security policy.

Persistence scope is single-valued per capability in this slice. If real capabilities need
independent character and world state, that model should be expanded from concrete use cases rather
than guessed here.
