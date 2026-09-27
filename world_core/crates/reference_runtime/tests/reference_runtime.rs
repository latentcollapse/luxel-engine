use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use sha2::{Digest, Sha256};
use wge_gameplay_contract::GameOutcome;
use wge_reference_runtime::{
    AuthoredLayout, GameplayWorldBinding, TraversalEvidence, TraversalOutcome, VisualEvidence,
    VisualGateStatus, WorldArtifact, build_from_layout_path, validate_gameplay_world_binding,
    validate_layout, validate_traversal_evidence, validate_visual_evidence,
    validate_world_artifact,
};

static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn example_layout() -> AuthoredLayout {
    serde_json::from_str(include_str!("../examples/riverwatch.layout.json"))
        .expect("authored example layout must deserialize")
}

fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "wge-reference-runtime-{label}-{}-{}",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).expect("test-owned temporary directory must be created");
    path
}

fn julia_executable() -> PathBuf {
    std::env::var_os("WGE_JULIA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("julia"))
}

fn terrain_lab_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("crate root has terrain_lab sibling")
        .join("terrain_lab")
}

fn build_layout(
    layout: &AuthoredLayout,
    label: &str,
) -> Result<wge_reference_runtime::WorldBuild, wge_reference_runtime::ReferenceRuntimeError> {
    validate_layout(layout)?;
    let directory = temp_dir(label);
    let input = directory.join("authored.layout.json");
    fs::write(
        &input,
        serde_json::to_vec(layout).expect("typed authored layout serializes"),
    )
    .expect("test layout writes to its owned directory");
    let result = build_from_layout_path(&input, &julia_executable(), &terrain_lab_root());
    fs::remove_dir_all(&directory).expect("test-owned temporary directory is removed");
    result
}

fn digest<T: Serialize>(value: &T) -> String {
    format!(
        "sha256:{}",
        hex(&Sha256::digest(
            serde_json::to_vec(value).expect("evidence serializes")
        ))
    )
}

fn hex(bytes: &[u8]) -> String {
    const TABLE: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(TABLE[(byte >> 4) as usize] as char);
        output.push(TABLE[(byte & 0x0f) as usize] as char);
    }
    output
}

fn reseal_world(world: &mut WorldArtifact) {
    let sha = digest(&world.body);
    world.artifact_id = format!("world-{}", sha.trim_start_matches("sha256:"));
    world.artifact_sha256 = sha;
}

fn reseal_traversal(evidence: &mut TraversalEvidence) {
    evidence.evidence_sha256 = digest(&evidence.body);
}

fn reseal_visual(evidence: &mut VisualEvidence) {
    evidence.evidence_sha256 = digest(&evidence.body);
}

fn reseal_gameplay(binding: &mut GameplayWorldBinding) {
    binding.evidence_sha256 = digest(&binding.body);
}

#[test]
fn fresh_cli_build_replays_gameplay_and_verifies_the_saved_candidate() {
    let first_dir = temp_dir("cli-first");
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/riverwatch.layout.json");
    let binary = env!("CARGO_BIN_EXE_wge-reference-runtime");
    let first = Command::new(binary)
        .arg("build")
        .arg("--layout")
        .arg(&example)
        .arg("--output-dir")
        .arg(&first_dir)
        .output()
        .expect("reference runtime CLI starts");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(report["status"], "passed");
    assert_eq!(report["traversal_outcome"], "completed");
    assert_eq!(report["gameplay_outcome"], "won");
    assert_eq!(report["visual_status"], "passed");

    for filename in [
        "world_artifact.json",
        "traversal_evidence.json",
        "gameplay_world_binding.json",
        "reference_capture.ppm",
        "visual_evidence.json",
        "candidate_report.json",
    ] {
        assert!(first_dir.join(filename).is_file(), "missing {filename}");
    }
    let verification = Command::new(binary)
        .arg("verify")
        .arg("--bundle")
        .arg(&first_dir)
        .output()
        .expect("verification CLI starts");
    assert!(
        verification.status.success(),
        "{}",
        String::from_utf8_lossy(&verification.stdout)
    );
    let verify_report: serde_json::Value = serde_json::from_slice(&verification.stdout).unwrap();
    assert_eq!(verify_report["independent_native_revalidation"], true);

    let world: WorldArtifact =
        serde_json::from_slice(&fs::read(first_dir.join("world_artifact.json")).unwrap()).unwrap();
    let traversal: TraversalEvidence =
        serde_json::from_slice(&fs::read(first_dir.join("traversal_evidence.json")).unwrap())
            .unwrap();
    let gameplay: GameplayWorldBinding =
        serde_json::from_slice(&fs::read(first_dir.join("gameplay_world_binding.json")).unwrap())
            .unwrap();
    let visual: VisualEvidence =
        serde_json::from_slice(&fs::read(first_dir.join("visual_evidence.json")).unwrap()).unwrap();
    let capture = fs::read(first_dir.join("reference_capture.ppm")).unwrap();
    assert_eq!(traversal.body.outcome, TraversalOutcome::Completed);
    assert_eq!(gameplay.body.outcome, GameOutcome::Won);
    assert_eq!(visual.body.status, VisualGateStatus::Passed);
    validate_world_artifact(&world).unwrap();
    validate_traversal_evidence(&world, &traversal).unwrap();
    validate_visual_evidence(&world, &capture, &visual).unwrap();
    validate_gameplay_world_binding(&world, &traversal, &capture, &visual, &gameplay).unwrap();

    // Repeat the complete fresh run. All content/evidence identities must be
    // stable even though a new Julia process and output directory are used.
    let second_dir = temp_dir("cli-second");
    let second = Command::new(binary)
        .arg("build")
        .arg("--layout")
        .arg(&example)
        .arg("--output-dir")
        .arg(&second_dir)
        .output()
        .expect("repeat reference runtime CLI starts");
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stdout)
    );
    let second_report: serde_json::Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(
        report["world_artifact_sha256"],
        second_report["world_artifact_sha256"]
    );
    assert_eq!(
        report["spatial_fields_sha256"],
        second_report["spatial_fields_sha256"]
    );
    assert_eq!(
        report["gameplay_receipt_sha256"],
        second_report["gameplay_receipt_sha256"]
    );
    assert_eq!(
        report["visual_evidence_sha256"],
        second_report["visual_evidence_sha256"]
    );
    for filename in [
        "world_artifact.json",
        "traversal_evidence.json",
        "gameplay_world_binding.json",
        "reference_capture.ppm",
        "visual_evidence.json",
        "candidate_report.json",
    ] {
        assert_eq!(
            fs::read(first_dir.join(filename)).unwrap(),
            fs::read(second_dir.join(filename)).unwrap(),
            "fresh builds must reproduce {filename} byte-for-byte"
        );
    }
    fs::remove_dir_all(first_dir).unwrap();
    fs::remove_dir_all(second_dir).unwrap();
}

#[test]
fn full_width_blocked_region_is_a_known_bad_traversal_control() {
    let mut layout = example_layout();
    layout.regions.push(wge_reference_runtime::SemanticRegion {
        region_id: "cross_map_scree_barrier".into(),
        code: 2,
        priority: 1,
        blocks_traversal: true,
        polygon_xz_m: vec![[-48.0, -1.0], [48.0, -1.0], [48.0, 1.0], [-48.0, 1.0]],
    });
    let error = match build_layout(&layout, "blocked-traversal") {
        Ok(_) => panic!("a world-spanning blocked band must prevent traversal"),
        Err(error) => error,
    };
    assert!(
        error.message.contains("no collision-clear route"),
        "{error}"
    );
}

#[test]
fn flat_visual_failure_is_a_failed_cli_gate_with_nonzero_exit() {
    let directory = temp_dir("flat-visual-cli");
    let layout_path = directory.join("flat.layout.json");
    let output_dir = directory.join("candidate");
    let mut layout = example_layout();
    layout.terrain.features.clear();
    layout.terrain.noise_amplitude_m = 0.0;
    fs::write(&layout_path, serde_json::to_vec(&layout).unwrap()).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_wge-reference-runtime"))
        .arg("build")
        .arg("--layout")
        .arg(&layout_path)
        .arg("--output-dir")
        .arg(&output_dir)
        .output()
        .expect("reference runtime CLI starts");
    assert_eq!(output.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["status"], "failed");
    assert_eq!(report["reference_gates_passed"], false);
    assert_eq!(report["visual_status"], "failed");
    assert!(
        !report["visual_failure_reasons"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(output_dir.join("visual_evidence.json").is_file());
    assert!(output_dir.join("candidate_report.json").is_file());

    let verification = Command::new(env!("CARGO_BIN_EXE_wge-reference-runtime"))
        .arg("verify")
        .arg("--bundle")
        .arg(&output_dir)
        .output()
        .expect("reference runtime verification CLI starts");
    assert_eq!(verification.status.code(), Some(2));
    let verification_report: serde_json::Value =
        serde_json::from_slice(&verification.stdout).unwrap();
    assert_eq!(verification_report["status"], "failed");
    assert_eq!(verification_report["independent_native_revalidation"], true);

    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn flat_world_has_a_real_failed_visual_gate_and_cannot_be_promoted_by_relabeling() {
    let mut layout = example_layout();
    layout.terrain.features.clear();
    layout.terrain.noise_amplitude_m = 0.0;
    let built = build_layout(&layout, "flat-visual-control")
        .expect("flat field remains mechanically traversable and gameplay-completable");
    assert_eq!(built.traversal.body.outcome, TraversalOutcome::Completed);
    assert_eq!(built.gameplay.body.outcome, GameOutcome::Won);
    assert_eq!(built.visual.body.status, VisualGateStatus::Failed);
    assert!(!built.reference_gates_passed());
    assert!(built.visual.body.failure_reasons.iter().any(|reason| {
        reason.contains("vertical relief") || reason.contains("luminance variation")
    }));
    validate_visual_evidence(&built.world, &built.capture_bytes, &built.visual)
        .expect("the failed measurement itself is valid evidence");

    let mut fake_pass = built.visual.clone();
    fake_pass.body.status = VisualGateStatus::Passed;
    fake_pass.body.failure_reasons.clear();
    reseal_visual(&mut fake_pass);
    assert!(validate_visual_evidence(&built.world, &built.capture_bytes, &fake_pass).is_err());
}

#[test]
fn resealed_world_evidence_still_fails_semantic_revalidation() {
    let built = build_layout(&example_layout(), "tamper-controls")
        .expect("known-good authored world compiles through Julia");

    let mut fake_route = built.world.clone();
    fake_route.body.navigation.route_cells.pop();
    reseal_world(&mut fake_route);
    assert!(
        validate_world_artifact(&fake_route)
            .expect_err("resealed route remains semantically stale")
            .message
            .contains("navigation")
    );

    let mut fake_collision = built.world.clone();
    fake_collision.body.collision.obstacles[0].radius_m += 0.25;
    reseal_world(&mut fake_collision);
    assert!(
        validate_world_artifact(&fake_collision)
            .expect_err("collision cannot diverge from the authored layout")
            .message
            .contains("collision")
    );

    let mut fake_height = built.world.clone();
    fake_height.body.fields.heights_m[0] += 0.25;
    reseal_world(&mut fake_height);
    assert!(
        validate_world_artifact(&fake_height)
            .expect_err("altered field bytes cannot retain the Julia receipt")
            .message
            .contains("field")
    );

    let mut forged_traversal = built.traversal.clone();
    forged_traversal.body.steps[1].position_xyz_m[0] += 3.0;
    reseal_traversal(&mut forged_traversal);
    assert!(validate_traversal_evidence(&built.world, &forged_traversal).is_err());

    let mut forged_gameplay = built.gameplay.clone();
    forged_gameplay.body.world_artifact_sha256 = "sha256:".to_owned() + &"0".repeat(64);
    reseal_gameplay(&mut forged_gameplay);
    assert!(
        validate_gameplay_world_binding(
            &built.world,
            &built.traversal,
            &built.capture_bytes,
            &built.visual,
            &forged_gameplay
        )
        .is_err()
    );

    let mut forged_julia_exchange = built.world.clone();
    let mut response: serde_json::Value =
        serde_json::from_str(&forged_julia_exchange.body.julia_provenance.response_json).unwrap();
    let first = response["heights_f64_le_hex"].as_str().unwrap();
    let mut bytes = first.as_bytes().to_vec();
    bytes[0] = if bytes[0] == b'0' { b'1' } else { b'0' };
    response["heights_f64_le_hex"] = String::from_utf8(bytes).unwrap().into();
    let response_bytes = serde_json::to_vec(&response).unwrap();
    forged_julia_exchange.body.julia_provenance.response_json =
        String::from_utf8(response_bytes.clone()).unwrap();
    forged_julia_exchange.body.julia_provenance.response_sha256 =
        format!("sha256:{}", hex(&Sha256::digest(&response_bytes)));
    reseal_world(&mut forged_julia_exchange);
    assert!(validate_world_artifact(&forged_julia_exchange).is_err());

    let mut tampered_capture = built.capture_bytes.clone();
    let last = tampered_capture.len() - 1;
    tampered_capture[last] ^= 1;
    assert!(validate_visual_evidence(&built.world, &tampered_capture, &built.visual).is_err());
}

#[test]
fn status_only_receipts_cannot_deserialize_as_native_evidence() {
    assert!(serde_json::from_str::<TraversalEvidence>(r#"{"status":"passed"}"#).is_err());
    assert!(serde_json::from_str::<VisualEvidence>(r#"{"status":"passed"}"#).is_err());
    assert!(serde_json::from_str::<GameplayWorldBinding>(r#"{"status":"passed"}"#).is_err());
}
