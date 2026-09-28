# Codeweald Unity adapter

Add `com.codeweald.zone-importer` as a local package in a Unity 2022.3 project,
then run **Tools > Codeweald > Import Zone Manifest** and choose the generated
`unity_zone_import.json`.

The editor transaction creates `TerrainData` from the normalized 16-bit
heightmap, applies the four semantic terrain masks, copies only SHA-256
verified source assets, and deterministically places the resulting Unity
prefabs inside each polygon/point feature. It also binds portable water flow,
objective pulse, and foliage-wind effects to the generated feature roots.

Generated Blender kits publish GLB sources for Godot plus FBX sidecars under
`assets/generated/**/unity/`; the manifest selects those sidecars for Unity.
The bundled nature kit similarly uses its supplied `FBX (Unity)` files. A
missing or unimportable source remains in the import report rather than being
replaced with a primitive. This adapter has source-level test coverage through
the zone build, but must still be exercised in an installed Unity Editor before
it can be called runtime-validated.

## WGE MVP handoff

The project-ledger vertical slice emits `wge.unity-mvp-import/v1` beside a
certified `project_snapshot.json`. In Unity use **Tools > Codeweald > WGE MVP >
Import Certified Snapshot**. That existing menu path now reads the strict
typed contract in `WgeMvpContract.cs`: it checks the canonical snapshot digest,
snapshot/manifest identity and target agreement, gate coverage, receipt IDs,
artifact bindings, SHA-256 bytes, and root-contained paths (including symlink
rejection) before copying artifacts. It preserves the imported artifact set
and writes `wge_mvp_import_report.json` with target runtime validation marked
`indeterminate`.

The package uses Unity's Newtonsoft JSON package to reject duplicate or
unknown contract fields. An offline C# harness covers valid handoffs, semantic
trace ID versus ledger artifact ID, missing and altered sources, path escape,
malformed/stale snapshots, and malformed/tampered/status-only runtime
receipts. Run it with:

```sh
dotnet run --project engine_adapters/unity/tests/ContractHarness/ContractHarness.csproj --configuration Release
```

`VerifyRuntimeReceipt` checks the receipt's schema, self-digest, nonce,
snapshot/build/artifact hashes, Unity profile, exact input trace, route order,
ability events, and objective result. This validates receipt structure and
bindings only; the self-digest is not an execution signature. Until a native
validator or independently observed Unity process authenticates runtime
execution, this adapter does not promote a Unity runtime gate. The legacy
`CodewealdMvpRuntimeReceipt` MonoBehaviour remains for package compatibility;
its inspector fields are explicitly not valid WGE evidence.
