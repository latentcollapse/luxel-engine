//! Filesystem store. Create and open are different operations.
//! Open never writes. Generation files are immutable under their ids.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{KernelFailure, canonical_json, ident, ops, sha256_prefixed};

pub const STORE_SCHEMA: &str = "luxel.store/v0";

#[derive(Debug)]
pub struct Opened {
    pub generation_id: String,
    pub generation: Value,
}

pub fn is_initialized(store: &Path) -> bool {
    store.join("store.json").is_file()
        || store.join("pointer.json").is_file()
        || store.join("generations").join("G0.json").is_file()
}

pub fn create_store(store: &Path, registry: &str) -> Result<(), KernelFailure> {
    if is_initialized(store) {
        return Err(KernelFailure::provenance(
            "store_exists",
            "refusing to recreate G0 over an existing store",
        ));
    }
    for name in [
        "generations",
        "candidates",
        "proposals",
        "repairs",
        "evidence",
        "bindings",
        "receipts",
        "history",
    ] {
        fs::create_dir_all(store.join(name))
            .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
    }
    let generation = genesis(registry);
    put_immutable(
        &store.join("generations").join("G0.json"),
        canonical_json(&generation).as_bytes(),
    )?;
    atomic_replace(
        &store.join("store.json"),
        canonical_json(&json!({"registry_digest": registry, "schema": STORE_SCHEMA})).as_bytes(),
    )?;
    replace_pointer(store, "G0")?;
    Ok(())
}

pub fn open_store(store: &Path) -> Result<Opened, KernelFailure> {
    let schema_path = store.join("store.json");
    if !schema_path.is_file() {
        return Err(KernelFailure::provenance(
            "missing_store",
            "no store schema; refusing to invent G0",
        ));
    }
    let schema = read_json(&schema_path)?;
    let version = schema
        .get("schema")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if version != STORE_SCHEMA {
        return Err(KernelFailure::provenance(
            "unsupported_schema",
            format!("store schema {version:?} is not {STORE_SCHEMA}"),
        ));
    }
    let pinned_registry = schema
        .get("registry_digest")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            KernelFailure::provenance("malformed_store", "store.json has no registry_digest")
        })?
        .to_owned();
    let pointer_path = store.join("pointer.json");
    if !pointer_path.is_file() {
        return Err(KernelFailure::provenance(
            "missing_pointer",
            "store has no generation pointer",
        ));
    }
    let pointer = read_json(&pointer_path)?;
    let generation_id = pointer
        .get("generation_id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            KernelFailure::provenance("missing_pointer", "pointer has no generation_id")
        })?
        .to_owned();
    if pointer.get("schema").and_then(|value| value.as_str()) != Some("luxel.pointer/v0") {
        return Err(KernelFailure::provenance(
            "malformed_pointer",
            "pointer schema is not luxel.pointer/v0",
        ));
    }
    let generation_path = store
        .join("generations")
        .join(format!("{generation_id}.json"));
    if !generation_path.is_file() {
        return Err(KernelFailure::provenance(
            "missing_generation",
            format!("pointer names {generation_id} but that generation file is absent"),
        ));
    }
    let raw = fs::read(&generation_path)
        .map_err(|error| KernelFailure::provenance("malformed_store", error.to_string()))?;
    let generation = parse_json(&raw, &generation_path)?;
    let recorded = generation
        .get("generation_id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if recorded != generation_id {
        return Err(KernelFailure::provenance(
            "identity_conflict",
            format!("generation file {generation_id} records id {recorded}"),
        ));
    }
    validate_generation_identity(&generation, &pinned_registry)?;
    require_canonical_generation(&raw, &generation, &pinned_registry)?;
    if generation_id != "G0" {
        let parent = required_str(&generation, "parent_id")?;
        let parent_path = store.join("generations").join(format!("{parent}.json"));
        if !parent_path.is_file() {
            return Err(KernelFailure::provenance(
                "missing_generation",
                format!("parent generation {parent} is not in the store"),
            ));
        }
        let parent_raw = fs::read(&parent_path)
            .map_err(|error| KernelFailure::provenance("malformed_store", error.to_string()))?;
        let parent_doc = parse_json(&parent_raw, &parent_path)?;
        if parent_doc
            .get("generation_id")
            .and_then(|value| value.as_str())
            != Some(parent)
        {
            return Err(KernelFailure::provenance(
                "identity_conflict",
                format!("parent file {parent} records a different id"),
            ));
        }
        validate_generation_identity(&parent_doc, &pinned_registry)?;
        require_canonical_generation(&parent_raw, &parent_doc, &pinned_registry)?;
        let evidence = certified_index(&generation, "evidence_index", ops::is_registered_gate)?;
        let receipts = receipt_map(&generation, &evidence)?;
        for (gate, measurement) in &evidence {
            let receipt_id = receipts.get(gate).ok_or_else(|| {
                KernelFailure::provenance("missing_receipt", format!("no receipt for gate {gate}"))
            })?;
            let receipt_path = store.join("receipts").join(format!("{receipt_id}.json"));
            if !receipt_path.is_file() {
                return Err(KernelFailure::provenance(
                    "missing_receipt",
                    "commit receipt file is absent",
                ));
            }
            let receipt_raw = fs::read(&receipt_path)
                .map_err(|error| KernelFailure::provenance("malformed_store", error.to_string()))?;
            let receipt = parse_json(&receipt_raw, &receipt_path)?;
            assert_receipt(&generation, &receipt, &receipt_raw, measurement, gate)?;
            let blob = evidence_path(store, measurement);
            let bytes = fs::read(&blob).map_err(|_| {
                KernelFailure::provenance(
                    "missing_evidence",
                    format!("evidence blob for {measurement} is absent"),
                )
            })?;
            if sha256_prefixed(&bytes) != *measurement {
                return Err(KernelFailure::provenance(
                    "evidence_hash_mismatch",
                    "evidence blob bytes do not match the generation measurement hash",
                ));
            }
        }
    }
    Ok(Opened {
        generation_id,
        generation,
    })
}

pub fn validate_generation_identity(
    generation: &Value,
    pinned_registry: &str,
) -> Result<(), KernelFailure> {
    let generation_id = required_str(generation, "generation_id")?;
    if required_str(generation, "registry_digest")? != pinned_registry {
        return Err(KernelFailure::provenance(
            "identity_conflict",
            "generation registry_digest does not match the store pin",
        ));
    }
    if generation_id == "G0" {
        if generation.get("parent_id").map(|value| value.is_null()) != Some(true) {
            return Err(KernelFailure::provenance(
                "identity_conflict",
                "G0 parent must be null",
            ));
        }
        if generation.get("ir_digest").map(|value| value.is_null()) != Some(true) {
            return Err(KernelFailure::provenance(
                "identity_conflict",
                "G0 has no semantic IR",
            ));
        }
        return Ok(());
    }
    let parent = required_str(generation, "parent_id")?;
    let ir = required_str(generation, "ir_digest")?;
    let solver = required_str(generation, "solver_image")?;
    let registry = required_str(generation, "registry_digest")?;
    let artifacts = certified_index(generation, "artifact_index", ops::is_registered_operation)?;
    let evidence = certified_index(generation, "evidence_index", ops::is_registered_gate)?;
    let expected =
        if let Some(measurement) = v0_single_measurement(generation, &artifacts, &evidence) {
            semantic_generation_id(parent, ir, measurement, solver, registry)
        } else {
            semantic_generation_id_multi(parent, ir, solver, registry, &artifacts, &evidence)
        };
    if expected != generation_id {
        return Err(KernelFailure::provenance(
            "identity_conflict",
            format!("generation {generation_id} does not match semantic identity {expected}"),
        ));
    }
    Ok(())
}

pub fn semantic_generation_id(
    parent: &str,
    ir: &str,
    measurement: &str,
    solver: &str,
    registry: &str,
) -> String {
    ident(
        "G",
        &format!("{parent}\n{ir}\n{measurement}\n{solver}\n{registry}"),
    )
}

pub fn semantic_generation_id_multi(
    parent: &str,
    ir: &str,
    solver: &str,
    registry: &str,
    artifacts: &std::collections::BTreeMap<String, String>,
    evidence: &std::collections::BTreeMap<String, String>,
) -> String {
    ident(
        "G",
        &format!(
            "{parent}\n{ir}\n{solver}\n{registry}\n{}\n{}",
            canonical_json(&index_value(artifacts)),
            canonical_json(&index_value(evidence))
        ),
    )
}

fn index_value(index: &std::collections::BTreeMap<String, String>) -> Value {
    Value::Object(
        index
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect(),
    )
}

fn certified_index(
    generation: &Value,
    key: &str,
    allowed: fn(&str) -> bool,
) -> Result<std::collections::BTreeMap<String, String>, KernelFailure> {
    let object = generation
        .get(key)
        .and_then(|value| value.as_object())
        .ok_or_else(|| {
            KernelFailure::provenance("identity_conflict", format!("generation.{key} is missing"))
        })?;
    if object.is_empty() {
        return Err(KernelFailure::provenance(
            "identity_conflict",
            format!("generation.{key} must name at least one certified output"),
        ));
    }
    let mut index = std::collections::BTreeMap::new();
    for (name, value) in object {
        if !allowed(name) {
            return Err(KernelFailure::provenance(
                "identity_conflict",
                format!("{name} is not a registered {key} id"),
            ));
        }
        let hash = value.as_str().ok_or_else(|| {
            KernelFailure::provenance(
                "identity_conflict",
                format!("generation.{key}.{name} must be a hash"),
            )
        })?;
        index.insert(name.clone(), hash.to_owned());
    }
    Ok(index)
}

fn v0_single_measurement<'a>(
    generation: &'a Value,
    artifacts: &std::collections::BTreeMap<String, String>,
    evidence: &std::collections::BTreeMap<String, String>,
) -> Option<&'a str> {
    if artifacts.len() != 1 || evidence.len() != 1 {
        return None;
    }
    let measurement = generation
        .get("measurement_sha256")
        .and_then(|value| value.as_str())?;
    if artifacts.values().next().map(String::as_str) != Some(measurement) {
        return None;
    }
    if evidence.values().next().map(String::as_str) != Some(measurement) {
        return None;
    }
    Some(measurement)
}

fn receipt_map(
    generation: &Value,
    evidence: &std::collections::BTreeMap<String, String>,
) -> Result<std::collections::BTreeMap<String, String>, KernelFailure> {
    if let Some(single) = generation
        .get("commit_receipt_id")
        .and_then(|value| value.as_str())
        && evidence.len() == 1
    {
        let mut map = std::collections::BTreeMap::new();
        map.insert(evidence.keys().next().unwrap().clone(), single.to_owned());
        return Ok(map);
    }
    let object = generation
        .get("receipt_index")
        .and_then(|value| value.as_object())
        .ok_or_else(|| {
            KernelFailure::provenance("missing_receipt", "generation has no receipt_index")
        })?;
    let mut map = std::collections::BTreeMap::new();
    for gate in evidence.keys() {
        let receipt = object
            .get(gate)
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                KernelFailure::provenance("missing_receipt", format!("no receipt for gate {gate}"))
            })?;
        map.insert(gate.clone(), receipt.to_owned());
    }
    Ok(map)
}

pub fn receipt_id_for(
    generation_id: &str,
    measurement: &str,
    gate: &str,
    solver: &str,
    registry: &str,
) -> String {
    ident(
        "R",
        &format!("{generation_id}\n{measurement}\n{gate}\n{solver}\n{registry}"),
    )
}

fn require_canonical_generation(
    raw: &[u8],
    generation: &Value,
    pinned_registry: &str,
) -> Result<(), KernelFailure> {
    let expected = expected_generation_body(generation, pinned_registry)?;
    let generation_id = required_str(generation, "generation_id")?;
    if !bytes_match_canonical(raw, &canonical_json(&expected)) {
        return Err(KernelFailure::provenance(
            "identity_conflict",
            format!(
                "generation {generation_id} bytes are not the one canonical record for that id"
            ),
        ));
    }
    Ok(())
}

fn expected_generation_body(
    generation: &Value,
    pinned_registry: &str,
) -> Result<Value, KernelFailure> {
    let generation_id = required_str(generation, "generation_id")?;
    if generation_id == "G0" {
        return Ok(genesis(pinned_registry));
    }
    let solver = required_str(generation, "solver_image")?;
    let registry = pinned_registry;
    let artifacts = certified_index(generation, "artifact_index", ops::is_registered_operation)?;
    let evidence = certified_index(generation, "evidence_index", ops::is_registered_gate)?;
    let receipts = receipt_map(generation, &evidence)?;
    if let Some(measurement) = v0_single_measurement(generation, &artifacts, &evidence) {
        let gate = evidence.keys().next().unwrap();
        let receipt_id = receipt_id_for(generation_id, measurement, gate, solver, registry);
        return Ok(json!({
            "artifact_index": index_value(&artifacts),
            "commit_receipt_id": receipt_id,
            "determinism": "exact",
            "evidence_index": index_value(&evidence),
            "generation_id": generation_id,
            "ir_digest": required_str(generation, "ir_digest")?,
            "measurement_sha256": measurement,
            "parent_id": required_str(generation, "parent_id")?,
            "registry_digest": registry,
            "schema": "luxel.generation/v0",
            "seed": null,
            "solver_image": solver
        }));
    }
    let mut receipt_index = serde_json::Map::new();
    for (gate, measurement) in &evidence {
        let receipt_id = receipts.get(gate).ok_or_else(|| {
            KernelFailure::provenance("missing_receipt", format!("no receipt for gate {gate}"))
        })?;
        let expected_id = receipt_id_for(generation_id, measurement, gate, solver, registry);
        if receipt_id != &expected_id {
            return Err(KernelFailure::provenance(
                "forged_receipt",
                "receipt index does not match the decision",
            ));
        }
        receipt_index.insert(gate.clone(), Value::String(receipt_id.clone()));
    }
    Ok(json!({
        "artifact_index": index_value(&artifacts),
        "commit_receipt_id": null,
        "determinism": "exact",
        "evidence_index": index_value(&evidence),
        "generation_id": generation_id,
        "ir_digest": required_str(generation, "ir_digest")?,
        "measurement_sha256": null,
        "parent_id": required_str(generation, "parent_id")?,
        "receipt_index": receipt_index,
        "registry_digest": registry,
        "schema": "luxel.generation/v0",
        "seed": null,
        "solver_image": solver
    }))
}

fn assert_receipt(
    generation: &Value,
    receipt: &Value,
    raw: &[u8],
    measurement: &str,
    gate: &str,
) -> Result<(), KernelFailure> {
    let generation_id = required_str(generation, "generation_id")?;
    let solver = required_str(generation, "solver_image")?;
    let registry = required_str(generation, "registry_digest")?;
    let parent = required_str(generation, "parent_id")?;
    let ir = required_str(generation, "ir_digest")?;
    let receipt_id = receipt_id_for(generation_id, measurement, gate, solver, registry);
    let listed = generation
        .get("commit_receipt_id")
        .and_then(|value| value.as_str())
        == Some(receipt_id.as_str())
        || generation
            .get("receipt_index")
            .and_then(|value| value.get(gate))
            .and_then(|value| value.as_str())
            == Some(receipt_id.as_str());
    if !listed {
        return Err(KernelFailure::provenance(
            "forged_receipt",
            "commit receipt id does not match the decision",
        ));
    }
    let expected = json!({
        "gate_id": gate,
        "gate_result": "pass",
        "generation_id": generation_id,
        "ir_digest": ir,
        "measurement_sha256": measurement,
        "minted_by": "rust-kernel",
        "operation": "generation.commit",
        "parent_id": parent,
        "receipt_id": receipt_id,
        "registry_digest": registry,
        "schema": "luxel.receipt/v0",
        "solver_image": solver
    });
    if !bytes_match_canonical(raw, &canonical_json(&expected))
        || canonical_json(receipt) != canonical_json(&expected)
    {
        return Err(KernelFailure::provenance(
            "forged_receipt",
            "receipt bytes are not the canonical decision for this generation",
        ));
    }
    Ok(())
}

fn bytes_match_canonical(raw: &[u8], canonical: &str) -> bool {
    raw == canonical.as_bytes() || raw == format!("{canonical}\n").as_bytes()
}

fn parse_json(raw: &[u8], path: &Path) -> Result<Value, KernelFailure> {
    serde_json::from_slice(raw).map_err(|error| {
        KernelFailure::provenance("malformed_store", format!("{}: {error}", path.display()))
    })
}

pub fn genesis(registry: &str) -> Value {
    json!({
        "artifact_index": {},
        "commit_receipt_id": null,
        "determinism": "exact",
        "evidence_index": {},
        "generation_id": "G0",
        "ir_digest": null,
        "measurement_sha256": null,
        "parent_id": null,
        "registry_digest": registry,
        "schema": "luxel.generation/v0",
        "seed": null,
        "solver_image": null
    })
}

pub fn evidence_path(store: &Path, measurement_sha: &str) -> PathBuf {
    let name = measurement_sha.trim_start_matches("sha256:");
    store.join("evidence").join(format!("{name}.bin"))
}

pub fn put_blob(store: &Path, measurement_sha: &str, bytes: &[u8]) -> Result<(), KernelFailure> {
    let actual = sha256_prefixed(bytes);
    if actual != measurement_sha {
        return Err(KernelFailure::provenance(
            "evidence_hash_mismatch",
            "refusing to store bytes under a hash the kernel did not compute",
        ));
    }
    put_immutable(&evidence_path(store, measurement_sha), bytes)
}

pub fn put_immutable(path: &Path, bytes: &[u8]) -> Result<(), KernelFailure> {
    if path.is_file() {
        let existing = fs::read(path)
            .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
        if existing == bytes {
            return Ok(());
        }
        return Err(KernelFailure::provenance(
            "identity_conflict",
            format!("{} already holds different bytes", path.display()),
        ));
    }
    atomic_replace(path, bytes)
}

pub fn replace_pointer(store: &Path, generation_id: &str) -> Result<(), KernelFailure> {
    let target = store
        .join("generations")
        .join(format!("{generation_id}.json"));
    if !target.is_file() {
        return Err(KernelFailure::provenance(
            "missing_generation",
            format!("refusing to point at missing generation {generation_id}"),
        ));
    }
    atomic_replace(
        &store.join("pointer.json"),
        canonical_json(&json!({
            "generation_id": generation_id,
            "schema": "luxel.pointer/v0"
        }))
        .as_bytes(),
    )
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), KernelFailure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)
            .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
        file.write_all(bytes)
            .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
        file.sync_all()
            .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
    }
    fs::rename(&tmp, path)
        .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
    Ok(())
}

fn read_json(path: &Path) -> Result<Value, KernelFailure> {
    let text = fs::read_to_string(path).map_err(|error| {
        KernelFailure::provenance("malformed_store", format!("{}: {error}", path.display()))
    })?;
    serde_json::from_str(&text).map_err(|error| {
        KernelFailure::provenance("malformed_store", format!("{}: {error}", path.display()))
    })
}

fn required_str<'a>(value: &'a Value, key: &str) -> Result<&'a str, KernelFailure> {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .ok_or_else(|| {
            KernelFailure::provenance("identity_conflict", format!("generation.{key} is missing"))
        })
}
