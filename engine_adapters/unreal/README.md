# Codeweald Unreal adapter

`CodewealdZoneImporter` is a compiled **Editor** project plugin, not a loose
editor script. Copy the whole directory to `<UE project>/Plugins/`, regenerate
project files if the engine asks, and enable the plugin. Its native menu item
is **Tools > Codeweald > Preflight Zone Manifest**. That transaction validates
the manifest and every required Landscape source file, then writes a report to
`<UE project>/Saved/Codeweald/`; a failed preflight creates no Unreal assets.

For verified source-prop import, also enable Unreal's **Python Editor Script
Plugin** and a glTF importer.

Run this in Unreal's Python console after copying or mounting the complete
Codeweald project so verified source asset paths resolve:

```python
import codeweald_zone_import as codeweald
report = codeweald.import_zone(r".../concept_batches/caledonia_v1/unreal_zone_import.json")
```

The build generates `terrain/unreal/landscape_height_16.png` at 1009 square
samples (a 16 by 16 grid of 63-quad Landscape components) and separate grass,
road, rock, and snow grayscale layer masks. The plugin refuses missing or
digest-mismatched source props, then imports only the verified GLB/FBX files.

It deliberately does **not** pretend to create a finished Landscape actor yet:
the native module establishes the editor-side acceptance/report boundary, while
the final LandscapeEditor import transaction must be implemented and exercised
in a real UE5 installation. A green preflight is only permission to attempt
that next transaction; it is not a claim that a Landscape actor exists.
