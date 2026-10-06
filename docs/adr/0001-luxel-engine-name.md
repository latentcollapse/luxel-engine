# 0001 — The engine is called Luxel Engine

2026-10-06 · Matt · Accepted

## Decision

The engine's name is **Luxel Engine** ("Luxel" in running text). "WGE" and
"WorldGen Engine" were internal development codenames and are retired as
names. In any doc written before this date, read "WGE" as Luxel.

## What changes now

The name people read: `README.md`, `docs/README.md`, `docs/roadmap.md`, and
every doc written from here on. Archived docs, closed contracts and quoted
reviews keep their original wording; they are records.

## What deliberately keeps the `wge` prefix, for now

These are identifiers, not names, and renaming them is not free:

| Identifier | Why it stays |
|---|---|
| Schema ids inside packets and receipts (`wge.kit-lock/v1`, `wge.native-graphics-campaign2/v1`, ...) | They are hashed into packet and receipt digests. Renaming changes every digest, so all 131 identity references (`tools/verify_identity.py`) would have to be re-baselined in the same change. |
| `pipeline/worldbuilder_dsl.py`, `tests/test_wge_language_contract.py`, `docs/DSL docs/` | SHA-pinned by the language contract freeze (`tests/dsl_conformance/freeze_v06.json`). Renaming means a re-freeze. |
| Crate and module names (`wge-native-graphics-contract`, `wge_control_plane`, `pipeline/wge_*.py`) | Mechanical, but they touch imports, Cargo manifests and scripts across the tree. |
| Env vars (`WGE_PARITY_RENDER_POLICY`, `WGE_KIT_SET`, `WGE_BACKDROP_SET`, ...) | Read by scripts, tests and saved run logs. |
| The repo directory `Code Projects/WGE/` | Absolute paths in `.mcp.json`, `tools/parity_ab_policy.sh`, `tools/verify_identity.py`, and the Claude Code project memory path. |

## If the identifiers are renamed later

Do it as one deliberate migration, not piecemeal: rename schema ids, crates,
modules, env vars and the directory together; re-baseline the identity
references in the same commit; re-freeze the language contract with its
verifier; and keep a one-release alias for the env vars. Until then, `wge` in
an identifier is the engine's internal prefix and does not need explaining.
