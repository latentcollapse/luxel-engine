use std::env;
use std::process::ExitCode;

use serde_json::json;
use wge_project_ledger::{
    EvidenceBundle, ProjectSnapshot, ProjectSpec, build_unity_import_manifest, canonical_json,
    commit_snapshot, load_json, spec_digest, validate_snapshot, validate_spec,
    validate_world_bundle, write_json,
};

fn main() -> ExitCode {
    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn dispatch() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or_else(usage)?;
    match command.as_str() {
        "validate-spec" => {
            let path = args.next().ok_or_else(usage)?;
            let spec: ProjectSpec = load_json(&path).map_err(|error| error.to_string())?;
            validate_spec(&spec).map_err(|error| error.to_string())?;
            let root = std::path::Path::new(&path)
                .parent()
                .ok_or_else(|| "spec path has no candidate root".to_owned())?;
            validate_world_bundle(root, &spec).map_err(|error| error.to_string())?;
            println!(
                "{}",
                canonical_json(&json!({
                    "status": "valid",
                    "schema_version": spec.schema_version,
                    "project_id": spec.project_id,
                    "spec_sha256": spec_digest(&spec).map_err(|error| error.to_string())?,
                    "artifact_count": spec.artifact_graph.len(),
                    "required_gates": spec.required_gates.iter().map(|gate| gate.gate_id.clone()).collect::<Vec<_>>(),
                }))
            );
            Ok(())
        }
        "commit" => {
            let spec_path = args.next().ok_or_else(usage)?;
            let evidence_flag = args.next().ok_or_else(usage)?;
            if evidence_flag != "--evidence" {
                return Err(usage());
            }
            let evidence_path = args.next().ok_or_else(usage)?;
            let output_flag = args.next().ok_or_else(usage)?;
            if output_flag != "--output" {
                return Err(usage());
            }
            let output = args.next().ok_or_else(usage)?;
            let spec: ProjectSpec = load_json(&spec_path).map_err(|error| error.to_string())?;
            let root = std::path::Path::new(&spec_path)
                .parent()
                .ok_or_else(|| "spec path has no candidate root".to_owned())?;
            validate_world_bundle(root, &spec).map_err(|error| error.to_string())?;
            let evidence: EvidenceBundle =
                load_json(&evidence_path).map_err(|error| error.to_string())?;
            let snapshot = commit_snapshot(&spec, &evidence).map_err(|error| error.to_string())?;
            write_json(&output, &snapshot).map_err(|error| error.to_string())?;
            println!(
                "{}",
                canonical_json(
                    &json!({"status":"certified","snapshot_id":snapshot.snapshot_id,"snapshot_sha256":snapshot.snapshot_sha256,"output":output})
                )
            );
            Ok(())
        }
        "validate-snapshot" => {
            let path = args.next().ok_or_else(usage)?;
            let snapshot: ProjectSnapshot = load_json(&path).map_err(|error| error.to_string())?;
            validate_snapshot(&snapshot).map_err(|error| error.to_string())?;
            println!(
                "{}",
                canonical_json(
                    &json!({"status":"valid","snapshot_id":snapshot.snapshot_id,"snapshot_sha256":snapshot.snapshot_sha256})
                )
            );
            Ok(())
        }
        "unity-manifest" => {
            let spec_path = args.next().ok_or_else(usage)?;
            let snapshot_flag = args.next().ok_or_else(usage)?;
            if snapshot_flag != "--snapshot" {
                return Err(usage());
            }
            let snapshot_path = args.next().ok_or_else(usage)?;
            let output_flag = args.next().ok_or_else(usage)?;
            if output_flag != "--output" {
                return Err(usage());
            }
            let output = args.next().ok_or_else(usage)?;
            let spec: ProjectSpec = load_json(&spec_path).map_err(|error| error.to_string())?;
            let root = std::path::Path::new(&spec_path)
                .parent()
                .ok_or_else(|| "spec path has no candidate root".to_owned())?;
            validate_world_bundle(root, &spec).map_err(|error| error.to_string())?;
            let snapshot: ProjectSnapshot =
                load_json(&snapshot_path).map_err(|error| error.to_string())?;
            let manifest =
                build_unity_import_manifest(&spec, &snapshot).map_err(|error| error.to_string())?;
            write_json(&output, &manifest).map_err(|error| error.to_string())?;
            println!(
                "{}",
                canonical_json(
                    &json!({"status":"ready","schema_version":manifest.schema_version,"snapshot_sha256":manifest.snapshot_sha256,"output":output})
                )
            );
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage: wge-project-ledger validate-spec SPEC.json | commit SPEC.json --evidence EVIDENCE.json --output SNAPSHOT.json | validate-snapshot SNAPSHOT.json | unity-manifest SPEC.json --snapshot SNAPSHOT.json --output MANIFEST.json".into()
}
