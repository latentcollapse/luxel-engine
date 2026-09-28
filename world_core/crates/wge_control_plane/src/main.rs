use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wge_control_plane::{ProjectStore, WorkOrderResult, profile_gates};
use wge_project_ledger::WorkOrder;

fn main() -> ExitCode {
    match dispatch() {
        Ok(value) => {
            println!(
                "{}",
                serde_json::to_string(&value).expect("JSON values serialize")
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("wge-control-plane: {error}");
            ExitCode::from(2)
        }
    }
}

fn dispatch() -> Result<Value, String> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or_else(usage)?;
    match command.as_str() {
        "create" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let spec = PathBuf::from(required_flag(&mut args, "--spec")?);
            let profile = required_flag(&mut args, "--profile")?;
            reject_extra(&mut args)?;
            let store = ProjectStore::create(&root, &spec, &profile).map_err(|e| e.to_string())?;
            Ok(store.inspect_project())
        }
        "open" | "inspect-project" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            reject_extra(&mut args)?;
            let store = ProjectStore::open(&root).map_err(|e| e.to_string())?;
            Ok(store.inspect_project())
        }
        "inspect-current" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            reject_extra(&mut args)?;
            ProjectStore::open(&root)
                .map_err(|e| e.to_string())?
                .inspect_current()
                .map_err(|e| e.to_string())
        }
        "inspect-failures" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let candidate = required_flag(&mut args, "--candidate")?;
            reject_extra(&mut args)?;
            ProjectStore::open(&root)
                .map_err(|e| e.to_string())?
                .inspect_failures(&candidate)
                .map_err(|e| e.to_string())
        }
        "inspect-artifact" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let candidate = required_flag(&mut args, "--candidate")?;
            let artifact = required_flag(&mut args, "--artifact")?;
            reject_extra(&mut args)?;
            ProjectStore::open(&root)
                .map_err(|e| e.to_string())?
                .inspect_artifact(&candidate, &artifact)
                .map_err(|e| e.to_string())
        }
        "propose-work" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let work_order_path = PathBuf::from(required_flag(&mut args, "--work-order")?);
            let capability_text = required_flag(&mut args, "--capabilities")?;
            reject_extra(&mut args)?;
            let work_order: WorkOrder = read_json(&work_order_path)?;
            let store = ProjectStore::open(&root).map_err(|e| e.to_string())?;
            let proposal = store
                .propose_work_order(&work_order, &parse_capabilities(&capability_text))
                .map_err(|e| e.to_string())?;
            serde_json::to_value(proposal).map_err(|e| e.to_string())
        }
        "create-candidate" | "build-candidate" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let manifest = PathBuf::from(required_flag(&mut args, "--manifest")?);
            let artifact_root = PathBuf::from(required_flag(&mut args, "--artifact-root")?);
            reject_extra(&mut args)?;
            let store = ProjectStore::open(&root).map_err(|e| e.to_string())?;
            let candidate = store
                .create_candidate(
                    &manifest,
                    &artifact_root,
                    store.manifest().current_snapshot_id.as_deref(),
                )
                .map_err(|e| e.to_string())?;
            serde_json::to_value(candidate).map_err(|e| e.to_string())
        }
        "run-playtest" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let candidate = required_flag(&mut args, "--candidate")?;
            let world_artifact = required_flag(&mut args, "--world-artifact")?;
            reject_extra(&mut args)?;
            ProjectStore::open(&root)
                .map_err(|e| e.to_string())?
                .run_playtest(&candidate, &world_artifact)
                .map_err(|e| e.to_string())
        }
        "capture-evidence" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let candidate = required_flag(&mut args, "--candidate")?;
            let world_artifact = required_flag(&mut args, "--world-artifact")?;
            let output = required_flag(&mut args, "--output")?;
            reject_extra(&mut args)?;
            ProjectStore::open(&root)
                .map_err(|e| e.to_string())?
                .capture_evidence(&candidate, &world_artifact, &output)
                .map_err(|e| e.to_string())
        }
        "attach-evidence" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let candidate = required_flag(&mut args, "--candidate")?;
            let request = PathBuf::from(required_flag(&mut args, "--request")?);
            reject_extra(&mut args)?;
            ProjectStore::open(&root)
                .map_err(|e| e.to_string())?
                .attach_evidence(&candidate, &request)
                .map_err(|e| e.to_string())?;
            Ok(json!({"status":"attached","candidate_id":candidate}))
        }
        "validate-candidate" | "evaluate-candidate" | "verify-candidate" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let candidate = required_flag(&mut args, "--candidate")?;
            reject_extra(&mut args)?;
            let report = ProjectStore::open(&root)
                .map_err(|e| e.to_string())?
                .validate_candidate(&candidate)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(report).map_err(|e| e.to_string())
        }
        "propose-repair" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let candidate = required_flag(&mut args, "--candidate")?;
            reject_extra(&mut args)?;
            let proposal = ProjectStore::open(&root)
                .map_err(|e| e.to_string())?
                .propose_repair(&candidate)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(proposal).map_err(|e| e.to_string())
        }
        "commit-candidate" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let candidate = required_flag(&mut args, "--candidate")?;
            reject_extra(&mut args)?;
            let mut store = ProjectStore::open(&root).map_err(|e| e.to_string())?;
            let snapshot = store
                .commit_candidate(&candidate)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(snapshot).map_err(|e| e.to_string())
        }
        "rollback" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let snapshot_id = required_flag(&mut args, "--snapshot")?;
            reject_extra(&mut args)?;
            let mut store = ProjectStore::open(&root).map_err(|e| e.to_string())?;
            let snapshot = store.rollback(&snapshot_id).map_err(|e| e.to_string())?;
            serde_json::to_value(snapshot).map_err(|e| e.to_string())
        }
        "execute-work-order" | "apply-repair" => {
            let root = PathBuf::from(required(&mut args, "root")?);
            let work_order_path = PathBuf::from(required_flag(&mut args, "--work-order")?);
            let result_path = PathBuf::from(required_flag(&mut args, "--result")?);
            let capability_text = required_flag(&mut args, "--capabilities")?;
            reject_extra(&mut args)?;
            let work_order: WorkOrder = read_json(&work_order_path)?;
            let result: WorkOrderResult = read_json(&result_path)?;
            let capabilities = parse_capabilities(&capability_text);
            let store = ProjectStore::open(&root).map_err(|e| e.to_string())?;
            let receipt = if command == "apply-repair" {
                store.execute_repair_work_order(
                    &work_order.snapshot_id,
                    &work_order,
                    &result,
                    &capabilities,
                )
            } else {
                store.execute_work_order(&work_order, &result, &capabilities)
            }
            .map_err(|e| e.to_string())?;
            serde_json::to_value(receipt).map_err(|e| e.to_string())
        }
        "profile" => {
            let profile = required(&mut args, "profile")?;
            reject_extra(&mut args)?;
            let gates = profile_gates(&profile).map_err(|e| e.to_string())?;
            Ok(
                json!({"schema_version":"wge.control-profile/v1","profile_id":profile,"gates":gates}),
            )
        }
        "help" | "--help" | "-h" => Ok(json!({"usage":usage()})),
        _ => Err(usage()),
    }
}

fn required(args: &mut impl Iterator<Item = String>, label: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("missing {label}"))
}

fn required_flag(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    let value = required(args, flag)?;
    if value == flag {
        required(args, flag)
    } else {
        Err(format!("expected {flag}, found {value}"))
    }
}

fn reject_extra(args: &mut impl Iterator<Item = String>) -> Result<(), String> {
    if let Some(extra) = args.next() {
        Err(format!("unexpected argument {extra}"))
    } else {
        Ok(())
    }
}

fn parse_capabilities(value: &str) -> BTreeSet<String> {
    value
        .split(',')
        .filter(|item| !item.trim().is_empty())
        .map(|item| item.trim().to_owned())
        .collect()
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("malformed {}: {e}", path.display()))
}

fn usage() -> String {
    "usage: wge-control-plane <create ROOT --spec SPEC --profile PROFILE | open ROOT | inspect-project ROOT | inspect-current ROOT | inspect-failures ROOT --candidate ID | inspect-artifact ROOT --candidate ID --artifact ID | propose-work ROOT --work-order ORDER --capabilities CAP,... | create-candidate/build-candidate ROOT --manifest MANIFEST --artifact-root ROOT | attach-evidence ROOT --candidate ID --request REQUEST | run-playtest ROOT --candidate ID --world-artifact ID | capture-evidence ROOT --candidate ID --world-artifact ID --output RELATIVE_DIR | validate/evaluate/verify-candidate ROOT --candidate ID | propose-repair ROOT --candidate ID | commit-candidate ROOT --candidate ID | rollback ROOT --snapshot ID | execute-work-order/apply-repair ROOT --work-order ORDER --result RESULT --capabilities CAP,... | profile PROFILE>".into()
}
