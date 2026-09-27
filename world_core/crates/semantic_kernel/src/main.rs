//! Command line for the lane-overlap semantic transaction.
//!
//! There is no author-facing `commit` subcommand. Commit happens inside `run`
//! after the Rust predicate passes.

use std::env;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use serde_json::Value;
use wge_semantic_kernel::{
    ErosionSupervisorOptions, KernelFailure, RunOptions, Worker, accept_receipt, bind_evidence,
    canonical_json, certify_source, check_solver_image, create_store, current_generation, load_ir,
    normalize_semantic_ir, registry_digest, reject_author_effect, run_erosion_supervisor,
    run_specimen, script_digest, solver_image, verify_current,
};

fn main() -> ExitCode {
    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            println!("{}", failure.to_json());
            ExitCode::from(1)
        }
    }
}

fn dispatch() -> Result<(), KernelFailure> {
    let mut args = env::args().skip(1);
    let command = args
        .next()
        .ok_or_else(|| KernelFailure::operator("usage", "missing command"))?;
    let flags = Flags::parse(args);
    match command.as_str() {
        "normalize-ir" => {
            let mut input = String::new();
            std::io::stdin()
                .read_to_string(&mut input)
                .map_err(|error| KernelFailure::operator("unreadable_input", error.to_string()))?;
            let value: Value = serde_json::from_str(&input).map_err(|error| {
                KernelFailure::operator("malformed_ir", format!("stdin is not valid JSON: {error}"))
            })?;
            let normalized = normalize_semantic_ir(&value)?;
            println!("{}", canonical_json(&normalized));
            Ok(())
        }
        "erosion-supervisor" => {
            let options = ErosionSupervisorOptions {
                julia: flags.get("julia").unwrap_or_else(|| PathBuf::from("julia")),
                project: flags.require("project")?,
                manifest: flags.require("manifest")?,
                worker_script: flags.require("worker")?,
            };
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            run_erosion_supervisor(&options, &mut input)
        }
        "create-store" => {
            let registry = registry_digest(&flags.require("registry")?)?;
            create_store(&flags.require("store")?, &registry)?;
            println!("{{\"event\":\"created\",\"generation_id\":\"G0\"}}");
            Ok(())
        }
        "run" => run_specimen(&RunOptions {
            store: flags.require("store")?,
            invalid_source: flags.require("invalid-source")?,
            invalid_ir: flags.require("invalid-ir")?,
            repaired_source: flags.require("repaired-source")?,
            repaired_ir: flags.require("repaired-ir")?,
            tampered_source: flags.require("tampered-source")?,
            tampered_ir: flags.require("tampered-ir")?,
            registry: flags.require("registry")?,
            julia: flags.get("julia").unwrap_or_else(|| PathBuf::from("julia")),
            project: flags.require("project")?,
            manifest: flags.require("manifest")?,
            worker_script: flags.require("worker")?,
            solver_image_override: flags.raw("solver-image"),
            stop_after: None,
        }),
        "fail-proposal" => run_specimen(&RunOptions {
            store: flags.require("store")?,
            invalid_source: flags.require("invalid-source")?,
            invalid_ir: flags.require("invalid-ir")?,
            repaired_source: flags.require("repaired-source")?,
            repaired_ir: flags.require("repaired-ir")?,
            tampered_source: flags.require("tampered-source")?,
            tampered_ir: flags.require("tampered-ir")?,
            registry: flags.require("registry")?,
            julia: flags.get("julia").unwrap_or_else(|| PathBuf::from("julia")),
            project: flags.require("project")?,
            manifest: flags.require("manifest")?,
            worker_script: flags.require("worker")?,
            solver_image_override: flags.raw("solver-image"),
            stop_after: Some("repair".into()),
        }),
        "apply-repair" => wge_semantic_kernel::apply_persisted_repair(&RunOptions {
            store: flags.require("store")?,
            invalid_source: flags.require("invalid-source")?,
            invalid_ir: flags.require("invalid-ir")?,
            repaired_source: flags.require("repaired-source")?,
            repaired_ir: flags.require("repaired-ir")?,
            tampered_source: flags.require("tampered-source")?,
            tampered_ir: flags.require("tampered-ir")?,
            registry: flags.require("registry")?,
            julia: flags.get("julia").unwrap_or_else(|| PathBuf::from("julia")),
            project: flags.require("project")?,
            manifest: flags.require("manifest")?,
            worker_script: flags.require("worker")?,
            solver_image_override: None,
            stop_after: None,
        }),
        "submit-equivalent" => wge_semantic_kernel::submit_equivalent(&RunOptions {
            store: flags.require("store")?,
            invalid_source: flags.require("invalid-source")?,
            invalid_ir: flags.require("invalid-ir")?,
            repaired_source: flags.require("source")?,
            repaired_ir: flags.require("ir")?,
            tampered_source: flags.require("source")?,
            tampered_ir: flags.require("ir")?,
            registry: flags.require("registry")?,
            julia: flags.get("julia").unwrap_or_else(|| PathBuf::from("julia")),
            project: flags.require("project")?,
            manifest: flags.require("manifest")?,
            worker_script: flags.require("worker")?,
            solver_image_override: None,
            stop_after: None,
        }),
        "inspect" => wge_semantic_kernel::inspect_store(&flags.require("store")?),
        "certify" => {
            let source = flags.require("source")?;
            let ir = flags.require("ir")?;
            certify_source(&RunOptions {
                store: flags.require("store")?,
                invalid_source: source.clone(),
                invalid_ir: ir.clone(),
                repaired_source: source,
                repaired_ir: ir,
                tampered_source: PathBuf::from("unused"),
                tampered_ir: PathBuf::from("unused"),
                registry: flags.require("registry")?,
                julia: flags.get("julia").unwrap_or_else(|| PathBuf::from("julia")),
                project: flags.require("project")?,
                manifest: flags.require("manifest")?,
                worker_script: flags.require("worker")?,
                solver_image_override: flags.raw("solver-image"),
                stop_after: None,
            })
        }
        "check-ir" => {
            let registry = registry_digest(&flags.require("registry")?)?;
            let source = std::fs::read_to_string(flags.require("source")?)
                .map_err(|error| KernelFailure::operator("unreadable_input", error.to_string()))?;
            let ir = load_ir(&flags.require("ir")?, &source, &registry)?;
            println!(
                "{{\"event\":\"ir_ok\",\"ir_digest\":\"{}\",\"registry_digest\":\"{}\"}}",
                ir.digest, ir.registry_digest
            );
            Ok(())
        }
        "reject-effect" => {
            let effect = flags
                .raw("effect")
                .ok_or_else(|| KernelFailure::operator("usage", "--effect is required"))?;
            reject_author_effect(&effect)?;
            println!("{{\"event\":\"effect_ok\",\"effect\":\"{effect}\"}}");
            Ok(())
        }
        "status" => {
            let generation = current_generation(&flags.require("store")?)?;
            println!("{{\"event\":\"current\",\"generation_id\":\"{generation}\"}}");
            Ok(())
        }
        "verify-current" => {
            let receipt = verify_current(&flags.require("store")?)?;
            println!("{receipt}");
            Ok(())
        }
        "accept-receipt" => {
            let id = flags
                .raw("receipt-id")
                .ok_or_else(|| KernelFailure::operator("usage", "--receipt-id is required"))?;
            accept_receipt(&flags.require("store")?, &id)?;
            println!("{{\"event\":\"receipt_ok\",\"receipt_id\":\"{id}\"}}");
            Ok(())
        }
        "bind-evidence" => {
            bind_evidence(
                &flags.require("store")?,
                &flags
                    .raw("candidate")
                    .ok_or_else(|| KernelFailure::operator("usage", "--candidate is required"))?,
                &flags.raw("evidence-sha").ok_or_else(|| {
                    KernelFailure::operator("usage", "--evidence-sha is required")
                })?,
            )?;
            println!("{{\"event\":\"evidence_ok\"}}");
            Ok(())
        }
        "check-solver" => {
            let claimed = flags
                .raw("solver-image")
                .ok_or_else(|| KernelFailure::operator("usage", "--solver-image is required"))?;
            check_solver_image(&flags.require("store")?, &claimed)?;
            println!("{{\"event\":\"solver_ok\"}}");
            Ok(())
        }
        "poke-worker" => {
            let script = flags.require("worker")?;
            let project = flags.require("project")?;
            let manifest = flags.require("manifest")?;
            let julia = flags.get("julia").unwrap_or_else(|| PathBuf::from("julia"));
            let version = wge_semantic_kernel::julia_version(&julia)?;
            let image = solver_image(&script, &project.join("Project.toml"), &manifest, &version)?;
            let expected = script_digest(&script)?;
            let worker = Worker::spawn(&julia, &project, &script)?;
            if worker.script_sha256 != expected {
                return Err(KernelFailure::provenance(
                    "solver_image_mismatch",
                    "poke worker script hash did not match the kernel",
                ));
            }
            println!(
                "{{\"event\":\"poke\",\"pid\":{},\"solver_image\":\"{image}\"}}",
                worker.pid
            );
            Ok(())
        }
        "probe-hostile" => {
            let script = flags.require("worker")?;
            let project = flags.require("project")?;
            let julia = flags.get("julia").unwrap_or_else(|| PathBuf::from("julia"));
            let mut worker = Worker::spawn(&julia, &project, &script)?;
            let pid = worker.pid;
            for (name, payload) in [
                ("register", r#"{"op":"register","name":"evil"}"#),
                (
                    "eval",
                    r#"{"code":"eval(Meta.parse(\"1+1\"))","footprint":{"x0":0,"x1":1,"y0":0,"y1":1},"lane":{"x0":0,"x1":2,"y0":0,"y1":2},"op":"lane_overlap"}"#,
                ),
                (
                    "measure",
                    r#"{"footprint":{"x0":40,"x1":70,"y0":30,"y1":70},"lane":{"x0":0,"x1":100,"y0":40,"y1":60},"op":"lane_overlap"}"#,
                ),
            ] {
                let (raw, _) = worker.request(payload.as_bytes())?;
                let text = String::from_utf8(raw).unwrap_or_else(|_| "\"bad\"".into());
                println!(
                    "{{\"event\":\"hostile\",\"payload\":\"{name}\",\"pid\":{pid},\"response\":{text}}}"
                );
            }
            Ok(())
        }
        other => Err(KernelFailure::operator(
            "usage",
            format!("unknown command {other}"),
        )),
    }
}

struct Flags {
    values: Vec<(String, String)>,
}

impl Flags {
    fn parse(args: impl Iterator<Item = String>) -> Self {
        let raw: Vec<String> = args.collect();
        let mut values = Vec::new();
        let mut index = 0;
        while index < raw.len() {
            let key = raw[index].trim_start_matches("--").to_owned();
            let value = raw.get(index + 1).cloned().unwrap_or_default();
            values.push((key, value));
            index += 2;
        }
        Self { values }
    }

    fn raw(&self, name: &str) -> Option<String> {
        self.values
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    }

    fn get(&self, name: &str) -> Option<PathBuf> {
        self.raw(name).map(PathBuf::from)
    }

    fn require(&self, name: &str) -> Result<PathBuf, KernelFailure> {
        self.get(name)
            .ok_or_else(|| KernelFailure::operator("usage", format!("--{name} is required")))
    }
}
