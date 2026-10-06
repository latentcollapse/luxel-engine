//! Deterministic, engine-neutral kinematic movement over the certified Luxel world.
//!
//! This is a bounded contact/query seam, not a force-based physics engine. The
//! controlled body is a grounded disc using the authored navigation radius;
//! terrain heights and grades come from Julia's spatial fields, while obstacle,
//! region, and world-bound semantics come from the Rust-owned world artifact.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    REFERENCE_TICK_RATE_HZ, ReferenceRuntimeError, WorldArtifact, validate_world_artifact,
};

pub const PHYSICS_STATE_SCHEMA: &str = "luxel.reference-kinematic-state/v1";
pub const PHYSICS_STEP_SCHEMA: &str = "luxel.reference-kinematic-step/v1";
/// Bounds the amount of field sampling performed by one movement request.
pub const PHYSICS_MAX_SWEEP_SAMPLES: usize = 4096;
const STATE_DIGEST_DOMAIN: &[u8] = b"luxel.reference-kinematic-state/v1\0";
const STEP_DIGEST_DOMAIN: &[u8] = b"luxel.reference-kinematic-step/v1\0";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum KinematicContactKind {
    TerrainGround,
    Obstacle,
    WorldBoundary,
    BlockedRegion,
    SlopeLimit,
}

impl KinematicContactKind {
    const fn blocks_motion(self) -> bool {
        !matches!(self, Self::TerrainGround)
    }
}

/// Contact geometry is reported at the controlled body's ground-plane origin.
/// `penetration_depth_m` is geometric; slope contacts instead carry grade data.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KinematicContact {
    pub kind: KinematicContactKind,
    pub contact_id: String,
    pub sweep_sample: u32,
    pub position_xyz_m: [f64; 3],
    /// Unit outward normal for an obstacle, inward normal for a world edge,
    /// and terrain surface normal for grounding. Semantic contacts use zero.
    pub normal_xyz: [f64; 3],
    pub penetration_depth_m: f64,
    pub measured_grade: Option<f64>,
    pub maximum_grade: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KinematicStateBody {
    pub schema_version: String,
    pub world_artifact_id: String,
    pub world_artifact_sha256: String,
    pub tick: u64,
    /// Body origin is the grounded point `(x, terrain height, z)` in metres.
    pub position_xyz_m: [f64; 3],
    pub velocity_xz_mps: [f64; 2],
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KinematicState {
    pub body: KinematicStateBody,
    pub state_sha256: String,
}

impl KinematicState {
    /// Canonical JSON bytes for the digest-bound state body.
    pub fn canonical_body_json(&self) -> Result<Vec<u8>, ReferenceRuntimeError> {
        canonical_json(&self.body, "kinematic state")
    }
}

/// A planar velocity command consumed for exactly one shared reference tick.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KinematicInput {
    pub expected_tick: u64,
    pub velocity_xz_mps: [f64; 2],
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KinematicStepDisposition {
    Advanced,
    Rejected,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KinematicStepBody {
    pub schema_version: String,
    pub world_artifact_id: String,
    pub world_artifact_sha256: String,
    pub previous_state_sha256: String,
    pub input: KinematicInput,
    pub fixed_tick_rate_hz: u32,
    pub disposition: KinematicStepDisposition,
    pub attempted_position_xyz_m: [f64; 3],
    pub next_state: KinematicState,
    pub contacts: Vec<KinematicContact>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KinematicStepReceipt {
    pub body: KinematicStepBody,
    pub receipt_sha256: String,
}

impl KinematicStepReceipt {
    /// Canonical JSON bytes for the digest-bound step body.
    pub fn canonical_body_json(&self) -> Result<Vec<u8>, ReferenceRuntimeError> {
        canonical_json(&self.body, "kinematic step")
    }
}

/// Validated read-only view over one authored world and its Julia-produced
/// numerical fields. Validation runs once when this view is created.
pub struct PhysicsWorld<'a> {
    world: &'a WorldArtifact,
}

#[derive(Clone, Copy)]
struct SurfaceSample {
    height_m: f64,
    normal_xyz: [f64; 3],
    maximum_local_grade: f64,
    grid_cell: usize,
}

impl<'a> PhysicsWorld<'a> {
    pub fn new(world: &'a WorldArtifact) -> Result<Self, ReferenceRuntimeError> {
        validate_world_artifact(world)?;
        Ok(Self { world })
    }

    /// Initialize from the authored player start. The world artifact already
    /// binds this spawn to a clear navigation cell.
    pub fn initialize_player(&self) -> Result<KinematicState, ReferenceRuntimeError> {
        let spawn_id = &self.world.body.authored_layout.traversal.start_spawn_id;
        let spawn = self
            .world
            .body
            .spawns
            .iter()
            .find(|spawn| &spawn.spawn_id == spawn_id)
            .ok_or_else(|| {
                ReferenceRuntimeError::contract("player start spawn is missing".into())
            })?;
        self.state_at(spawn.position_xz_m)
    }

    /// Create a grounded state at a finite in-bounds point. This is useful for
    /// deterministic probes and test controls; static obstacle, region, and
    /// body-boundary penetration is rejected. A body may be stationary on a
    /// steep sample, but any movement across an over-limit grade is rejected.
    pub fn state_at(
        &self,
        position_xz_m: [f64; 2],
    ) -> Result<KinematicState, ReferenceRuntimeError> {
        finite_pair(position_xz_m, "initial xz position")?;
        let layout = &self.world.body.authored_layout;
        if position_xz_m[0].abs() > layout.width_m / 2.0
            || position_xz_m[1].abs() > layout.length_m / 2.0
        {
            return Err(ReferenceRuntimeError::contract(
                "initial position lies outside authored world bounds".into(),
            ));
        }
        let sample = self.surface_at(position_xz_m)?;
        let mut contacts = self.static_contacts(position_xz_m, 0, sample)?;
        self.append_obstacle_contacts(position_xz_m, 0, sample.height_m, &mut contacts)?;
        if contacts.iter().any(|contact| {
            !matches!(
                contact.kind,
                KinematicContactKind::TerrainGround | KinematicContactKind::SlopeLimit
            )
        }) {
            stable_sort_contacts(&mut contacts);
            return Err(ReferenceRuntimeError::contract(format!(
                "initial kinematic position is blocked by {}",
                contacts
                    .iter()
                    .find(|contact| contact.kind.blocks_motion())
                    .map(|contact| contact.contact_id.as_str())
                    .unwrap_or("a world constraint")
            )));
        }
        Ok(self.seal_state(KinematicStateBody {
            schema_version: PHYSICS_STATE_SCHEMA.into(),
            world_artifact_id: self.world.artifact_id.clone(),
            world_artifact_sha256: self.world.artifact_sha256.clone(),
            tick: 0,
            position_xyz_m: [
                canonical_zero(position_xz_m[0]),
                canonical_zero(sample.height_m),
                canonical_zero(position_xz_m[1]),
            ],
            velocity_xz_mps: [0.0, 0.0],
        }))
    }

    /// Return the canonical static contacts at an xz position. Positions
    /// outside the playable body bounds report a boundary contact; terrain
    /// sampling clamps to the authored field edge for finite diagnostics.
    pub fn contacts_at(
        &self,
        position_xz_m: [f64; 2],
    ) -> Result<Vec<KinematicContact>, ReferenceRuntimeError> {
        finite_pair(position_xz_m, "contact query position")?;
        let sample = self.surface_at(position_xz_m)?;
        let mut contacts = self.static_contacts(position_xz_m, 0, sample)?;
        self.append_obstacle_contacts(position_xz_m, 0, sample.height_m, &mut contacts)?;
        stable_sort_contacts(&mut contacts);
        Ok(contacts)
    }

    /// Integrate one planar velocity command at the shared fixed tick rate.
    /// A colliding or over-slope move is rejected atomically: the tick is
    /// consumed, the body remains grounded at its prior xz, and its velocity
    /// becomes zero. There is no sliding or impulse response in this seam.
    pub fn step(
        &self,
        state: &KinematicState,
        input: KinematicInput,
    ) -> Result<KinematicStepReceipt, ReferenceRuntimeError> {
        self.validate_state(state)?;
        finite_pair(input.velocity_xz_mps, "kinematic velocity")?;
        let expected_tick = state.body.tick.checked_add(1).ok_or_else(|| {
            ReferenceRuntimeError::contract("kinematic tick counter overflow".into())
        })?;
        if input.expected_tick != expected_tick {
            return Err(ReferenceRuntimeError::stale(format!(
                "expected kinematic tick {expected_tick}, received {}",
                input.expected_tick
            )));
        }

        let dt = 1.0 / f64::from(REFERENCE_TICK_RATE_HZ);
        let start_xz = [state.body.position_xyz_m[0], state.body.position_xyz_m[2]];
        let displacement = [input.velocity_xz_mps[0] * dt, input.velocity_xz_mps[1] * dt];
        finite_pair(displacement, "fixed-step displacement")?;
        let attempted_xz = [start_xz[0] + displacement[0], start_xz[1] + displacement[1]];
        finite_pair(attempted_xz, "integrated position")?;
        let distance_m = displacement[0].hypot(displacement[1]);
        if !distance_m.is_finite() {
            return Err(ReferenceRuntimeError::contract(
                "fixed-step displacement magnitude must be finite".into(),
            ));
        }

        let layout = &self.world.body.authored_layout;
        let dx = layout.width_m / (layout.resolution - 1) as f64;
        let dz = layout.length_m / (layout.resolution - 1) as f64;
        let max_sample_spacing = dx.min(dz) * 0.5;
        let sample_count = if distance_m == 0.0 {
            0
        } else {
            let required = (distance_m / max_sample_spacing).ceil();
            if !required.is_finite() || required > PHYSICS_MAX_SWEEP_SAMPLES as f64 {
                return Err(ReferenceRuntimeError::contract(format!(
                    "kinematic sweep exceeds the bounded {PHYSICS_MAX_SWEEP_SAMPLES}-sample step"
                )));
            }
            (required as usize).max(1)
        };

        let mut contacts = Vec::new();
        for index in 1..=sample_count {
            let fraction = index as f64 / sample_count as f64;
            let point = [
                start_xz[0] + displacement[0] * fraction,
                start_xz[1] + displacement[1] * fraction,
            ];
            let sample = self.surface_at(point)?;
            contacts.extend(self.constraint_contacts(
                point,
                index as u32,
                sample,
                distance_m > 0.0,
            )?);
        }

        if distance_m > 0.0 {
            self.append_swept_obstacle_contacts(
                start_xz,
                attempted_xz,
                sample_count,
                &mut contacts,
            )?;
        }
        let attempted_surface = self.surface_at(attempted_xz)?;
        contacts.push(ground_contact(
            attempted_xz,
            attempted_surface,
            sample_count as u32,
        ));
        stable_sort_contacts(&mut contacts);

        let rejected = contacts.iter().any(|contact| contact.kind.blocks_motion());
        let (result_xz, result_surface, result_velocity) = if rejected {
            let surface = self.surface_at(start_xz)?;
            (start_xz, surface, [0.0, 0.0])
        } else {
            (attempted_xz, attempted_surface, input.velocity_xz_mps)
        };
        let next_state = self.seal_state(KinematicStateBody {
            schema_version: PHYSICS_STATE_SCHEMA.into(),
            world_artifact_id: self.world.artifact_id.clone(),
            world_artifact_sha256: self.world.artifact_sha256.clone(),
            tick: input.expected_tick,
            position_xyz_m: [
                canonical_zero(result_xz[0]),
                canonical_zero(result_surface.height_m),
                canonical_zero(result_xz[1]),
            ],
            velocity_xz_mps: result_velocity.map(canonical_zero),
        });
        let attempted_position_xyz_m = [
            canonical_zero(attempted_xz[0]),
            canonical_zero(attempted_surface.height_m),
            canonical_zero(attempted_xz[1]),
        ];
        let body = KinematicStepBody {
            schema_version: PHYSICS_STEP_SCHEMA.into(),
            world_artifact_id: self.world.artifact_id.clone(),
            world_artifact_sha256: self.world.artifact_sha256.clone(),
            previous_state_sha256: state.state_sha256.clone(),
            input,
            fixed_tick_rate_hz: REFERENCE_TICK_RATE_HZ,
            disposition: if rejected {
                KinematicStepDisposition::Rejected
            } else {
                KinematicStepDisposition::Advanced
            },
            attempted_position_xyz_m,
            next_state,
            contacts,
        };
        let receipt_sha256 = digest_body(STEP_DIGEST_DOMAIN, &body, "kinematic step")?;
        Ok(KinematicStepReceipt {
            body,
            receipt_sha256,
        })
    }

    /// Re-run a step from its inputs and compare the complete typed receipt.
    pub fn validate_step_receipt(
        &self,
        prior_state: &KinematicState,
        input: KinematicInput,
        receipt: &KinematicStepReceipt,
    ) -> Result<(), ReferenceRuntimeError> {
        let digest = digest_body(STEP_DIGEST_DOMAIN, &receipt.body, "kinematic step")?;
        if digest != receipt.receipt_sha256 {
            return Err(ReferenceRuntimeError::provenance(
                "kinematic step receipt digest does not match its body".into(),
            ));
        }
        if self.step(prior_state, input)? != *receipt {
            return Err(ReferenceRuntimeError::divergence(
                "kinematic step receipt differs from deterministic replay".into(),
            ));
        }
        Ok(())
    }

    fn validate_state(&self, state: &KinematicState) -> Result<(), ReferenceRuntimeError> {
        if state.body.schema_version != PHYSICS_STATE_SCHEMA
            || state.body.world_artifact_id != self.world.artifact_id
            || state.body.world_artifact_sha256 != self.world.artifact_sha256
        {
            return Err(ReferenceRuntimeError::provenance(
                "kinematic state belongs to a different world or schema".into(),
            ));
        }
        let digest = digest_body(STATE_DIGEST_DOMAIN, &state.body, "kinematic state")?;
        if digest != state.state_sha256 {
            return Err(ReferenceRuntimeError::provenance(
                "kinematic state digest does not match its body".into(),
            ));
        }
        finite_pair(
            [state.body.position_xyz_m[0], state.body.position_xyz_m[2]],
            "kinematic state xz position",
        )?;
        finite(state.body.position_xyz_m[1], "kinematic state y position")?;
        finite_pair(state.body.velocity_xz_mps, "kinematic state velocity")?;
        let position_xz = [state.body.position_xyz_m[0], state.body.position_xyz_m[2]];
        let sample = self.surface_at(position_xz)?;
        let tolerance = 64.0 * f64::EPSILON * (1.0 + sample.height_m.abs());
        if (state.body.position_xyz_m[1] - sample.height_m).abs() > tolerance {
            return Err(ReferenceRuntimeError::provenance(
                "kinematic state is not grounded on the bound terrain field".into(),
            ));
        }
        let mut contacts = self.static_contacts(position_xz, 0, sample)?;
        self.append_obstacle_contacts(position_xz, 0, sample.height_m, &mut contacts)?;
        if contacts.iter().any(|contact| {
            contact.kind.blocks_motion() && contact.kind != KinematicContactKind::SlopeLimit
        }) {
            return Err(ReferenceRuntimeError::provenance(
                "kinematic state begins inside a blocking world constraint".into(),
            ));
        }
        Ok(())
    }

    fn seal_state(&self, body: KinematicStateBody) -> KinematicState {
        let state_sha256 = digest_body(STATE_DIGEST_DOMAIN, &body, "kinematic state")
            .expect("validated finite kinematic body serializes");
        KinematicState { body, state_sha256 }
    }

    fn surface_at(&self, position_xz_m: [f64; 2]) -> Result<SurfaceSample, ReferenceRuntimeError> {
        finite_pair(position_xz_m, "terrain sample position")?;
        let layout = &self.world.body.authored_layout;
        let resolution = layout.resolution;
        let spacing_x = layout.width_m / (resolution - 1) as f64;
        let spacing_z = layout.length_m / (resolution - 1) as f64;
        let x = position_xz_m[0].clamp(-layout.width_m / 2.0, layout.width_m / 2.0);
        let z = position_xz_m[1].clamp(-layout.length_m / 2.0, layout.length_m / 2.0);
        let column = ((x + layout.width_m / 2.0) / spacing_x).clamp(0.0, (resolution - 1) as f64);
        let row = ((layout.length_m / 2.0 - z) / spacing_z).clamp(0.0, (resolution - 1) as f64);
        let column0 = (column.floor() as usize).min(resolution - 2);
        let row0 = (row.floor() as usize).min(resolution - 2);
        let column1 = column0 + 1;
        let row1 = row0 + 1;
        let tx = (column - column0 as f64).clamp(0.0, 1.0);
        let tz = (row - row0 as f64).clamp(0.0, 1.0);
        let fields = &self.world.body.fields;
        let index00 = row0 * resolution + column0;
        let index10 = row0 * resolution + column1;
        let index01 = row1 * resolution + column0;
        let index11 = row1 * resolution + column1;
        let h00 = fields.heights_m[index00];
        let h10 = fields.heights_m[index10];
        let h01 = fields.heights_m[index01];
        let h11 = fields.heights_m[index11];
        let height_m = h00 * (1.0 - tx) * (1.0 - tz)
            + h10 * tx * (1.0 - tz)
            + h01 * (1.0 - tx) * tz
            + h11 * tx * tz;
        let derivative_x = ((h10 - h00) * (1.0 - tz) + (h11 - h01) * tz) / spacing_x;
        let derivative_row = ((h01 - h00) * (1.0 - tx) + (h11 - h10) * tx) / spacing_z;
        let derivative_z = -derivative_row;
        let normal_length = derivative_x.hypot(1.0).hypot(derivative_z);
        let normal_xyz = [
            canonical_zero(-derivative_x / normal_length),
            canonical_zero(1.0 / normal_length),
            canonical_zero(-derivative_z / normal_length),
        ];
        let maximum_local_grade = fields.slope_grade[index00]
            .max(fields.slope_grade[index10])
            .max(fields.slope_grade[index01])
            .max(fields.slope_grade[index11]);
        let grid_cell = crate::world::cell_for_position(layout, [x, z]);
        if !height_m.is_finite()
            || !maximum_local_grade.is_finite()
            || normal_xyz.iter().any(|value| !value.is_finite())
        {
            return Err(ReferenceRuntimeError::contract(
                "terrain interpolation produced a non-finite surface sample".into(),
            ));
        }
        Ok(SurfaceSample {
            height_m: canonical_zero(height_m),
            normal_xyz,
            maximum_local_grade,
            grid_cell,
        })
    }

    fn static_contacts(
        &self,
        position_xz_m: [f64; 2],
        sweep_sample: u32,
        surface: SurfaceSample,
    ) -> Result<Vec<KinematicContact>, ReferenceRuntimeError> {
        let mut contacts = vec![ground_contact(position_xz_m, surface, sweep_sample)];
        contacts.extend(self.constraint_contacts(position_xz_m, sweep_sample, surface, true)?);
        Ok(contacts)
    }

    fn constraint_contacts(
        &self,
        position_xz_m: [f64; 2],
        sweep_sample: u32,
        surface: SurfaceSample,
        check_slope: bool,
    ) -> Result<Vec<KinematicContact>, ReferenceRuntimeError> {
        let layout = &self.world.body.authored_layout;
        let radius = self.world.body.collision.agent_radius_m;
        let x_min = -layout.width_m / 2.0 + radius;
        let x_max = layout.width_m / 2.0 - radius;
        let z_min = -layout.length_m / 2.0 + radius;
        let z_max = layout.length_m / 2.0 - radius;
        let x = position_xz_m[0];
        let z = position_xz_m[1];
        let mut contacts = Vec::new();
        for (id, penetration, normal_xyz) in [
            ("x_min", (x_min - x).max(0.0), [1.0, 0.0, 0.0]),
            ("x_max", (x - x_max).max(0.0), [-1.0, 0.0, 0.0]),
            ("z_min", (z_min - z).max(0.0), [0.0, 0.0, 1.0]),
            ("z_max", (z - z_max).max(0.0), [0.0, 0.0, -1.0]),
        ] {
            if !penetration.is_finite() {
                return Err(ReferenceRuntimeError::contract(
                    "world-boundary penetration is outside the finite numeric range".into(),
                ));
            }
            if penetration > 0.0 {
                contacts.push(KinematicContact {
                    kind: KinematicContactKind::WorldBoundary,
                    contact_id: id.into(),
                    sweep_sample,
                    position_xyz_m: [x, surface.height_m, z],
                    normal_xyz,
                    penetration_depth_m: canonical_zero(penetration),
                    measured_grade: None,
                    maximum_grade: None,
                });
            }
        }

        let region_code = self.world.body.fields.region_codes[surface.grid_cell];
        if let Some(region) = layout
            .regions
            .iter()
            .find(|region| region.code == region_code && region.blocks_traversal)
        {
            contacts.push(KinematicContact {
                kind: KinematicContactKind::BlockedRegion,
                contact_id: region.region_id.clone(),
                sweep_sample,
                position_xyz_m: [x, surface.height_m, z],
                normal_xyz: [0.0, 0.0, 0.0],
                penetration_depth_m: 0.0,
                measured_grade: None,
                maximum_grade: None,
            });
        }

        if check_slope {
            let maximum_grade = layout.traversal.maximum_grade;
            if surface.maximum_local_grade > maximum_grade {
                contacts.push(KinematicContact {
                    kind: KinematicContactKind::SlopeLimit,
                    contact_id: format!("terrain-cell-{}", surface.grid_cell),
                    sweep_sample,
                    position_xyz_m: [x, surface.height_m, z],
                    normal_xyz: [0.0, 0.0, 0.0],
                    penetration_depth_m: 0.0,
                    measured_grade: Some(surface.maximum_local_grade),
                    maximum_grade: Some(maximum_grade),
                });
            }
        }
        Ok(contacts)
    }

    fn append_obstacle_contacts(
        &self,
        position_xz_m: [f64; 2],
        sweep_sample: u32,
        ground_y_m: f64,
        contacts: &mut Vec<KinematicContact>,
    ) -> Result<(), ReferenceRuntimeError> {
        let radius = self.world.body.collision.agent_radius_m;
        for obstacle in &self.world.body.collision.obstacles {
            let offset = [
                position_xz_m[0] - obstacle.center_xz_m[0],
                position_xz_m[1] - obstacle.center_xz_m[1],
            ];
            let separation = offset[0].hypot(offset[1]);
            let penetration = obstacle.radius_m + radius - separation;
            if !separation.is_finite() || !penetration.is_finite() {
                return Err(ReferenceRuntimeError::contract(
                    "obstacle contact exceeds the finite numeric range".into(),
                ));
            }
            if penetration > 0.0 {
                let normal = if separation > 0.0 {
                    [offset[0] / separation, 0.0, offset[1] / separation]
                } else {
                    [1.0, 0.0, 0.0]
                };
                contacts.push(KinematicContact {
                    kind: KinematicContactKind::Obstacle,
                    contact_id: obstacle.obstacle_id.clone(),
                    sweep_sample,
                    position_xyz_m: [position_xz_m[0], ground_y_m, position_xz_m[1]],
                    normal_xyz: normal.map(canonical_zero),
                    penetration_depth_m: canonical_zero(penetration),
                    measured_grade: None,
                    maximum_grade: None,
                });
            }
        }
        Ok(())
    }

    fn append_swept_obstacle_contacts(
        &self,
        start_xz: [f64; 2],
        end_xz: [f64; 2],
        sample_count: usize,
        contacts: &mut Vec<KinematicContact>,
    ) -> Result<(), ReferenceRuntimeError> {
        let segment = [end_xz[0] - start_xz[0], end_xz[1] - start_xz[1]];
        let segment_length = segment[0].hypot(segment[1]);
        let segment_direction = if segment_length > 0.0 {
            [segment[0] / segment_length, segment[1] / segment_length]
        } else {
            [0.0, 0.0]
        };
        let body_radius = self.world.body.collision.agent_radius_m;
        for obstacle in &self.world.body.collision.obstacles {
            let relative = [
                obstacle.center_xz_m[0] - start_xz[0],
                obstacle.center_xz_m[1] - start_xz[1],
            ];
            let fraction = if segment_length > 0.0 {
                ((relative[0] * segment_direction[0] + relative[1] * segment_direction[1])
                    / segment_length)
                    .clamp(0.0, 1.0)
            } else {
                0.0
            };
            let closest = [
                start_xz[0] + segment[0] * fraction,
                start_xz[1] + segment[1] * fraction,
            ];
            let offset = [
                closest[0] - obstacle.center_xz_m[0],
                closest[1] - obstacle.center_xz_m[1],
            ];
            let separation = offset[0].hypot(offset[1]);
            let penetration = obstacle.radius_m + body_radius - separation;
            if penetration > 0.0 {
                let surface = self.surface_at(closest)?;
                let normal = if separation > 0.0 {
                    [offset[0] / separation, 0.0, offset[1] / separation]
                } else {
                    [1.0, 0.0, 0.0]
                };
                contacts.push(KinematicContact {
                    kind: KinematicContactKind::Obstacle,
                    contact_id: obstacle.obstacle_id.clone(),
                    sweep_sample: (fraction * sample_count as f64).ceil() as u32,
                    position_xyz_m: [closest[0], surface.height_m, closest[1]],
                    normal_xyz: normal.map(canonical_zero),
                    penetration_depth_m: canonical_zero(penetration),
                    measured_grade: None,
                    maximum_grade: None,
                });
            }
        }
        Ok(())
    }
}

fn ground_contact(
    position_xz_m: [f64; 2],
    surface: SurfaceSample,
    sweep_sample: u32,
) -> KinematicContact {
    KinematicContact {
        kind: KinematicContactKind::TerrainGround,
        contact_id: "terrain_surface".into(),
        sweep_sample,
        position_xyz_m: [
            canonical_zero(position_xz_m[0]),
            surface.height_m,
            canonical_zero(position_xz_m[1]),
        ],
        normal_xyz: surface.normal_xyz,
        penetration_depth_m: 0.0,
        measured_grade: None,
        maximum_grade: None,
    }
}

fn stable_sort_contacts(contacts: &mut [KinematicContact]) {
    contacts.sort_by(|left, right| {
        left.sweep_sample
            .cmp(&right.sweep_sample)
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.contact_id.cmp(&right.contact_id))
            .then_with(|| compare_position(left.position_xyz_m, right.position_xyz_m))
    });
}

fn compare_position(left: [f64; 3], right: [f64; 3]) -> std::cmp::Ordering {
    left.into_iter()
        .zip(right)
        .map(|(left, right)| left.total_cmp(&right))
        .find(|order| !order.is_eq())
        .unwrap_or(std::cmp::Ordering::Equal)
}

fn finite_pair(values: [f64; 2], label: &str) -> Result<(), ReferenceRuntimeError> {
    finite(values[0], label)?;
    finite(values[1], label)
}

fn finite(value: f64, label: &str) -> Result<(), ReferenceRuntimeError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ReferenceRuntimeError::contract(format!(
            "{label} must be finite"
        )))
    }
}

fn canonical_zero(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}

fn canonical_json<T: Serialize>(value: &T, label: &str) -> Result<Vec<u8>, ReferenceRuntimeError> {
    serde_json::to_vec(value).map_err(|error| {
        ReferenceRuntimeError::contract(format!("{label} canonical JSON failed: {error}"))
    })
}

fn digest_body<T: Serialize>(
    domain: &[u8],
    value: &T,
    label: &str,
) -> Result<String, ReferenceRuntimeError> {
    let bytes = canonical_json(value, label)?;
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update(bytes);
    Ok(format!("sha256:{}", hex(&digest.finalize())))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_zero_normalizes_signed_zero() {
        assert_eq!(canonical_zero(-0.0).to_bits(), 0.0_f64.to_bits());
    }

    #[test]
    fn prefixed_sha256_is_not_the_unprefixed_byte_digest() {
        let digest = digest_body(STATE_DIGEST_DOMAIN, &"control", "control").unwrap();
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest.len(), 71);
        assert_ne!(digest, crate::fields::prefixed_sha256(b"control"));
    }
}
