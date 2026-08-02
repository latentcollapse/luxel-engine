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
