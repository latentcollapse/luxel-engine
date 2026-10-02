use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use wge_reference_runtime::{
    AuthoredLayout, TraversalOutcome, TraversalSession, TraversalSessionInput, WorldArtifact,
    build_from_layout_path, run_playthrough, validate_traversal_session_completion,
};

static WORLD: OnceLock<WorldArtifact> = OnceLock::new();
static SLOPE_WORLD: OnceLock<WorldArtifact> = OnceLock::new();
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

fn world() -> &'static WorldArtifact {
    WORLD.get_or_init(|| build_fixture_world(example_layout(), "riverwatch"))
}

fn slope_world() -> &'static WorldArtifact {
    SLOPE_WORLD.get_or_init(|| {
        let mut layout = example_layout();
        layout.traversal.maximum_grade = 0.3;
        build_fixture_world(layout, "riverwatch-slope-control")
    })
}

fn example_layout() -> AuthoredLayout {
    serde_json::from_str(include_str!("../examples/riverwatch.layout.json"))
        .expect("checked-in layout fixture deserializes")
}

fn build_fixture_world(layout: AuthoredLayout, label: &str) -> WorldArtifact {
    let directory = std::env::temp_dir().join(format!(
        "wge-live-runtime-{label}-{}-{}",
        std::process::id(),
        TEMP_ID.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&directory).expect("test directory is created");
    let input = directory.join("layout.json");
    fs::write(
        &input,
        serde_json::to_vec(&layout).expect("layout serializes"),
    )
    .expect("test layout is written");
    let julia = std::env::var_os("WGE_JULIA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("julia"));
    let project_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("WGE root is an ancestor");
    let result = build_from_layout_path(&input, &julia, &project_root.join("terrain_lab"))
        .expect("Julia/Rust world fixture builds");
    fs::remove_dir_all(&directory).expect("test directory is removed");
    result.world
}

fn drive_authored_route(session: &mut TraversalSession, world: &WorldArtifact, from: usize) {
    for (tick, cell) in world
        .body
        .navigation
        .route_cells
        .iter()
        .copied()
        .enumerate()
        .skip(from)
    {
        session
            .advance(TraversalSessionInput {
                expected_tick: tick as u64,
                destination_cell: cell,
            })
            .expect("authored route step is valid");
    }
}

fn deterministic_result() -> (
    wge_reference_runtime::TraversalSessionSnapshot,
    wge_reference_runtime::TraversalSessionCompletion,
    Vec<wge_reference_runtime::TraversalStep>,
) {
    let world = world();
    let mut session = TraversalSession::initialize(world).expect("session initializes");
    drive_authored_route(&mut session, world, 1);
    let completion = session.finish().expect("route can be completed");
    (
        session.snapshot().expect("state snapshot serializes"),
        completion,
        session.steps().to_vec(),
    )
}

#[test]
fn live_session_replay_matches_batch_traversal_and_is_byte_deterministic() {
    let world = world();
    let first_replay = deterministic_result();
    let second_replay = deterministic_result();
    assert_eq!(first_replay, second_replay);
    assert_eq!(
        serde_json::to_vec(&first_replay.0).unwrap(),
        serde_json::to_vec(&second_replay.0).unwrap()
    );

    let mut session = TraversalSession::initialize(world).unwrap();
    drive_authored_route(&mut session, world, 1);
    let completion = session.finish().unwrap();
    let snapshot = session.snapshot().unwrap();
    let batch = run_playthrough(world).unwrap();
    assert_eq!(
        session.steps(),
        batch.body.steps,
        "the incremental route must use the batch traversal semantics"
    );
    assert_eq!(completion.body.outcome, TraversalOutcome::Completed);
    assert_eq!(completion.body.world_artifact_sha256, world.artifact_sha256);
    assert_eq!(
        session.current_cell(),
        Some(world.body.navigation.objective_cell)
    );
    assert!(session.is_finished());
    assert_eq!(snapshot, first_replay.0);
    assert_eq!(completion, first_replay.1);
    assert_eq!(session.state_sha256().unwrap(), snapshot.state_sha256);
}

#[test]
fn snapshot_restore_continuation_matches_uninterrupted_session() {
    let world = world();
    let route = &world.body.navigation.route_cells;
    assert!(route.len() > 3, "fixture needs a meaningful continuation");
    let split = route.len() / 2;

    let mut uninterrupted = TraversalSession::initialize(world).unwrap();
    drive_authored_route(&mut uninterrupted, world, 1);
    let uninterrupted_completion = uninterrupted.finish().unwrap();
    let uninterrupted_snapshot = uninterrupted.snapshot().unwrap();

    let mut partial = TraversalSession::initialize(world).unwrap();
    for (tick, cell) in route.iter().copied().enumerate().take(split).skip(1) {
        partial
            .advance(TraversalSessionInput {
                expected_tick: tick as u64,
                destination_cell: cell,
            })
            .unwrap();
    }
    let checkpoint = partial.snapshot().unwrap();
    let mut restored = TraversalSession::restore(world, &checkpoint).unwrap();
    for (tick, cell) in route.iter().copied().enumerate().skip(split) {
        restored
            .advance(TraversalSessionInput {
                expected_tick: tick as u64,
                destination_cell: cell,
            })
            .unwrap();
    }
    let restored_completion = restored.finish().unwrap();
    let restored_snapshot = restored.snapshot().unwrap();

    assert_eq!(restored_snapshot, uninterrupted_snapshot);
    assert_eq!(restored_completion, uninterrupted_completion);
    validate_traversal_session_completion(world, &restored_snapshot, &restored_completion)
        .expect("completion independently replays from the sealed terminal snapshot");
}

#[test]
fn stale_and_duplicate_ticks_are_rejected_without_mutating_state() {
    let world = world();
    let mut session = TraversalSession::initialize(world).unwrap();
    let before = session.snapshot().unwrap();
    let first_destination = world.body.navigation.route_cells[1];

    let stale = session
        .advance(TraversalSessionInput {
            expected_tick: 0,
            destination_cell: first_destination,
        })
        .expect_err("tick zero is already consumed by initialization");
    assert_eq!(stale.code, "stale_runtime_step");
    assert_eq!(session.snapshot().unwrap(), before);

    session
        .advance(TraversalSessionInput {
            expected_tick: 1,
            destination_cell: first_destination,
        })
        .unwrap();
    let after_first = session.snapshot().unwrap();
    let duplicate = session
        .advance(TraversalSessionInput {
            expected_tick: 1,
            destination_cell: first_destination,
        })
        .expect_err("a repeated tick cannot be applied twice");
    assert_eq!(duplicate.code, "stale_runtime_step");
    assert_eq!(session.snapshot().unwrap(), after_first);
}

#[test]
fn non_adjacent_collision_and_over_slope_moves_fail_closed() {
    let world = world();
    let start = world.body.navigation.start_cell;

    let mut non_adjacent_session = TraversalSession::initialize(world).unwrap();
    let non_adjacent_destination = (0..world.body.fields.heights_m.len())
        .find(|cell| {
            cell_is_clear(world, *cell)
                && manhattan(start, *cell, world.body.authored_layout.resolution) > 1
        })
        .expect("fixture has a clear non-adjacent destination");
    let error = non_adjacent_session
        .advance(TraversalSessionInput {
            expected_tick: 1,
            destination_cell: non_adjacent_destination,
        })
        .expect_err("teleporting is rejected");
    assert!(error.message.contains("non-adjacent"));

    let parents = reachable_parents(world);
    let (collision_path, blocked_cell) = (0..parents.len())
        .filter(|cell| parents[*cell].is_some())
        .find_map(|cell| {
            neighbors(cell, world.body.authored_layout.resolution)
                .into_iter()
                .find(|neighbor| !cell_is_clear(world, *neighbor))
                .map(|blocked| (path_from_parents(&parents, cell), blocked))
        })
        .expect("fixture has an accessible collision boundary");
    let mut collision_session = TraversalSession::initialize(world).unwrap();
    for (tick, cell) in collision_path.iter().copied().enumerate().skip(1) {
        collision_session
            .advance(TraversalSessionInput {
                expected_tick: tick as u64,
                destination_cell: cell,
            })
            .unwrap();
    }
    let collision_error = collision_session
        .advance(TraversalSessionInput {
            expected_tick: collision_session.next_tick(),
            destination_cell: blocked_cell,
        })
        .expect_err("entering a blocked cell is rejected");
    assert!(collision_error.message.contains("collided"));

    let slope_world = slope_world();
    let slope_parents = reachable_parents(slope_world);
    let (slope_path, steep_destination) = (0..slope_parents.len())
        .filter(|cell| slope_parents[*cell].is_some())
        .find_map(|cell| {
            neighbors(cell, slope_world.body.authored_layout.resolution)
                .into_iter()
                .find(|neighbor| {
                    cell_is_clear(slope_world, *neighbor)
                        && edge_grade(slope_world, cell, *neighbor)
                            > slope_world.body.authored_layout.traversal.maximum_grade
                })
                .map(|steep| (path_from_parents(&slope_parents, cell), steep))
        })
        .expect("fixture has a reachable edge above the authored slope limit");
    let mut slope_session = TraversalSession::initialize(slope_world).unwrap();
    for (tick, cell) in slope_path.iter().copied().enumerate().skip(1) {
        slope_session
            .advance(TraversalSessionInput {
                expected_tick: tick as u64,
                destination_cell: cell,
            })
            .unwrap();
    }
    let slope_error = slope_session
        .advance(TraversalSessionInput {
            expected_tick: slope_session.next_tick(),
            destination_cell: steep_destination,
        })
        .expect_err("over-limit slope is rejected");
    assert!(slope_error.message.contains("slope limit"));
}

#[test]
fn tampered_snapshot_digest_and_resealed_divergent_history_are_rejected() {
    let world = world();
    let route = &world.body.navigation.route_cells;
    let mut session = TraversalSession::initialize(world).unwrap();
    session
        .advance(TraversalSessionInput {
            expected_tick: 1,
            destination_cell: route[1],
        })
        .unwrap();
    let snapshot = session.snapshot().unwrap();

    let mut tampered_digest = snapshot.clone();
    tampered_digest.body.objective_reached = !tampered_digest.body.objective_reached;
    let error = TraversalSession::restore(world, &tampered_digest)
        .expect_err("body changes require a new state digest");
    assert_eq!(error.code, "provenance_failure");

    let mut divergent = snapshot;
    divergent.body.steps[1].cell = (0..world.body.fields.heights_m.len())
        .find(|cell| {
            cell_is_clear(world, *cell)
                && manhattan(route[0], *cell, world.body.authored_layout.resolution) > 1
        })
        .expect("fixture has a clear non-adjacent destination");
    divergent.state_sha256 = digest(&divergent.body);
    let error = TraversalSession::restore(world, &divergent)
        .expect_err("even a resealed history must replay under the spatial contract");
    assert!(matches!(
        error.code,
        "contract_violation" | "runtime_state_diverged"
    ));
}

fn cell_is_clear(world: &WorldArtifact, cell: usize) -> bool {
    let layout = &world.body.authored_layout;
    if cell >= world.body.fields.heights_m.len() {
        return false;
    }
    let position = cell_position(world, cell);
    let radius = layout.traversal.agent_radius_m;
    if position[0] < -layout.width_m / 2.0 + radius
        || position[0] > layout.width_m / 2.0 - radius
        || position[1] < -layout.length_m / 2.0 + radius
        || position[1] > layout.length_m / 2.0 - radius
    {
        return false;
    }
    if layout
        .regions
        .iter()
        .find(|region| region.code == world.body.fields.region_codes[cell])
        .is_some_and(|region| region.blocks_traversal)
    {
        return false;
    }
    !world.body.collision.obstacles.iter().any(|obstacle| {
        let dx = position[0] - obstacle.center_xz_m[0];
        let dz = position[1] - obstacle.center_xz_m[1];
        dx.hypot(dz) < obstacle.radius_m + radius
    })
}

fn edge_grade(world: &WorldArtifact, from: usize, to: usize) -> f64 {
    let layout = &world.body.authored_layout;
    let same_row = from / layout.resolution == to / layout.resolution;
    let spacing = if same_row {
        layout.width_m / (layout.resolution - 1) as f64
    } else {
        layout.length_m / (layout.resolution - 1) as f64
    };
    (world.body.fields.heights_m[from] - world.body.fields.heights_m[to]).abs() / spacing
}

fn cell_position(world: &WorldArtifact, cell: usize) -> [f64; 2] {
    let layout = &world.body.authored_layout;
    let row = cell / layout.resolution;
    let column = cell % layout.resolution;
    [
        -layout.width_m / 2.0 + column as f64 * layout.width_m / (layout.resolution - 1) as f64,
        layout.length_m / 2.0 - row as f64 * layout.length_m / (layout.resolution - 1) as f64,
    ]
}

fn manhattan(left: usize, right: usize, resolution: usize) -> usize {
    (left / resolution).abs_diff(right / resolution)
        + (left % resolution).abs_diff(right % resolution)
}

fn neighbors(cell: usize, resolution: usize) -> Vec<usize> {
    let row = cell / resolution;
    let column = cell % resolution;
    let mut output = Vec::with_capacity(4);
    if row > 0 {
        output.push(cell - resolution);
    }
    if row + 1 < resolution {
        output.push(cell + resolution);
    }
    if column > 0 {
        output.push(cell - 1);
    }
    if column + 1 < resolution {
        output.push(cell + 1);
    }
    output
}

fn reachable_parents(world: &WorldArtifact) -> Vec<Option<usize>> {
    let resolution = world.body.authored_layout.resolution;
    let mut parents = vec![None; world.body.fields.heights_m.len()];
    let start = world.body.navigation.start_cell;
    parents[start] = Some(start);
    let mut queue = VecDeque::from([start]);
    while let Some(cell) = queue.pop_front() {
        for neighbor in neighbors(cell, resolution) {
            if parents[neighbor].is_none()
                && cell_is_clear(world, neighbor)
                && edge_grade(world, cell, neighbor)
                    <= world.body.authored_layout.traversal.maximum_grade
            {
                parents[neighbor] = Some(cell);
                queue.push_back(neighbor);
            }
        }
    }
    parents
}

fn path_from_parents(parents: &[Option<usize>], target: usize) -> Vec<usize> {
    let mut path = vec![target];
    let mut cursor = target;
    while let Some(parent) = parents[cursor] {
        if parent == cursor {
            break;
        }
        path.push(parent);
        cursor = parent;
    }
    path.reverse();
    path
}

fn digest<T: serde::Serialize>(value: &T) -> String {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(value).expect("test state serializes");
    let mut output = String::from("sha256:");
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to a string succeeds");
    }
    output
}
