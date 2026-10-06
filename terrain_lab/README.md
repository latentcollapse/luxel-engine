# Luxel Terrain Lab

`terrain_lab/` is the Julia numerical/spatial package in the active Luxel path.
Julia owns terrain analysis, hydrology, placement fields, and related numerical
experiments. It does not own world identity, semantic authority, receipt
promotion, or engine rendering. Typed packets and provenance cross back to the
Rust authority plane.

The environment is locked by `Project.toml` and `Manifest.toml`.

```bash
julia --project=terrain_lab --startup-file=no -e 'using Pkg; Pkg.instantiate()'
julia --project=terrain_lab --startup-file=no terrain_lab/test/runtests.jl
```

For package-specific utilities, inspect `terrain_lab/bin/` and the current
tests before constructing a command. Do not copy old paths under
`godot_renderer/`; those belong to the historical migration lane.

Julia output is evidence or a bounded numerical field, not permission to mutate
canonical terrain. Rust validates provenance, policy, determinism, and any
promotion decision.
