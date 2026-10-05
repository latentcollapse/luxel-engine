# Vendored dependencies

## Lava

`vendor/Lava` is SimonDanisch/Lava.jl at commit
`11c7e31bdf62408d22bf379e9e59510f69d2103e` (git tree
`795df8f1fb847d261f4da4d944448c4d96683862`), `src/`, `Project.toml`,
`LICENSE` and `README.md` only, with these patches applied in order:

| Patch | What it adds | Why |
|---|---|---|
| `lava-0001-fragment-discard.patch` | `Lava.discard()`: a fragment-stage block terminator emitted as SPIR-V `OpKill` | Alpha-mask materials (CONVERGE-2 N-6). Upstream has no discard. |

The patches only add code. A shader that does not call `discard()` compiles
to the same SPIR-V as upstream, so `LAVA_REVISION` stays the base commit.

To verify: check out upstream at the commit above, copy `src/` over a clean
directory, apply the patches with `patch -p1`, and `diff -r` against
`vendor/Lava/src`.
