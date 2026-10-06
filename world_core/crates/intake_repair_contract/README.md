# Luxel intake and repair contract

This crate defines provider-neutral, closed JSON contracts for source intake
and evidence-bound repair. Rust computes identities, checks source bytes and
provenance, validates the typed provider interpretation, and derives repair
assessments. A provider may supply interpretation data; it does not supply
semantic authority or certification status. Python integrations should only
serialize/transport these payloads.

## Intake flow

1. Start with a `SourceBundleDraft` containing only brief, concept-art, or
   design-document sources and a caller-generated fresh `request_id`.
2. `prepare_source_bundle` verifies each raw source digest and emits Rust-owned
   source IDs and a request-bound bundle ID.
3. Send that bundle to any provider. Its raw response must be a typed
   `ProviderInterpretation` with observations, inferences, assumptions,
   conflicts, confidence, source IDs, and evidence regions. Bind the exact raw
   response digest and provider/version/protocol in `IntakeDraft`.
4. `normalize_intake` checks the provider response against the actual source
   bytes and emits deterministic `SemanticIntake`. Observations require
   source regions; all claim evidence must resolve to the source bundle.
5. `validate_intake` repeats those checks before downstream use. Old generated
   semantic reports are not accepted as intake drafts or source bundles.

Freshness is represented by a caller-generated request token bound into the
bundle identity; integrations must generate a new token for each new intake
request and must retain the original source and provider-response bytes.

## Repair flow

`normalize_repair_proposal` closes and identities an authorized proposal, but
does not establish that its failure claim is true. Integration code registers
native Rust validators using `NativeValidatorRegistry::register` with the
validator ID, receipt schema, and trusted validator function. Then
`validate_proposal_evidence` checks the proposal against raw candidate and
receipt bytes. `validate_repair_delta` independently revalidates before/after
candidate, artifact, and native receipt bytes; enforces the authorized edit
scope and bound; and derives the outcome, metric deltas, assessment, and
explanation. The caller cannot submit a pass/improvement field. Identity-only
checks (`validate_repair_proposal` and `validate_delta_identity`) are not
evidence validation and must never be used to promote a candidate.

The integrating Rust authority owns its validator registry and promotion
decision. An unregistered validator, unknown schema, stale digest, status-only
receipt, unauthorized edit, unchanged bytes, or non-improving measurement
cannot be upgraded to a pass by this contract.

## CLI

Build from the `world_core` workspace with:

```text
cargo build --offline -p luxel-intake-repair-contract --bin luxel-intake-repair
```

Commands:

```text
luxel-intake-repair prepare-source-bundle DRAFT.json OUTPUT.json source_ref=PATH...
luxel-intake-repair validate-source-bundle BUNDLE.json source_id=PATH...
luxel-intake-repair normalize-intake DRAFT.json SOURCE_BUNDLE.json PROVIDER_RESPONSE OUTPUT.json source_id=PATH...
luxel-intake-repair validate-intake INTAKE.json PROVIDER_RESPONSE source_id=PATH...
luxel-intake-repair normalize-repair-proposal DRAFT.json OUTPUT.json
luxel-intake-repair validate-repair-proposal PROPOSAL.json
luxel-intake-repair validate-delta-identity DELTA.json
```

The CLI offers transport-friendly intake and structural identity operations.
Full repair evidence validation is available through the Rust API because it
requires the integrating authority's trusted native-validator registry.
