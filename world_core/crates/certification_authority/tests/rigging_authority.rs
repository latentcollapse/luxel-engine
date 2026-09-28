use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use wge_asset_contract::{AssetPreparationRequest, inspect_glb, prepare_asset};
use wge_certification_authority::schema::RiggingReceiptPayload;
use wge_certification_authority::{
    ArtifactBytes, CandidateContext, EvidenceBinding, ReceiptEnvelope, ReceiptStatus,
    ValidatorRegistry, candidate_identity, native_mvp_gate_profile, sha256_prefixed,
    validate_receipt,
};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf()
}

fn good_request() -> AssetPreparationRequest {
    let fixture =
        fs::read(project_root().join("tests/fixtures/asset_runtime/good_character.json")).unwrap();
    let fixture: Value = serde_json::from_slice(&fixture).unwrap();
    serde_json::from_value(fixture["request"].clone()).unwrap()
}

/// Minimal deterministic skinned-character GLB matching the in-repo
/// `good_character` runtime-preparation control.
fn good_glb() -> Vec<u8> {
    let mut binary = Vec::new();
    for point in [
        [0.0f32, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 2.0, 0.0],
        [0.0, 0.0, 1.0],
    ] {
        for value in point {
            binary.extend_from_slice(&value.to_le_bytes());
        }
    }
    for index in [0u16, 2, 1, 0, 1, 3, 0, 3, 2, 1, 2, 3] {
        binary.extend_from_slice(&index.to_le_bytes());
    }
    for _ in 0..4 {
        for joint in [0u16, 1, 0, 0] {
            binary.extend_from_slice(&joint.to_le_bytes());
        }
    }
    for _ in 0..4 {
        for weight in [0.75f32, 0.25, 0.0, 0.0] {
            binary.extend_from_slice(&weight.to_le_bytes());
        }
    }
    let identity = [
        1.0f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    for _ in 0..2 {
        for value in identity {
            binary.extend_from_slice(&value.to_le_bytes());
        }
    }
    for time in [0.0f32, 1.0] {
        binary.extend_from_slice(&time.to_le_bytes());
    }
    for _ in 0..2 {
        for value in [0.0f32, 0.0, 0.0, 1.0] {
            binary.extend_from_slice(&value.to_le_bytes());
        }
    }
    for value in [0.0f32, 0.0, 0.0, 0.0, 0.0, 0.25] {
        binary.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.70710677, 0.70710677] {
        binary.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(binary.len(), 392);

    let primitive =
        json!({"attributes":{"POSITION":0,"JOINTS_0":2,"WEIGHTS_0":3},"indices":1,"mode":4});
    let document = json!({
        "asset":{"version":"2.0","generator":"wge-certification-control"},"scene":0,
        "scenes":[{"nodes":[0,3,4]}],
        "nodes":[{"name":"root","children":[1]},{"name":"hand_r","children":[2]},
          {"name":"weapon_mount","extras":{"wge_socket":true}},
          {"name":"body_lod0_node","mesh":0,"skin":0},{"name":"body_lod1_node","mesh":1,"skin":0}],
        "meshes":[{"name":"body_lod0","primitives":[primitive]},{"name":"body_lod1","primitives":[primitive]}],
        "skins":[{"name":"humanoid","inverseBindMatrices":4,"skeleton":0,"joints":[0,1]}],
        "animations":[
          {"name":"idle","samplers":[{"input":5,"output":6,"interpolation":"LINEAR"}],"channels":[{"sampler":0,"target":{"node":1,"path":"rotation"}}]},
          {"name":"locomotion","samplers":[{"input":5,"output":7,"interpolation":"LINEAR"}],"channels":[{"sampler":0,"target":{"node":1,"path":"translation"}}]},
          {"name":"attack","samplers":[{"input":5,"output":8,"interpolation":"LINEAR"}],"channels":[{"sampler":0,"target":{"node":1,"path":"rotation"}}]}],
        "buffers":[{"byteLength":392}],
        "bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":48},{"buffer":0,"byteOffset":48,"byteLength":24},{"buffer":0,"byteOffset":72,"byteLength":32},{"buffer":0,"byteOffset":104,"byteLength":64},{"buffer":0,"byteOffset":168,"byteLength":128},{"buffer":0,"byteOffset":296,"byteLength":8},{"buffer":0,"byteOffset":304,"byteLength":32},{"buffer":0,"byteOffset":336,"byteLength":24},{"buffer":0,"byteOffset":360,"byteLength":32}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3","min":[0,0,0],"max":[1,2,1]},{"bufferView":1,"componentType":5123,"count":12,"type":"SCALAR"},{"bufferView":2,"componentType":5123,"count":4,"type":"VEC4"},{"bufferView":3,"componentType":5126,"count":4,"type":"VEC4"},{"bufferView":4,"componentType":5126,"count":2,"type":"MAT4"},{"bufferView":5,"componentType":5126,"count":2,"type":"SCALAR","min":[0],"max":[1]},{"bufferView":6,"componentType":5126,"count":2,"type":"VEC4"},{"bufferView":7,"componentType":5126,"count":2,"type":"VEC3"},{"bufferView":8,"componentType":5126,"count":2,"type":"VEC4"}]
    });
    let mut document = serde_json::to_vec(&document).unwrap();
    document.resize((document.len() + 3) & !3, b' ');
    binary.resize((binary.len() + 3) & !3, 0);
    let total = 12 + 8 + document.len() + 8 + binary.len();
    let mut glb = Vec::with_capacity(total);
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total as u32).to_le_bytes());
    glb.extend_from_slice(&(document.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4e4f_534au32.to_le_bytes());
    glb.extend_from_slice(&document);
    glb.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004e_4942u32.to_le_bytes());
    glb.extend_from_slice(&binary);
    glb
}

fn candidate(
    glb: Vec<u8>,
    request: &AssetPreparationRequest,
    receipt_override: Option<Vec<u8>>,
) -> CandidateContext {
    let mut request = request.clone();
    request.expected_source_sha256 = Some(inspect_glb(&glb).unwrap().identity.source_sha256);
    let request_bytes = serde_json::to_vec(&request).unwrap();
    let receipt_bytes = receipt_override
        .unwrap_or_else(|| serde_json::to_vec(&prepare_asset(&glb, &request).unwrap()).unwrap());
    let artifacts = BTreeMap::from([
        (
            "hero-glb".into(),
            ArtifactBytes {
                kind: "rigging_glb".into(),
                bytes: glb,
            },
        ),
        (
            "hero-request".into(),
            ArtifactBytes {
                kind: "rigging_request".into(),
                bytes: request_bytes,
            },
        ),
        (
            "hero-preparation".into(),
            ArtifactBytes {
                kind: "rigging_preparation_receipt".into(),
                bytes: receipt_bytes,
            },
        ),
    ]);
    let mut candidate = CandidateContext {
        project_id: "rigging-control-project".into(),
        snapshot_id: "rigging-control-snapshot".into(),
        candidate_sha256: String::new(),
        artifacts,
        authorized_repair_artifact_ids: BTreeSet::new(),
    };
    candidate.candidate_sha256 = candidate_identity(&candidate).unwrap();
    candidate
}

fn envelope(
    candidate: &CandidateContext,
    payload: Value,
    status: ReceiptStatus,
) -> ReceiptEnvelope {
    let registry = ValidatorRegistry::wge_native_mvp_v1();
    let descriptor = registry.descriptor("wge.validator.rigging-runtime-preparation/v1");
    let descriptor = descriptor.unwrap().clone();
    let evidence = ["hero-glb", "hero-request", "hero-preparation"]
        .into_iter()
        .map(|id| {
            let artifact = &candidate.artifacts[id];
            EvidenceBinding {
                artifact_id: id.into(),
                kind: artifact.kind.clone(),
                sha256: sha256_prefixed(&artifact.bytes),
            }
        })
        .collect();
    let mut receipt = ReceiptEnvelope {
        schema_version: "wge.certification-receipt-envelope/v1".into(),
        receipt_id: String::new(),
        project_id: candidate.project_id.clone(),
        snapshot_id: candidate.snapshot_id.clone(),
        candidate_sha256: candidate.candidate_sha256.clone(),
        gate_id: descriptor.gate_id,
        validator_id: descriptor.validator_id,
        receipt_schema: descriptor.receipt_schema,
        status,
        producer: "untrusted-test-producer".into(),
        observed_input_sha256: String::new(),
        evidence,
        payload,
    };
    receipt.seal().unwrap();
    receipt
}

fn payload() -> Value {
    serde_json::to_value(RiggingReceiptPayload {
        source_glb_artifact_id: "hero-glb".into(),
        preparation_request_artifact_id: "hero-request".into(),
        preparation_receipt_artifact_id: "hero-preparation".into(),
    })
    .unwrap()
}

#[test]
fn native_mvp_profile_promotes_rigging_while_engine_neutral_keeps_it_deferred() {
    use wge_certification_authority::{DEFERRED_GATES, engine_neutral_gate_profile};
    let engine_neutral = engine_neutral_gate_profile();
    let old_rigging = engine_neutral
        .iter()
        .find(|gate| gate.gate_id == "rigging")
        .unwrap();
    assert_eq!(
        old_rigging.disposition,
        wge_certification_authority::GateDisposition::DeferredIndeterminate
    );
    assert!(DEFERRED_GATES.contains(&"rigging"));
    let native = native_mvp_gate_profile();
    let rigging = native
        .iter()
        .find(|gate| gate.gate_id == "rigging")
        .unwrap();
    assert_eq!(
        rigging.disposition,
        wge_certification_authority::GateDisposition::RequiredPass
    );
    assert_eq!(
        rigging.validator_id,
        "wge.validator.rigging-runtime-preparation/v1"
    );
}

#[test]
fn native_rigging_validator_recomputes_candidate_bound_preparation() {
    let request = good_request();
    let glb = good_glb();
    let candidate = candidate(glb, &request, None);
    let receipt = envelope(&candidate, payload(), ReceiptStatus::Pass);
    let result = validate_receipt(
        &receipt,
        &candidate,
        &ValidatorRegistry::wge_native_mvp_v1(),
    )
    .unwrap();
    assert_eq!(result.status, ReceiptStatus::Pass);
    assert!(result.detail.contains("2 joints, 3 clips, 1 sockets"));
}

#[test]
fn native_rigging_gate_derives_failure_from_a_known_bad_joint_index_control() {
    let request = good_request();
    let mut glb = good_glb();
    let json_length = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
    let binary_start = 20 + json_length + 8;
    glb[binary_start + 72..binary_start + 74].copy_from_slice(&9u16.to_le_bytes());
    let candidate = candidate(glb, &request, None);
    let receipt = envelope(&candidate, payload(), ReceiptStatus::Fail);
    let verdict = validate_receipt(
        &receipt,
        &candidate,
        &ValidatorRegistry::wge_native_mvp_v1(),
    )
    .unwrap();
    assert_eq!(verdict.status, ReceiptStatus::Fail);
    assert!(
        verdict
            .detail
            .contains("independently revalidated findings")
    );
}

#[test]
fn native_rigging_rejects_status_only_forged_and_stale_preparation_receipts() {
    let request = good_request();
    let glb = good_glb();
    let registry = ValidatorRegistry::wge_native_mvp_v1();

    let mut status_only = candidate(glb.clone(), &request, None);
    status_only
        .artifacts
        .get_mut("hero-preparation")
        .unwrap()
        .bytes = br#"{"status":"ready"}"#.to_vec();
    status_only.candidate_sha256 = candidate_identity(&status_only).unwrap();
    let receipt = envelope(&status_only, payload(), ReceiptStatus::Pass);
    assert!(validate_receipt(&receipt, &status_only, &registry).is_err());

    let good = candidate(glb.clone(), &request, None);
    let mut forged: Value =
        serde_json::from_slice(&good.artifacts["hero-preparation"].bytes).unwrap();
    forged["package"]["package_id"] = json!("runtime_asset_sha256_forged");
    let forged_candidate = candidate(
        glb.clone(),
        &request,
        Some(serde_json::to_vec(&forged).unwrap()),
    );
    let forged_receipt = envelope(&forged_candidate, payload(), ReceiptStatus::Pass);
    assert!(
        validate_receipt(&forged_receipt, &forged_candidate, &registry)
            .unwrap_err()
            .detail
            .contains("differs from independent")
    );

    let stale_bytes = good.artifacts["hero-preparation"].bytes.clone();
    let mut changed_glb = glb;
    let final_byte = changed_glb.last_mut().unwrap();
    *final_byte ^= 1;
    let stale_candidate = candidate(changed_glb, &request, Some(stale_bytes));
    let stale_receipt = envelope(&stale_candidate, payload(), ReceiptStatus::Pass);
    assert!(validate_receipt(&stale_receipt, &stale_candidate, &registry).is_err());
}
