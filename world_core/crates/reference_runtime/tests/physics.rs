use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use luxel_reference_runtime::{
    AuthoredLayout, KinematicContactKind, KinematicInput, KinematicStepDisposition, PhysicsWorld,
    REFERENCE_TICK_RATE_HZ, WorldArtifact, build_from_layout_path,
};

static WORLD: OnceLock<WorldArtifact> = OnceLock::new();
static SLOPE_WORLD: OnceLock<WorldArtifact> = OnceLock::new();
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn world() -> &'static WorldArtifact {
    WORLD.get_or_init(|| {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let layout = manifest_dir.join("examples/riverwatch.layout.json");
        let project_root = manifest_dir
            .ancestors()
            .nth(3)
            .expect("Luxel root is an ancestor");
        let julia = std::env::var_os("LUXEL_JULIA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("julia"));
        build_from_layout_path(&layout, &julia, &project_root.join("terrain_lab"))
            .expect("authored layout builds through the real Julia/Rust path")
            .world
    })
}

fn slope_world() -> &'static WorldArtifact {
    SLOPE_WORLD.get_or_init(|| {
        let mut layout: AuthoredLayout =
            serde_json::from_str(include_str!("../examples/riverwatch.layout.json"))
                .expect("authored layout fixture deserializes");
        layout.traversal.maximum_grade = 0.3;
        let directory = std::env::temp_dir().join(format!(
            "luxel-physics-slope-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).expect("test directory is created");
        let input = directory.join("layout.json");
        fs::write(
            &input,
            serde_json::to_vec(&layout).expect("typed layout serializes"),
        )
        .expect("test layout is written");
        let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("Luxel root is an ancestor");
        let julia = std::env::var_os("LUXEL_JULIA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("julia"));
        let built = build_from_layout_path(&input, &julia, &project_root.join("terrain_lab"));
        fs::remove_dir_all(&directory).expect("test directory is removed");
        built
            .expect("lower-grade authored world still builds via Julia and Rust")
            .world
    })
}

fn physics() -> PhysicsWorld<'static> {
    PhysicsWorld::new(world()).expect("built world passes native validation")
}

#[test]
fn known_good_route_motion_is_grounded_replayable_and_byte_deterministic() {
    let physics = physics();
    let mut state = physics.initialize_player().unwrap();
    let start = state.clone();
    let input = KinematicInput {
        expected_tick: 1,
        velocity_xz_mps: [0.0, -1.0],
    };
    let first = physics.step(&state, input).unwrap();
    let second = physics.step(&state, input).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first.body.disposition,
        KinematicStepDisposition::Advanced,
        "known-good control must advance"
    );
    assert_eq!(first.body.fixed_tick_rate_hz, REFERENCE_TICK_RATE_HZ);
    assert_eq!(first.body.next_state.body.tick, 1);
    assert_ne!(
        first.body.next_state.body.position_xyz_m,
        start.body.position_xyz_m
    );
    assert!(first.body.contacts.iter().any(|contact| {
        contact.kind == KinematicContactKind::TerrainGround
            && contact.normal_xyz[1] > 0.0
            && contact.penetration_depth_m == 0.0
    }));
    assert_eq!(
        first.canonical_body_json().unwrap(),
        second.canonical_body_json().unwrap()
    );
    physics
        .validate_step_receipt(&state, input, &first)
        .expect("native replay accepts the exact receipt");
    state = first.body.next_state.clone();
    assert_eq!(
        state.body.position_xyz_m[1],
        physics
            .contacts_at([state.body.position_xyz_m[0], state.body.position_xyz_m[2]])
            .unwrap()[0]
            .position_xyz_m[1]
    );
}

#[test]
fn known_bad_obstacle_penetration_is_rejected_without_moving_the_body() {
    let physics = physics();
    let state = physics.state_at([13.0, 13.0]).unwrap();
    let input = KinematicInput {
        expected_tick: 1,
        velocity_xz_mps: [15.0, 0.0],
    };
    let receipt = physics.step(&state, input).unwrap();
    assert_eq!(receipt.body.disposition, KinematicStepDisposition::Rejected);
    assert_eq!(
        receipt.body.next_state.body.position_xyz_m,
        state.body.position_xyz_m
    );
    assert_eq!(receipt.body.next_state.body.velocity_xz_mps, [0.0, 0.0]);
    assert!(receipt.body.contacts.iter().any(|contact| {
        contact.kind == KinematicContactKind::Obstacle
            && contact.contact_id == "fallen_spire"
            && contact.penetration_depth_m > 0.0
    }));
    physics
        .validate_step_receipt(&state, input, &receipt)
        .expect("rejected step is still deterministically replayable");
}

#[test]
fn known_bad_steep_field_motion_is_rejected_by_julia_grade_semantics() {
    let physics = PhysicsWorld::new(slope_world()).unwrap();
    let layout = &slope_world().body.authored_layout;
    let fields = &slope_world().body.fields;
    let resolution = layout.resolution;
    let spacing_x = layout.width_m / (resolution - 1) as f64;
    let spacing_z = layout.length_m / (resolution - 1) as f64;
    let maximum_grade = layout.traversal.maximum_grade;
    let mut control = None;
    for (cell, grade) in fields.slope_grade.iter().enumerate() {
        if *grade <= maximum_grade {
            continue;
        }
        let row = cell / resolution;
        let column = cell % resolution;
        let position = [
            -layout.width_m / 2.0 + column as f64 * spacing_x,
            layout.length_m / 2.0 - row as f64 * spacing_z,
        ];
        if physics.state_at(position).is_ok() {
            let contacts = physics.contacts_at(position).unwrap();
            if contacts
                .iter()
                .any(|contact| contact.kind == KinematicContactKind::SlopeLimit)
            {
                control = Some(position);
                break;
            }
        }
    }
    let start = control.expect("fixture has a clear high-grade sample for the negative control");
    let state = physics.state_at(start).unwrap();
    let receipt = physics
        .step(
            &state,
            KinematicInput {
                expected_tick: 1,
                velocity_xz_mps: [1.0, 0.0],
            },
        )
        .unwrap();
    assert_eq!(receipt.body.disposition, KinematicStepDisposition::Rejected);
    assert_eq!(
        receipt.body.next_state.body.position_xyz_m,
        state.body.position_xyz_m
    );
    assert!(receipt.body.contacts.iter().any(|contact| {
        contact.kind == KinematicContactKind::SlopeLimit
            && contact.measured_grade.unwrap() > contact.maximum_grade.unwrap()
    }));
}

#[test]
fn malformed_or_stale_inputs_and_forged_state_are_rejected() {
    let physics = physics();
    let state = physics.initialize_player().unwrap();
    let stale = physics
        .step(
            &state,
            KinematicInput {
                expected_tick: 2,
                velocity_xz_mps: [0.0, 0.0],
            },
        )
        .unwrap_err();
    assert_eq!(stale.code, "stale_runtime_step");

    let non_finite = physics
        .step(
            &state,
            KinematicInput {
                expected_tick: 1,
                velocity_xz_mps: [f64::NAN, 0.0],
            },
        )
        .unwrap_err();
    assert_eq!(non_finite.code, "contract_violation");

    let mut forged = state;
    forged.body.position_xyz_m[1] += 1.0;
    assert!(
        physics
            .step(
                &forged,
                KinematicInput {
                    expected_tick: 1,
                    velocity_xz_mps: [0.0, 0.0],
                }
            )
            .is_err()
    );
}

#[test]
fn contact_order_is_stable_and_contact_query_finds_authored_obstacle() {
    let physics = physics();
    let first = physics.contacts_at([17.0, 13.0]).unwrap();
    let second = physics.contacts_at([17.0, 13.0]).unwrap();
    assert_eq!(first, second);
    assert!(first.windows(2).all(|pair| {
        pair[0]
            .sweep_sample
            .cmp(&pair[1].sweep_sample)
            .then_with(|| pair[0].kind.cmp(&pair[1].kind))
            .then_with(|| pair[0].contact_id.cmp(&pair[1].contact_id))
            .then_with(|| {
                pair[0]
                    .position_xyz_m
                    .into_iter()
                    .zip(pair[1].position_xyz_m)
                    .map(|(left, right)| left.total_cmp(&right))
                    .find(|order| !order.is_eq())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .is_le()
    }));
    assert!(first.iter().any(|contact| {
        contact.kind == KinematicContactKind::Obstacle && contact.contact_id == "fallen_spire"
    }));
}
