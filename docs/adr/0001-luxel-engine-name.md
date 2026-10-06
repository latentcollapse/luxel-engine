# 0001 — The engine is called Luxel Engine

2026-10-06 · Matt · Accepted

## Decision

The engine's name is **Luxel Engine** ("Luxel" in running text). "WGE" and
"WorldGen Engine" were internal development codenames and are retired.
The internal identifier prefix is `luxel`. The GitHub repository is
`latentcollapse/luxel-engine` and the local checkout is
`Code Projects/luxel-engine/`.

## The migration (one change, 2026-10-06)

Everything that ships in the repository was renamed together:

| Kind | Before | After |
|---|---|---|
| Prose and names | WGE | Luxel |
| Schema ids (packets, receipts, locks, specs) | `wge.*` | `luxel.*` |
| Crates, binaries | `wge-*`, `wge_control_plane` | `luxel-*`, `luxel_control_plane` |
| Python modules and tests | `pipeline/wge_*.py`, `tests/test_wge_*.py` | `pipeline/luxel_*.py`, `tests/test_luxel_*.py` |
| Julia package | `WGEGraphics` | `LuxelGraphics` |
| Env vars | `WGE_*` | `LUXEL_*` |
| DSL namespace and source extension | `wge.world`, `.wge` | `luxel.world`, `.luxel` |
| Unity adapter types | `WgeMvp*` | `LuxelMvp*` |
| MCP server | `wgeGaea` | `luxelGaea` |

Archived docs were renamed along with everything else: the repository has one
name. Two kinds of text were deliberately left alone: the names of real files
outside the repository (`WGE-checkpoint-*.zip`), and `codeweald.*` identifiers,
which belong to the game, not the engine.

## What it cost, and how it was verified

- **The language contract was re-frozen.** `tests/dsl_conformance/freeze_v06.json`
  pins its evidence files by SHA-256; their names and contents changed (the
  namespace and extension are part of the language), so the digests were
  recomputed and the re-freeze recorded in the file. No semantic change.
- **Hashed packets and receipts changed digest** wherever a schema id is
  inside them. Captures did not: the identity check compares rendered pixels,
  and the 131 reference frames re-render byte-identical under the new names.
- **Locally built assets** that embed generator strings were rebuilt and
  their locks re-pinned.
