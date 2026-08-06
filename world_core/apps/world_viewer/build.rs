use std::env;

/// Bakes the source-tree digest computed by
/// `godot_renderer/pipeline/build_viewer.py` into the binary, so it can
/// self-report what sources it was actually built from via `--provenance`.
///
/// A binary built any other way (plain `cargo build`/`cargo run`) has no
/// digest to report and embeds "unknown" instead of a wrong-but-plausible
/// one -- tooling item 1 (tool provenance): unknown provenance must be
/// refused by consumers, not treated as trustworthy by default.
fn main() {
    let digest = env::var("CODEWEALD_SOURCE_DIGEST").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=CODEWEALD_SOURCE_DIGEST={digest}");
    println!("cargo:rerun-if-env-changed=CODEWEALD_SOURCE_DIGEST");
}
