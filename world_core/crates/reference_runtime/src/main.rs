use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Serialize;
use wge_reference_runtime::{
    GameplayWorldBinding, TraversalEvidence, VisualEvidence, WorldArtifact, build_from_layout_path,
    validate_gameplay_world_binding, validate_traversal_evidence, validate_visual_evidence,
    validate_world_artifact,
};

#[derive(Serialize)]
struct CandidateReport<'a> {
    schema_version: &'static str,
    status: &'static str,
    reference_gates_passed: bool,
    world_artifact_id: &'a str,
    world_artifact_sha256: &'a str,
    spatial_fields_sha256: &'a str,
    julia_worker_id: &'a str,
    julia_version: &'a str,
    traversal_outcome: &'a str,
    traversal_steps: usize,
    gameplay_outcome: &'a str,
    gameplay_receipt_sha256: &'a str,
    visual_status: &'a str,
    visual_evidence_sha256: &'a str,
    visual_failure_reasons: &'a [String],
}

fn main() -> ExitCode {
    match dispatch() {
        Ok(status) => ExitCode::from(status),
        Err(error) => {
            eprintln!(
                "{{\"status\":\"rejected\",\"error\":{:?}}}",
                error.to_string()
            );
            ExitCode::from(1)
        }
    }
}

fn dispatch() -> Result<u8, Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or("missing command")?;
    let mut flags = parse_flags(args)?;
    match command.as_str() {
        "build" => {
            let layout = take(&mut flags, "layout")?;
            let output_dir = PathBuf::from(take(&mut flags, "output-dir")?);
            let julia = flags
                .remove("julia")
                .or_else(|| env::var("WGE_JULIA").ok())
                .unwrap_or_else(|| "julia".into());
            reject_extra(flags)?;
            let terrain_lab = terrain_lab_root();
            let build =
                build_from_layout_path(Path::new(&layout), Path::new(&julia), &terrain_lab)?;
            fs::create_dir_all(&output_dir)?;
            write_json(&output_dir.join("world_artifact.json"), &build.world)?;
            write_json(
                &output_dir.join("traversal_evidence.json"),
                &build.traversal,
            )?;
            write_json(
                &output_dir.join("gameplay_world_binding.json"),
                &build.gameplay,
            )?;
            fs::write(
                output_dir.join("reference_capture.ppm"),
                &build.capture_bytes,
            )?;
            write_json(&output_dir.join("visual_evidence.json"), &build.visual)?;
            let passed = build.reference_gates_passed();
            let report = CandidateReport {
                schema_version: "wge.reference-runtime-candidate/v1",
                status: if passed { "passed" } else { "failed" },
                reference_gates_passed: passed,
                world_artifact_id: &build.world.artifact_id,
                world_artifact_sha256: &build.world.artifact_sha256,
                spatial_fields_sha256: &build.world.body.fields.spatial_sha256,
                julia_worker_id: &build.world.body.julia_provenance.worker_id,
                julia_version: &build.world.body.julia_provenance.julia_version,
                traversal_outcome: match build.traversal.body.outcome {
                    wge_reference_runtime::TraversalOutcome::Completed => "completed",
                    wge_reference_runtime::TraversalOutcome::Incomplete => "incomplete",
                },
                traversal_steps: build.traversal.body.steps.len(),
                gameplay_outcome: match build.gameplay.body.outcome {
                    wge_gameplay_contract::GameOutcome::Won => "won",
                    wge_gameplay_contract::GameOutcome::Lost => "lost",
                    wge_gameplay_contract::GameOutcome::InProgress => "in_progress",
                },
                gameplay_receipt_sha256: &build.gameplay.body.gameplay_receipt.receipt_sha256,
                visual_status: match build.visual.body.status {
                    wge_reference_runtime::VisualGateStatus::Passed => "passed",
                    wge_reference_runtime::VisualGateStatus::Failed => "failed",
                },
                visual_evidence_sha256: &build.visual.evidence_sha256,
                visual_failure_reasons: &build.visual.body.failure_reasons,
            };
            write_json(&output_dir.join("candidate_report.json"), &report)?;
            println!("{}", serde_json::to_string(&report)?);
            Ok(if passed { 0 } else { 2 })
        }
        "verify" => {
            let bundle = PathBuf::from(take(&mut flags, "bundle")?);
            reject_extra(flags)?;
            let world: WorldArtifact = read_json(&bundle.join("world_artifact.json"))?;
            let traversal: TraversalEvidence = read_json(&bundle.join("traversal_evidence.json"))?;
            let gameplay: GameplayWorldBinding =
                read_json(&bundle.join("gameplay_world_binding.json"))?;
            let visual: VisualEvidence = read_json(&bundle.join("visual_evidence.json"))?;
            let capture = fs::read(bundle.join("reference_capture.ppm"))?;
            validate_world_artifact(&world)?;
            validate_traversal_evidence(&world, &traversal)?;
            validate_visual_evidence(&world, &capture, &visual)?;
            validate_gameplay_world_binding(&world, &traversal, &capture, &visual, &gameplay)?;
            let passed = traversal.body.outcome
                == wge_reference_runtime::TraversalOutcome::Completed
                && gameplay.body.outcome == wge_gameplay_contract::GameOutcome::Won
                && visual.body.status == wge_reference_runtime::VisualGateStatus::Passed;
            println!(
                "{}",
                serde_json::json!({
                    "schema_version": "wge.reference-runtime-verification/v1",
                    "status": if passed { "passed" } else { "failed" },
                    "world_artifact_id": world.artifact_id,
                    "world_artifact_sha256": world.artifact_sha256,
                    "traversal_evidence_sha256": traversal.evidence_sha256,
                    "gameplay_binding_sha256": gameplay.evidence_sha256,
                    "visual_evidence_sha256": visual.evidence_sha256,
                    "visual_failure_reasons": visual.body.failure_reasons,
                    "independent_native_revalidation": true
                })
            );
            Ok(if passed { 0 } else { 2 })
        }
        "help" | "--help" | "-h" => {
            println!(
                "wge-reference-runtime\n\n  build --layout FILE --output-dir DIR [--julia PATH]\n  verify --bundle DIR\n\nBuild creates world_artifact.json, traversal_evidence.json, gameplay_world_binding.json, reference_capture.ppm, visual_evidence.json, and candidate_report.json. A measured visual failure is retained in the bundle and exits 2."
            );
            Ok(0)
        }
        _ => Err(format!("unknown command {command:?}; use help").into()),
    }
}

fn parse_flags(
    mut args: impl Iterator<Item = String>,
) -> Result<std::collections::BTreeMap<String, String>, Box<dyn std::error::Error>> {
    let mut flags = std::collections::BTreeMap::new();
    while let Some(key) = args.next() {
        let Some(key) = key.strip_prefix("--") else {
            return Err(format!("unexpected argument {key:?}").into());
        };
        let value = args
            .next()
            .ok_or_else(|| format!("--{key} requires a value"))?;
        if flags.insert(key.to_owned(), value).is_some() {
            return Err(format!("--{key} was supplied more than once").into());
        }
    }
    Ok(flags)
}

fn take(
    flags: &mut std::collections::BTreeMap<String, String>,
    key: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    flags
        .remove(key)
        .ok_or_else(|| format!("required flag --{key} is missing").into())
}

fn reject_extra(
    flags: std::collections::BTreeMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    if flags.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "unsupported flags: {}",
            flags.keys().cloned().collect::<Vec<_>>().join(", ")
        )
        .into())
    }
}

fn terrain_lab_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("reference runtime must live under world_core/crates")
        .join("terrain_lab")
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    fs::write(path, bytes)?;
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
