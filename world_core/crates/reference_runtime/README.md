# WGE engine-neutral reference world runtime

This crate compiles a typed authored layout through a Julia field worker into a
content-addressed Rust world artifact. Rust validates the returned height,
slope, and semantic-region grids; derives collision and grade-limited
navigation; runs traversal; replays the existing gameplay contract against
actual world route cells; and produces a deterministic PPM capture with a
measured visual receipt.

Build a fresh authored layout:

```sh
cargo run --offline -p wge-reference-runtime -- build \
  --layout world_core/crates/reference_runtime/examples/riverwatch.layout.json \
  --output-dir /tmp/wge-riverwatch
```

Independently revalidate the stored candidate and evidence:

```sh
cargo run --offline -p wge-reference-runtime -- verify --bundle /tmp/wge-riverwatch
```

`build` writes the authored-layout-derived world artifact, traversal receipt,
gameplay/world binding, deterministic `reference_capture.ppm`, visual receipt,
and a gate report. A failed visual gate is preserved as failed evidence and
returns exit code 2. The crate does not inspect or promote the supplied GLB and
has no engine import dependency.

The JSON exchange schema and Julia worker are in
`terrain_lab/bin/wge_reference_world_fields.jl`. The receipt retains exact
request/response bytes and pins the worker and terrain project digests. Rust
also rerasterizes authored regions and recomputes the slope field before using
the worker values.
