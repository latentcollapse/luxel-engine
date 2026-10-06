# Luxel Engine docs

Start with [roadmap.md](roadmap.md). The engine's name and the internal `luxel` prefix: [adr/0001-luxel-engine-name.md](adr/0001-luxel-engine-name.md). Docs are filed by roadmap layer; closed
handoffs and reports live in `archive/`, bundled by month and topic, each with
a `SUMMARY.md`.

| Folder | What it holds | Code it describes |
|---|---|---|
| [platform/](platform/) | architecture, contracts every layer uses, ledgers, open decisions | `world_core/crates/{native_graphics_contract, asset_contract, certification_authority, project_ledger, worldspec, live_evidence_contract, intake_repair_contract}`, `graphics_lab/` (Julia/Lava renderer), `world_core/apps/world_viewer` |
| [world/](world/) | terrain, atmosphere, materials, lighting, vegetation, architecture; the CONVERGE contracts | `native_graphics_contract` (scene lowering, `backdrop.rs`, `kit.rs`, `scatter.rs`, `terrain_layers.rs`, `still_water.rs`), `terrain_lab/`, `pipeline/` terrain, zone and forestry modules, `tools/` kit, backdrop and measurement scripts |
| characters/ | (none yet) | `pipeline/rigging_provider.py` |
| [deformation/](deformation/) | TetCage seam | `graphics_lab/tet-lab/`, `native_graphics_contract/src/deformation.rs` |
| [gameplay/](gameplay/) | gameplay kit, capability resolution, live loop and runtime | `world_core/crates/{gameplay_contract, reference_runtime}`, `pipeline/` navigation, collision and acceptance modules |
| [content-sdk/](content-sdk/) | semantic facade, construction plans, semantic kernel | `world_core/crates/{semantic_kernel, luxel_control_plane}`, `pipeline/luxel_*.py`, `pipeline/worldbuilder_dsl.py`; language spec in `DSL docs/` (hash-frozen, stays put) |
| [add-ons/](add-ons/) | Grindstone, Reforge | (none yet) |
| [integrations/](integrations/) | tools Luxel drives from outside | Gaea: `pipeline/gaea_*.py`, `Code Projects/Gaea/`; Blender: `pipeline/blender_*.py`, `tools/blender_*.py`; ComfyUI: `pipeline/comfy_generate_texture.py`; engines: `engine_adapters/`, `pipeline/zone_to_{unity,unreal}.py`, `pipeline/*.gd` |
| [adr/](adr/) | decision records | |
| [archive/](archive/) | superseded and closed docs | |

Code still lives where it was. It moves into per-layer homes when that layer
becomes the active campaign, so a move never lands in the middle of
someone else's sprint; this table is the map until then.
