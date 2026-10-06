# Landscape Parity Plan

2026-10-05 · Matt · Status: **plan, nothing built.** Everything below is labelled *verified* (measured or read in the repo) or *plan*.

Feeds CONVERGE-4. Source items: `docs/world/converge/converge3-contracts.md` "Carried into CONVERGE-4" (N-3 km-scale backdrop, N-1 ridge contrast, N-8 contact AO, L-1 cascaded shadows, biome/species, material layers) and `docs/world/graphical-parity-audit.md` GP-10.

## Goal

Close frames that read as Witcher 3-class landscape: dense detailed ground, grounded lighting, atmospheric depth, and Alps-like mountains on the horizon. Current state: RIFT-2011-class (review of `artifacts/parity/review-converge3-w1b/close-candidate.png`).

## Decision: Gaea is the landform engine

- **Verified.** There is no official Gaea MCP or agent plugin. Community `gaea-mcp` servers exist; they are unofficial and treat the `.terrain` format as unstable. Not adopted.
- **Verified.** `pipeline/gaea_terrain.py` (439 lines) reads and edits `.terrain` JSON and inserts nodes. Gaea builds the result headlessly under Proton (`docs/integrations/gaea-programme.md` §3.3). Gaea is a dependency WGE drives.
- **Verified.** Our own generator (`terrain_lab`, ~1,160 lines of Julia) is an erosion kernel (hydraulic, thermal, flux field, valley cross-section), not a graph engine. Procedural mountain generation failed twice and was retired 2026-08-04 (commit `9fa9faa`). Not rebuilt.
- **Decision.** Gaea produces landform. `terrain_lab` stays as fallback and for play-space fields Gaea cannot know about (lanes). Blender handles asset and mesh work through the existing `blenderMCP`.

## The rule (from the Gaea programme)

Terrain is generated **without reference to the play space**, then the play space is placed into it. A range built as a function of the play space reads as a wall, and a nearer peak can never be taller than a farther one. The arena (256 m, play area 58.9% of it) has no room for mountains, so mountains are **far-field backdrop**, not playable terrain.

## Work packages

### P1. Gaea driver as a local MCP tool — **DONE 2026-10-05**
- `pipeline/gaea_build.py`: the headless build driver (GAEA_PROGRAMME §8 step 5). It stages the graph to a space-free name on NVMe, runs Swarm under a `script` pty with `--silent`, passes `--seed`, `--resolution` and `-v` variables (restricted to command-line-safe characters), decides success by files written, never by exit code, cleans staging on success and failure, and writes `gaea-build-receipt.json`. `compare` pixel-diffs two builds.
- `pipeline/gaea_mcp_server.py`: stdio MCP server `wgeGaea` (registered in `.mcp.json`) with `gaea_status`, `gaea_list_examples`, `gaea_inspect_graph`, `gaea_mark_export`, `gaea_insert_node`, `gaea_build`, `gaea_compare_builds`. Edits never write in place or into Gaea's `Examples/`.
- Swarm also builds nothing, silently, for a graph whose `BuildDefinition` has no `Type` (29 of 59 examples). `add_save_definition` fills it in as `Standard`; `gaea_build.preflight` refuses a graph with no export or no build Type before Swarm is launched.
- `gaea_terrain.load_graph` accepts the trailing commas Gaea's Newtonsoft writer allows (1 of 59 examples, `Glacier - Complex Setup`, failed to load before).
- **Verified live:** `Snowy Ridge` built at 512 in 21–25 s (five builds); `Detailed Snow Peak` built through the MCP server over stdio in 30.5 s, producing height, snow, hardness and depth maps. A graph with no `SaveDefinition` raised `GaeaBuildError` as required. Tests: `tests/test_gaea_build.py` (24 cases; 5 of 5 driver mutations caught).
- Not done: the graph-variable path (`-v`) is wired and unit-tested but not exercised live, because no bundled example exposes variables. It gets its first live use in P2.

### P2. Alps-style Gaea graph
- Ridged multifractal base, hydraulic and thermal erosion for drainage and arête structure, snow, rock and scree masks by slope and altitude, exported as height plus masks.
- Input: Matt's `MountainRange` graph as the starting point, or a real DEM through `fetch_dem.py` / `import_dem.py`.
- Accept (with P4): N-1 ridge contrast, far-ridge contrast against the sky ≤ 0.40 of the foreground's (currently 0.65, not met since CONVERGE-1 because the 480 m world is too small).

### P3. Digest pinning
OPEN_DECISIONS item 9 was in fact resolved on 2026-08-03 (WGE drives Gaea; digest-pinned imports stay Grade A as declared inputs). This plan's first draft called it undecided; that was wrong.

**Measured 2026-10-05, which changes what "pinned" has to mean:**
- **File digests are useless.** Gaea writes `date:create` / `date:modify` into every PNG, so two builds with identical pixels have different file SHA-256s.
- **The `Mountain` primitive is pixel-identical across builds.**
- **`Erosion2` is not**, with or without `--safemode`. Two builds of `Snowy Ridge` at 512, same seed: height differs by up to 113/65535 (p99 30/65535) on 65–69% of pixels across three pairs; about 4k snow-mask pixels flip.

Consequences, now implemented in the receipt:
- The receipt pins **decoded pixel digests** (mode + shape + pixel bytes), the `.terrain` graph SHA-256, seed, resolution, variables, the `Gaea.Swarm.dll` and `Gaea.Nodes.dll` digests, and the Proton version.
- An eroded heightfield is a **source artifact**: it reproduces exactly from its pinned bytes, and regenerates from its graph only within tolerance. The built output must be kept, not rebuilt on demand (GAEA_PROGRAMME §7 already argued this).
- Remaining for P3: commit policy and location for the built heightfields that P2 accepts.

### P4. Far-field backdrop renderer (CONVERGE-4 N-3)
- Distant heightfield ring or clipmap at roughly 5 to 20 km, lower resolution than the arena.
- Aerial perspective: distance desaturation and blue shift, height fog, so ridges layer into silhouettes. This carries as much of the Alps read as the geometry.
- Slope and altitude layering (snow, rock, scree) driven by the P2 masks.

### P5. Near-field terrain surface (GP-10)
- World-metre UVs with tiling (terrain UV is currently normalised 0 to 1 across the field with `wrap = :clamp`, about 1.67 texels/m).
- Detail-normal layer, macro colour variation, slope and height splat, anisotropic filtering.
- Ground cover chosen per biome (CONVERGE-3 review: current trees and terrain read as arid; the fern does not belong).

### P6. Lighting grounding
- Cascaded shadows (L-1) first: the single 512² map covers 60 m. Then contact AO and depth capture (N-8), measured after scatter.

## Order

P1 → P2 and P3 together → P5 → P4 → P6. P5 before P4 because the splat system is shared by both, and the near field is the biggest visible jump per effort.

## Out of scope

- Real-time GI, volumetric god-rays, water beyond still pools (L-5), wind (L-3). Tracked in CONVERGE-4's carried list.
- Building a Gaea replacement.

## Open risks

- Gaea runs under Proton on this machine; a Gaea or Proton update could break the headless recipe. Mitigation: P3 pins the Gaea version.
- The N-1 ridge-contrast gate (≤ 0.40, currently 0.65) is a haze-at-distance gate: it needs distance, which a km-scale backdrop supplies, and fog. Measure after P4, not after P2.
- The `.terrain` format is undocumented and may change between Gaea versions.
