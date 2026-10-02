use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use wge_control_plane::{
    ProjectStore, WorkOrderResult, capability_registry, construction_plan, profile_gates,
    semantic_facade, style_profile,
};
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
        "capabilities" => {
            let (class, status) = optional_capability_filters(&mut args)?;
            reject_extra(&mut args)?;
            let catalog = capability_registry::CapabilityCatalog::native_v1();
            catalog.validate().map_err(|error| error.to_string())?;
            let class = match class.as_deref() {
                Some(value) => Some(
                    capability_registry::class_from_str(value)
                        .ok_or_else(|| format!("unknown capability class {value}"))?,
                ),
                None => None,
            };
            let status = match status.as_deref() {
                Some(value) => Some(
                    capability_registry::status_from_str(value)
                        .ok_or_else(|| format!("unknown capability status {value}"))?,
                ),
                None => None,
            };
            Ok(serde_json::json!({
                "schema_version": capability_registry::CAPABILITY_REGISTRY_SCHEMA,
                "registry_id": catalog.registry_id.clone(),
                "registry_sha256": catalog.registry_sha256.clone(),
                "capabilities": catalog.list(class, status),
            }))
        }
        "capability-explain" => {
            let id = required(&mut args, "capability id")?;
            reject_extra(&mut args)?;
            let catalog = capability_registry::CapabilityCatalog::native_v1();
            catalog.validate().map_err(|error| error.to_string())?;
            capability_registry::descriptor_json(&catalog, &id).map_err(|error| error.to_string())
        }
        "facade" => {
            let status = optional_facade_status(&mut args)?;
            reject_extra(&mut args)?;
            let catalog = semantic_facade::SemanticFacadeCatalog::native_v1();
            catalog.validate().map_err(|error| error.to_string())?;
            let status = status
                .as_deref()
                .map(semantic_facade_status_from_str)
                .transpose()?;
            Ok(json!({
                "schema_version": semantic_facade::SEMANTIC_FACADE_SCHEMA,
                "facade_id": catalog.facade_id,
                "facade_sha256": catalog.facade_sha256,
                "operations": catalog.list(status),
            }))
        }
        "facade-explain" => {
            let id = required(&mut args, "facade operation id")?;
            reject_extra(&mut args)?;
            let catalog = semantic_facade::SemanticFacadeCatalog::native_v1();
            catalog.validate().map_err(|error| error.to_string())?;
            catalog.explain(&id).map_err(|error| error.to_string())
        }
        "style-validate" | "style-lower" => {
            let profile_path = PathBuf::from(required(&mut args, "style profile")?);
            reject_extra(&mut args)?;
            let profile: style_profile::StyleProfile = read_json(&profile_path)?;
            profile.validate().map_err(|error| error.to_string())?;
            if command == "style-validate" {
                Ok(serde_json::json!({
                    "schema_version": style_profile::STYLE_PROFILE_SCHEMA,
                    "status": "valid",
                    "profile_id": profile.profile_id,
                    "profile_sha256": profile.profile_sha256,
                }))
            } else {
                let catalog = capability_registry::CapabilityCatalog::native_v1();
                let plan = style_profile::StylePlan::lower(&profile, &catalog)
                    .map_err(|error| error.to_string())?;
                serde_json::to_value(plan).map_err(|error| error.to_string())
            }
        }
        "project-plan" => {
            let draft_path = PathBuf::from(required(&mut args, "construction plan draft")?);
            let style_path = PathBuf::from(required(&mut args, "style plan")?);
            reject_extra(&mut args)?;
            let draft: construction_plan::ConstructionPlanDraft = read_json(&draft_path)?;
            let style: style_profile::StylePlan = read_json(&style_path)?;
            let plan = construction_plan::ConstructionPlan::compile(&draft, &style)
                .map_err(|error| error.to_string())?;
            serde_json::to_value(plan).map_err(|error| error.to_string())
        }
        "construction-validate" => {
            let plan_path = PathBuf::from(required(&mut args, "construction plan")?);
            let style_path = PathBuf::from(required(&mut args, "style plan")?);
            reject_extra(&mut args)?;
            let plan: construction_plan::ConstructionPlan = read_json(&plan_path)?;
            let style: style_profile::StylePlan = read_json(&style_path)?;
            plan.validate(&style).map_err(|error| error.to_string())?;
            Ok(json!({
                "schema_version": construction_plan::CONSTRUCTION_PLAN_SCHEMA,
                "status": "valid",
                "plan_id": plan.plan_id,
                "plan_sha256": plan.plan_sha256,
                "readiness": plan.readiness,
            }))
        }
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

fn optional_capability_filters(
    args: &mut impl Iterator<Item = String>,
) -> Result<(Option<String>, Option<String>), String> {
    let mut class = None;
    let mut status = None;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--class" => {
                if class.is_some() {
                    return Err("duplicate --class".into());
                }
                class = Some(required(args, "--class")?);
            }
            "--status" => {
                if status.is_some() {
                    return Err("duplicate --status".into());
                }
                status = Some(required(args, "--status")?);
            }
            _ => return Err(format!("unexpected argument {flag}")),
        }
    }
    Ok((class, status))
}

fn optional_facade_status(
    args: &mut impl Iterator<Item = String>,
) -> Result<Option<String>, String> {
    match args.next() {
        None => Ok(None),
        Some(flag) if flag == "--status" => Ok(Some(required(args, "--status")?)),
        Some(flag) => Err(format!("unexpected argument {flag}")),
    }
}

fn semantic_facade_status_from_str(value: &str) -> Result<semantic_facade::FacadeStatus, String> {
    match value {
        "available" => Ok(semantic_facade::FacadeStatus::Available),
        "partial" => Ok(semantic_facade::FacadeStatus::Partial),
        "planned" => Ok(semantic_facade::FacadeStatus::Planned),
        "deferred" => Ok(semantic_facade::FacadeStatus::Deferred),
        _ => Err(format!("unknown semantic facade status {value}")),
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
    "usage: wge-control-plane <capabilities [--class CLASS] [--status STATUS] | capability-explain ID | facade [--status STATUS] | facade-explain ID | style-validate PROFILE | style-lower PROFILE | project-plan DRAFT STYLE_PLAN | construction-validate PLAN STYLE_PLAN | create ROOT --spec SPEC --profile PROFILE | open ROOT | inspect-project ROOT | inspect-current ROOT | inspect-failures ROOT --candidate ID | inspect-artifact ROOT --candidate ID --artifact ID | propose-work ROOT --work-order ORDER --capabilities CAP,... | create-candidate/build-candidate ROOT --manifest MANIFEST --artifact-root ROOT | attach-evidence ROOT --candidate ID --request REQUEST | run-playtest ROOT --candidate ID --world-artifact ID | capture-evidence ROOT --candidate ID --world-artifact ID --output RELATIVE_DIR | validate/evaluate/verify-candidate ROOT --candidate ID | propose-repair ROOT --candidate ID | commit-candidate ROOT --candidate ID | rollback ROOT --snapshot ID | execute-work-order/apply-repair ROOT --work-order ORDER --result RESULT --capabilities CAP,... | profile PROFILE>".into()
}
