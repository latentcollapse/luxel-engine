//! Session contract tests: request validation, deterministic camera state,
//! and world-binding fail-closed checks that need no live worker.

use std::path::PathBuf;

use luxel_native_graphics_contract::session::{PresentedGraphicsSession, PresentedSessionRequest};
use luxel_reference_runtime::build_from_layout_path;

fn julia_executable() -> PathBuf {
    std::env::var_os("LUXEL_JULIA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("julia"))
}

fn build_world_fixture() -> luxel_reference_runtime::WorldArtifact {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .ancestors()
        .nth(3)
        .expect("crate is inside the workspace");
    let layout = manifest_dir
        .join("../reference_runtime/examples/riverwatch.layout.json")
        .canonicalize()
        .expect("reference layout exists");
    let terrain_lab = workspace_root.join("terrain_lab");
    let input = std::env::temp_dir().join(format!(
        "luxel-session-fixture-{}-{}",
        std::process::id(),
        std::thread::current().name().unwrap_or("t")
    ));
    std::fs::create_dir_all(&input).expect("fixture dir is writable");
    let layout_path = input.join("riverwatch.layout.json");
    std::fs::copy(&layout, &layout_path).expect("layout copies into fixture dir");
    build_from_layout_path(&layout_path, &julia_executable(), &terrain_lab)
        .expect("Julia-backed reference world builds")
        .world
}

/// One shared world per process: the build spawns a Julia subprocess, and a
/// per-test fixture directory races when parallel test threads copy the same
/// layout file over each other mid-read.
fn world_fixture() -> luxel_reference_runtime::WorldArtifact {
    static WORLD: std::sync::OnceLock<luxel_reference_runtime::WorldArtifact> =
        std::sync::OnceLock::new();
    WORLD.get_or_init(build_world_fixture).clone()
}

fn request() -> PresentedSessionRequest {
    PresentedSessionRequest::new("session-contract", 320, 240, true)
        .expect("typed presented-session request")
}

fn packet_for(
    world: &luxel_reference_runtime::WorldArtifact,
) -> luxel_native_graphics_contract::GraphicsScenePacket {
    luxel_native_graphics_contract::lower_reference_world(world)
        .expect("world lowers to scene packet")
}

#[test]
fn session_request_rejects_bad_identity_and_extent() {
    assert!(PresentedSessionRequest::new("", 320, 240, false).is_err());
    assert!(PresentedSessionRequest::new("x", 0, 240, false).is_err());
    assert!(PresentedSessionRequest::new("x", 320, 0, false).is_err());
}

#[test]
fn session_rejects_camera_extent_mismatch() {
    let world = world_fixture();
    let packet = packet_for(&world);
    let mismatched = PresentedSessionRequest::new(
        "session-contract",
        packet.body.camera.width_px + 1,
        packet.body.camera.height_px,
        true,
    )
    .expect("request with different width is still typed");
    let error = PresentedGraphicsSession::new(&mismatched, &packet, &world)
        .expect_err("camera extent must match the window extent");
    assert_eq!(error.code, "provenance");
}

#[test]
fn session_camera_walking_is_deterministic_and_normalized() {
    let world = world_fixture();
    let packet = packet_for(&world);
    let mut session =
        PresentedGraphicsSession::new(&request(), &packet, &world).expect("typed session");

    let waypoints: Vec<[f32; 3]> = (0..8)
        .map(|step| {
            [
                packet.body.camera.position_xyz_m[0] + step as f32,
                packet.body.camera.position_xyz_m[1],
                packet.body.camera.position_xyz_m[2],
            ]
        })
        .collect();
    session
        .walk_path(&waypoints)
        .expect("deterministic waypoint walk");

    let forward = session.camera_forward();
    let length =
        (forward[0] * forward[0] + forward[1] * forward[1] + forward[2] * forward[2]).sqrt();
    assert!((length - 1.0).abs() < 1e-4, "forward stays normalized");
    assert_eq!(session.tick(), 8);

    let before = session.camera_forward();
    session
        .walk_path(&[waypoints[7]])
        .expect("a repeated waypoint keeps the previous forward");
    assert_eq!(session.camera_forward(), before);

    let make = || {
        let mut other =
            PresentedGraphicsSession::new(&request(), &packet, &world).expect("typed session");
        other.walk_path(&waypoints).expect("walk");
        (
            other.camera_position(),
            other.camera_forward(),
            other.tick(),
        )
    };
    assert_eq!(make(), make(), "identical traces are identical sessions");
}

#[test]
fn session_rejects_nonfinite_or_degenerate_camera_state() {
    let world = world_fixture();
    let packet = packet_for(&world);
    let mut session =
        PresentedGraphicsSession::new(&request(), &packet, &world).expect("typed session");

    assert!(
        session
            .set_camera([f32::NAN, 0.0, 0.0], [0.0, 0.0, 1.0])
            .is_err()
    );
    assert!(
        session
            .set_camera([0.0, 0.0, 0.0], [0.0, 0.0, 0.0])
            .is_err()
    );
}

#[test]
fn session_capture_rejects_a_world_that_no_longer_matches() {
    let world = world_fixture();
    let packet = packet_for(&world);
    let session =
        PresentedGraphicsSession::new(&request(), &packet, &world).expect("typed session");

    session
        .validate_bound_world(&world)
        .expect("the bound world validates");

    let mut forged = world.clone();
    forged.body.collision.agent_radius_m += 0.01;
    let error = session
        .validate_bound_world(&forged)
        .expect_err("a diverged world artifact must fail closed");
    assert_eq!(error.code, "provenance");
}
