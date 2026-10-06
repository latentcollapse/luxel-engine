use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use luxel_live_evidence_contract::{
    DiagnosticCode, LIVE_FRAME_SCHEMA, LiveEvidenceSession, LiveFrameAttestation,
    LiveFrameTelemetry, PacketIdentity, SampleTrigger, SamplingPolicy, SemanticEvent,
    SemanticEventKind, SessionState,
};
use luxel_native_graphics_contract::{
    GraphicsSessionMode, GraphicsWorkerSupervisor, LiveGraphicsSession, LivePresentOutcome,
    lower_reference_world, lower_showcase_packet,
};
use luxel_reference_runtime::build_from_layout_path;

fn digest(label: &str) -> String {
    use sha2::{Digest, Sha256};
    let bytes = Sha256::digest(label.as_bytes());
    let mut hex = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut hex, "{byte:02x}").expect("writing to String cannot fail");
    }
    format!("sha256:{hex}")
}

fn identity(packet: &str, capabilities: &str) -> PacketIdentity {
    PacketIdentity {
        packet_sha256: digest(packet),
        capabilities_sha256: digest(capabilities),
    }
}

fn evidence_session(packet: PacketIdentity) -> LiveEvidenceSession {
    LiveEvidenceSession::new(
        "project-a",
        "session-a",
        packet,
        SamplingPolicy {
            interval_ms: 100,
            sample_window_ms: 50,
        },
        10,
    )
    .expect("typed live evidence session")
}

fn frame(packet: PacketIdentity, frame_sequence: u64, observed_at_ms: u64) -> LiveFrameAttestation {
    LiveFrameAttestation {
        schema_version: LIVE_FRAME_SCHEMA.into(),
        project_id: "project-a".into(),
        session_id: "session-a".into(),
        packet,
        frame_sequence,
        observed_at_ms,
        telemetry: LiveFrameTelemetry {
            frame_time_us: 2_000,
            gpu_time_us: 1_000,
            draw_calls: 12,
            submitted_instances: 100,
            visible_instances: 80,
            dropped_frames: 0,
        },
    }
}

fn is_indeterminate(session: &LiveEvidenceSession, expected: DiagnosticCode) -> bool {
    matches!(session.state(), SessionState::Indeterminate { diagnostic } if diagnostic.code == expected)
}

#[test]
fn stale_frame_packet_identity_fails_closed() {
    let active = identity("packet-a", "caps-a");
    let stale = identity("packet-old", "caps-a");
    let mut session = evidence_session(active);
    let result = session.record_live_frame(frame(stale, 1, 11));
    assert!(result.is_err());
    assert!(is_indeterminate(
        &session,
        DiagnosticCode::StaleOrMismatchedPacket
    ));
}

#[test]
fn frame_sequence_and_monotonic_time_regressions_fail_closed() {
    let packet = identity("packet-a", "caps-a");
    let mut sequence_session = evidence_session(packet.clone());
    sequence_session
        .record_live_frame(frame(packet.clone(), 7, 20))
        .expect("first frame accepted");
    assert!(
        sequence_session
            .record_live_frame(frame(packet.clone(), 6, 21))
            .is_err()
    );
    assert!(is_indeterminate(
        &sequence_session,
        DiagnosticCode::StaleOrMismatchedPacket
    ));

    let mut clock_session = evidence_session(packet.clone());
    clock_session
        .record_live_frame(frame(packet.clone(), 7, 20))
        .expect("first frame accepted");
    assert!(
        clock_session
            .record_live_frame(frame(packet, 8, 19))
            .is_err()
    );
    assert!(is_indeterminate(
        &clock_session,
        DiagnosticCode::ClockRegression
    ));
}

#[test]
fn sample_requests_rebind_and_missing_or_forged_results_never_certify() {
    let initial = identity("packet-a", "caps-a");
    let changed = identity("packet-b", "caps-a");
    let mut session = evidence_session(initial);
    let request = session
        .tick(110)
        .expect("sample timer advances")
        .expect("periodic sample is due");
    assert_eq!(request.packet, identity("packet-a", "caps-a"));

    let rebound = session
        .observe_event(SemanticEvent {
            sequence: 1,
            occurred_at_ms: 111,
            kind: SemanticEventKind::SceneChanged,
            packet: changed,
        })
        .expect("semantic change rebinds the pending request");
    assert_ne!(request.request_id, rebound.request_id);
    assert!(rebound.triggers.contains(&SampleTrigger::SceneChanged));
    assert_eq!(rebound.packet, identity("packet-b", "caps-a"));
    assert!(session.complete_snapshot(None, 112).is_err());
    assert!(is_indeterminate(&session, DiagnosticCode::MissingSnapshot));

    let mut forged_session = evidence_session(identity("packet-c", "caps-c"));
    forged_session
        .tick(110)
        .expect("sample timer advances")
        .expect("periodic sample is due");
    assert!(
        forged_session
            .complete_snapshot_json(br#"{"status":"pass"}"#, 111)
            .is_err()
    );
    assert!(matches!(
        forged_session.state(),
        SessionState::Demoted { diagnostic }
            if diagnostic.code == DiagnosticCode::ForgedPassStatus
    ));
}

#[test]
fn session_digest_is_deterministic_for_identical_transitions() {
    let make = || {
        let mut session = evidence_session(identity("packet-a", "caps-a"));
        session.tick(110).expect("timer advances");
        session
    };
    assert_eq!(
        make().digest().expect("first digest"),
        make().digest().expect("second digest")
    );
}

fn julia_executable() -> PathBuf {
    std::env::var_os("LUXEL_JULIA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("julia"))
}

fn graphics_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("graphics test lock is not poisoned")
}

#[test]
fn supervised_offscreen_capture_is_bound_into_the_live_session() {
    let _guard = graphics_test_guard();
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace");
    let layout = manifest_dir
        .join("../reference_runtime/examples/riverwatch.layout.json")
        .canonicalize()
        .expect("reference layout exists");
    let graphics_lab = workspace_root.join("graphics_lab");
    let terrain_lab = workspace_root.join("terrain_lab");
    let worker = graphics_lab.join("bin/luxel_graphics_worker.jl");
    let output_dir =
        std::env::temp_dir().join(format!("luxel-live-graphics-contract-{}", std::process::id()));
    fs::create_dir_all(&output_dir).expect("test output directory is writable");
    let input = output_dir.join("riverwatch.layout.json");
    fs::copy(&layout, &input).expect("layout copies into temp fixture directory");

    let build = build_from_layout_path(&input, &julia_executable(), &terrain_lab)
        .expect("Julia-backed reference world builds");
    let packet = lower_reference_world(&build.world).expect("world lowers to scene packet");
    let mut supervisor =
        GraphicsWorkerSupervisor::start(julia_executable(), &graphics_lab, &worker)
            .expect("Rust supervisor starts the Julia graphics worker");
    let mut session = LiveGraphicsSession::new(
        &mut supervisor,
        "riverwatch-project",
        "capture-session-1",
        packet.clone(),
        SamplingPolicy {
            interval_ms: 1_000,
            sample_window_ms: 500,
        },
        100,
    )
    .expect("live session binds exact packet and validated capabilities");

    assert_eq!(session.mode(), GraphicsSessionMode::OffscreenCapture);
    assert!(matches!(
        session.request_live_present(),
        LivePresentOutcome::Unsupported { .. }
    ));
    assert!(session.tick(1_099).expect("tick before deadline").is_none());
    let request = session
        .tick(1_100)
        .expect("periodic request is scheduled")
        .expect("sample is due");

    let changed_packet = lower_showcase_packet(&packet).expect("showcase packet is authorized");
    session
        .synchronize_binding(&mut supervisor, &changed_packet, 1_110)
        .expect("scene change is observed and sample rebound");
    let rebound = session.pending_sample().expect("sample remains pending");
    assert_ne!(request.request_id, rebound.request_id);
    assert_eq!(rebound.packet.packet_sha256, changed_packet.packet_sha256);
    assert!(rebound.triggers.contains(&SampleTrigger::SceneChanged));

    // The first Vulkan frame compiles pipelines and can legitimately exceed
    // the live contract's 1 s Tier B bound. Warm that exact packet before the
    // measured session frame; the adapter still records only the subsequent
    // promoted frame's unmodified telemetry.
    let warmup = supervisor
        .render_and_promote(&changed_packet, &build.world)
        .expect("first packet capture warms the worker pipelines");
    assert_eq!(
        warmup.receipt.body.packet_sha256,
        changed_packet.packet_sha256
    );

    let output = session
        .render_capture_and_record(&mut supervisor, &changed_packet, &build.world, 1, 1_120)
        .expect("Rust promotes the offscreen Lava capture and binds Tier B telemetry");
    assert_eq!(output.attestation.frame_sequence, 1);
    assert_eq!(
        output.attestation.packet.packet_sha256,
        changed_packet.packet_sha256
    );
    assert_eq!(
        output.attestation.telemetry.frame_time_us,
        output.promoted.receipt.body.telemetry.frame_time_us
    );
    assert_eq!(
        output.attestation.telemetry.draw_calls as usize,
        output.promoted.receipt.body.telemetry.draw_calls
    );
    assert_eq!(output.attestation.telemetry.dropped_frames, 0);
    assert!(
        session
            .digest()
            .expect("session digest")
            .starts_with("sha256:")
    );
    assert_eq!(session.mode(), GraphicsSessionMode::OffscreenCapture);

    let _ = fs::remove_dir_all(output_dir);
}
