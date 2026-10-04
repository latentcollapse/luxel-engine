//! CALIBRATION-1: the material calibration scene (WGE_CONVERGE1_CONTRACTS.md §3).
//!
//! The scene is an imported asset (`tools/build_calibration_glb.py`) and takes
//! the authorized C2 route: GLB → `prepare_asset` + `condition_render_asset` →
//! `SceneArtifact` → `compose_bound_scene_with_view` →
//! `render_bound_scene_and_promote`. Nothing here reads files; the GLB bytes
//! and the world are explicit inputs, so a calibration packet is a pure
//! function of (world, GLB, rig, view) and the supervisor can re-derive it.
//!
//! Lighting is the one thing the bound-scene path could not express: the
//! world's base projection fixes lights and environment. `CalibrationRig` is
//! that seam, authorized by ENUMERATION exactly like `Campaign2View`: a rig is
//! a name, and its lights, environment and render policy are constants here.

use serde::{Deserialize, Serialize};
use wge_asset_contract::{
    ASSET_RUNTIME_REQUEST_SCHEMA, AssetPreparationReceipt, AssetPreparationRequest, AssetUse,
    Axis as AssetAxis, CollisionMetadata, CollisionShape, LodMetadata, RenderAssetPackage,
    RenderConditioningRequest, RenderMipPolicy, RuntimeTarget,
};
use wge_project_ledger::{
    CollisionPolicy, MaterialAssignment, SCENE_ARTIFACT_SCHEMA, SCENE_OBJECT_SCHEMA,
    SceneArtifact, SceneArtifactBody, SceneImportance, SceneLodLevel, SceneLodPolicy, SceneObject,
    SceneObjectProvenance, SceneProvenance, SceneTransform, SceneVisibilityPolicy,
    seal_scene_with_render_assets,
};
use wge_reference_runtime::WorldArtifact;

use crate::{
    CameraProjection, EnvironmentIntent, GraphicsCamera, GraphicsContractError,
    GraphicsScenePacketBody, LightIntent, LightKind, sha256_prefixed,
};
use crate::render_policy::{
    DebugPolicy, MeshSurfacePolicy, RenderPolicy, ShadowFitPolicy, ShadowPolicy, SkyModel,
    SkyPolicy,
};

/// The enumerated light rigs. Each is a fixed set of lights, environment and
/// render policy; there is no free parameter a packet could smuggle in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationRig {
    /// Sun at 35° elevation, 45° to the camera's right, analytic clear sky.
    Sun,
    /// No sun; a uniform grey sky is the only light.
    Overcast,
    /// Sun at 10° elevation behind the row (backlight), 15° off axis.
    Grazing,
    /// `Sun` with every albedo forced to 0.5 (render_policy.debug), so
    /// material identity can be judged with colour removed.
    SunAlbedoGrey,
}

impl CalibrationRig {
    pub const ALL: [Self; 4] = [Self::Sun, Self::Overcast, Self::Grazing, Self::SunAlbedoGrey];

    pub fn name(self) -> &'static str {
        match self {
            Self::Sun => "sun",
            Self::Overcast => "overcast",
            Self::Grazing => "grazing",
            Self::SunAlbedoGrey => "sun-albedo-grey",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|rig| rig.name() == name)
    }

    /// Overwrite the body's lights, environment and render policy with this
    /// rig's constants. Everything else in the body is untouched.
    pub fn apply(self, body: &mut GraphicsScenePacketBody) {
        let (direction_xyz, color_rgb, intensity) = match self {
            // Toward the sun: camera-right (−X, the cameras look +Z) and toward
            // the camera (−Z), 35° up. The light travels the other way.
            Self::Sun | Self::SunAlbedoGrey => ([0.579_228, -0.573_576, 0.579_228], [1.0, 0.96, 0.90], 3.0),
            Self::Overcast => ([0.0, -1.0, 0.0], [1.0, 1.0, 1.0], 0.0),
            // Toward the sun: behind the row (+Z), 15° to camera-right, 10° up.
            Self::Grazing => ([0.254_887, -0.173_648, -0.951_251], [1.0, 0.86, 0.70], 3.0),
        };
        body.lights = vec![LightIntent {
            light_id: format!("calibration-{}", self.name()),
            kind: LightKind::Directional { direction_xyz },
            color_rgb,
            intensity,
        }];
        body.environment = match self {
            Self::Overcast => EnvironmentIntent {
                sky_top_rgb: OVERCAST_SKY_RGB,
                sky_horizon_rgb: OVERCAST_SKY_RGB,
                ground_rgb: [0.14, 0.14, 0.14],
                fog_color_rgb: OVERCAST_SKY_RGB,
                fog_density: 0.0,
                exposure: OVERCAST_EXPOSURE,
            },
            _ => EnvironmentIntent {
                sky_top_rgb: [0.16, 0.30, 0.58],
                sky_horizon_rgb: [0.62, 0.66, 0.72],
                ground_rgb: [0.11, 0.12, 0.085],
                fog_color_rgb: [0.62, 0.66, 0.72],
                fog_density: 0.0,
                exposure: SUN_EXPOSURE,
            },
        };
        let sky = match self {
            Self::Overcast => SkyPolicy {
                sun_disc_radius_milli_deg: 0,
                sun_disc_gain_bp: 0,
                sun_glow_gain_bp: 0,
                model: None,
            },
            _ => SkyPolicy {
                sun_disc_radius_milli_deg: 650,
                sun_disc_gain_bp: 120_000,
                sun_glow_gain_bp: 2_400,
                model: Some(SkyModel::Analytic { turbidity_milli: 3000 }),
            },
        };
        // No bloom, vignette, grade or dither (all absent = off). Metric mesh
        // UVs need the repeat wrap; shadows use the `full` arm's settings and a
        // view-fitted frame. 22 m covers every calibration view (the row camera
        // is ~18 m from the row) at ~4 cm per shadow texel; the converge0 40 m
        // fit gave ~10 cm texels and rendered a 0.5 m ball's shadow as a blob.
        body.render_policy = Some(RenderPolicy {
            shadow: Some(ShadowPolicy { darkness_bp: 9600, filter_radius_milli: 2400 }),
            mesh_surface: Some(MeshSurfacePolicy { wrap_repeat: true }),
            shadow_fit: Some(ShadowFitPolicy { view_distance_m: CALIBRATION_SHADOW_DISTANCE_M }),
            sky: Some(sky),
            debug: (self == Self::SunAlbedoGrey).then_some(DebugPolicy { albedo_override_bp: 5000 }),
            ..RenderPolicy::default()
        });
    }
}

/// Fixed exposures (no auto-exposure), one per lighting setup, each set so the
/// 18% grey card renders as middle grey (0.18 linear before tone mapping), as a
/// calibration shoot exposes for the card. Sun: 1.0 measured +2.5%. Overcast:
/// the card measured sRGB 77 at 1.0 = 0.070 linear through the inverted tone
/// map, so 0.18 / 0.070 = 2.57. Grazing keeps the sun's exposure: it is the
/// same sun from behind, and a backlit card SHOULD read dark.
pub const SUN_EXPOSURE: f32 = 1.0;
pub const CALIBRATION_SHADOW_DISTANCE_M: i32 = 22;
pub const OVERCAST_EXPOSURE: f32 = 2.57;
pub const OVERCAST_SKY_RGB: [f32; 3] = [0.72, 0.73, 0.75];

/// The scan-backed material columns, in row order, plus the calibration group
/// (cards, balls, checker strip). Mesh ids are `{column}_{body}`.
pub const CALIBRATION_COLUMNS: [&str; 9] = [
    "metal", "rough_metal", "stone", "wet_stone", "bark", "wood", "painted", "terrain", "emissive",
];

/// Host placement in the riverwatch world. The 34 x 7 m footprint
/// (x −33…1, z −30.5…−23.5) is clear of every authored and foliage instance
/// and its terrain varies by 0.33 m (11.84–12.18 m), measured from the
/// reference packet. The scene is yawed 180°, so its camera side (+Z local)
/// faces −Z and the cameras look +Z, into the world rather than off its edge.
pub const CALIBRATION_ANCHOR_XZ_M: [f64; 2] = [-16.0, -26.5];
const GROUND_LOCAL_X_M: [f64; 2] = [-17.0, 17.0];
const GROUND_LOCAL_Z_M: [f64; 2] = [-3.0, 4.0];
const GROUND_CLEARANCE_M: f64 = 0.03;
const PLINTH_TOP_M: f32 = 0.15;
const SLAB_LEAN_DEG: f32 = 15.0;

pub fn render_request() -> RenderConditioningRequest {
    RenderConditioningRequest {
        schema_version: "wge.render-asset-request/v1".into(),
        meters_per_unit: 1.0,
        vertical_axis: AssetAxis::Y,
        require_uv0: true,
        generate_normals: true,
        generate_tangents: true,
        // Every scanned texture is mipped (N-4 X1: unmipped scans alias).
        mip_policy: RenderMipPolicy::GenerateCpuChain,
        max_texture_dimension: 1024,
    }
}

/// The runtime request, pinned to the GLB digest. Collision is the ground
/// plane's box (the only thing in the scene one could stand on); LOD0 names it.
pub fn runtime_request(
    glb: &[u8],
    package: &RenderAssetPackage,
) -> Result<AssetPreparationRequest, GraphicsContractError> {
    let (min, max) = mesh_bounds(package, "ground_plane")?;
    Ok(AssetPreparationRequest {
        schema_version: ASSET_RUNTIME_REQUEST_SCHEMA.into(),
        target: RuntimeTarget::Native,
        asset_use: AssetUse::StaticMesh,
        expected_source_sha256: Some(sha256_prefixed(glb).trim_start_matches("sha256:").to_owned()),
        meters_per_unit: 1.0,
        vertical_axis: AssetAxis::Y,
        rig: None,
        required_animations: Vec::new(),
        required_sockets: Vec::new(),
        collision: Some(CollisionMetadata {
            shape: CollisionShape::Box,
            center: [0, 1, 2].map(|axis| (f64::from(min[axis]) + f64::from(max[axis])) * 0.5),
            size: [0, 1, 2].map(|axis| f64::from(max[axis]) - f64::from(min[axis])),
            axis: None,
        }),
        lods: vec![LodMetadata {
            level: 0,
            mesh_name: "ground_plane".into(),
            switch_below_fraction: 1.0,
        }],
    })
}

fn mesh_bounds(
    package: &RenderAssetPackage,
    mesh_id: &str,
) -> Result<([f32; 3], [f32; 3]), GraphicsContractError> {
    let mesh = package
        .meshes
        .iter()
        .find(|mesh| mesh.mesh_id == mesh_id)
        .ok_or_else(|| GraphicsContractError::provenance(format!("calibration asset has no mesh {mesh_id}")))?;
    bounds(mesh.positions_m.iter())
        .ok_or_else(|| GraphicsContractError::provenance(format!("calibration mesh {mesh_id} is empty")))
}

fn bounds<'a>(points: impl Iterator<Item = &'a [f32; 3]>) -> Option<([f32; 3], [f32; 3])> {
    points.fold(None, |acc, p| {
        Some(match acc {
            None => (*p, *p),
            Some((lo, hi)) => ([0, 1, 2].map(|a| lo[a].min(p[a])), [0, 1, 2].map(|a| hi[a].max(p[a]))),
        })
    })
}

/// Where the scene's local origin lands in the world, and how high: the ground
/// plane's top sits `GROUND_CLEARANCE_M` above the highest terrain sample under
/// (and one grid cell around) the footprint, so terrain never pokes through.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CalibrationPlacement {
    pub anchor_xyz_m: [f64; 3],
}

impl CalibrationPlacement {
    pub fn for_world(world: &WorldArtifact) -> Result<Self, GraphicsContractError> {
        let layout = &world.body.authored_layout;
        let fields = &world.body.fields;
        let n = fields.resolution;
        if n < 2 || fields.heights_m.len() != n * n {
            return Err(GraphicsContractError::provenance("calibration host world has no height grid"));
        }
        let [ax, az] = CALIBRATION_ANCHOR_XZ_M;
        // Yaw 180°: local x → −x, local z → −z.
        let (x0, x1) = (ax - GROUND_LOCAL_X_M[1], ax - GROUND_LOCAL_X_M[0]);
        let (z0, z1) = (az - GROUND_LOCAL_Z_M[1], az - GROUND_LOCAL_Z_M[0]);
        if x0 < -layout.width_m / 2.0 || x1 > layout.width_m / 2.0 || z0 < -layout.length_m / 2.0 || z1 > layout.length_m / 2.0 {
            return Err(GraphicsContractError::provenance("calibration footprint leaves the host world"));
        }
        let cells = (n - 1) as f64;
        let column = |x: f64| ((x / layout.width_m + 0.5) * cells).clamp(0.0, cells);
        let row = |z: f64| ((0.5 - z / layout.length_m) * cells).clamp(0.0, cells);
        let (c0, c1) = (column(x0).floor() as usize, column(x1).ceil() as usize);
        let (r0, r1) = (row(z1).floor() as usize, row(z0).ceil() as usize);
        let mut highest = f64::NEG_INFINITY;
        for r in r0.saturating_sub(1)..=(r1 + 1).min(n - 1) {
            for c in c0.saturating_sub(1)..=(c1 + 1).min(n - 1) {
                highest = highest.max(fields.heights_m[r * n + c]);
            }
        }
        Ok(Self { anchor_xyz_m: [ax, highest + GROUND_CLEARANCE_M, az] })
    }

    fn point(&self, local: [f32; 3]) -> [f32; 3] {
        [
            (self.anchor_xyz_m[0] - f64::from(local[0])) as f32,
            (self.anchor_xyz_m[1] + f64::from(local[1])) as f32,
            (self.anchor_xyz_m[2] - f64::from(local[2])) as f32,
        ]
    }
}

/// One scene object per conditioned mesh, all at the placement anchor with a
/// 180° yaw, sealed against the runtime receipt and render package.
pub fn calibration_scene(
    world: &WorldArtifact,
    runtime_receipt: &AssetPreparationReceipt,
    render_package: &RenderAssetPackage,
    placement: CalibrationPlacement,
) -> Result<SceneArtifact, GraphicsContractError> {
    let runtime_package = runtime_receipt
        .package
        .as_ref()
        .ok_or_else(|| GraphicsContractError::provenance("calibration runtime receipt is not ready"))?;
    let objects = render_package
        .meshes
        .iter()
        .map(|mesh| {
            let structural = matches!(mesh.mesh_id.as_str(), "ground_plane" | "plinth");
            SceneObject {
                schema_version: SCENE_OBJECT_SCHEMA.into(),
                object_id: format!("calibration-{}", mesh.mesh_id),
                source_asset_id: runtime_receipt.source_identity.asset_id.clone(),
                source_asset_sha256: runtime_receipt.source_identity.source_sha256.clone(),
                asset_receipt_sha256: runtime_receipt.receipt_sha256.clone(),
                runtime_package_id: runtime_package.package_id.clone(),
                mesh_id: mesh.mesh_id.clone(),
                render_package_id: Some(render_package.package_id.clone()),
                render_mesh_id: Some(mesh.mesh_id.clone()),
                semantic_role: if structural { "calibration staging".into() } else { "calibration specimen".into() },
                gameplay_refs: Vec::new(),
                transform: SceneTransform {
                    translation_xyz_m: placement.anchor_xyz_m,
                    rotation_xyzw: [0.0, 1.0, 0.0, 0.0],
                    scale_xyz: [1.0, 1.0, 1.0],
                },
                collision: if mesh.mesh_id == "ground_plane" {
                    CollisionPolicy::Static { shape: wge_project_ledger::SceneCollisionShape::Box }
                } else {
                    CollisionPolicy::None
                },
                material_assignments: vec![MaterialAssignment {
                    slot: 0,
                    material_id: mesh.material_id.clone(),
                    material_artifact_id: None,
                    material_sha256: None,
                }],
                importance: if structural { SceneImportance::Background } else { SceneImportance::Landmark },
                lod: SceneLodPolicy {
                    levels: vec![SceneLodLevel { level: 0, mesh_id: mesh.mesh_id.clone(), switch_below_fraction: 0.0 }],
                },
                visibility: SceneVisibilityPolicy {
                    renderable: true,
                    casts_shadows: true,
                    receives_shadows: true,
                    max_distance_m: 500.0,
                },
                provenance: SceneObjectProvenance {
                    authoring_id: "calibration1".into(),
                    source_refs: vec!["tools/build_calibration_glb.py".into(), "tools/calibration_materials/calibration1.json".into()],
                    provider_job_id: None,
                },
            }
        })
        .collect();
    let body = SceneArtifactBody {
        schema_version: SCENE_ARTIFACT_SCHEMA.into(),
        scene_id: "scene-calibration1".into(),
        world_artifact_id: world.artifact_id.clone(),
        world_artifact_sha256: world.artifact_sha256.clone(),
        objects,
        provenance: SceneProvenance {
            construction_plan_id: "calibration1".into(),
            authoring_digest: format!(
                "sha256:{}",
                runtime_receipt.source_identity.source_sha256.trim_start_matches("sha256:")
            ),
            source_refs: vec!["calibration1.glb".into()],
        },
    };
    seal_scene_with_render_assets(body, std::slice::from_ref(runtime_receipt), std::slice::from_ref(render_package))
        .map_err(|error| GraphicsContractError::provenance(format!("calibration scene does not seal: {error}")))
}

/// The derived inspection views. Cameras are presentation, not scene state:
/// the bound-scene seam already re-applies a Rust-derived camera.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "column", rename_all = "snake_case")]
pub enum CalibrationView {
    /// The whole row, 30 mm-equivalent.
    Row,
    /// One column (or "calibration" for the cards and balls), 50 mm-equivalent,
    /// the 1 m sphere ≈ 40% of frame height.
    Close(String),
    /// 10° over one column's slab, along its width.
    Grazing(String),
}

impl CalibrationView {
    pub fn name(&self) -> String {
        match self {
            Self::Row => "row".into(),
            Self::Close(column) => format!("close-{column}"),
            Self::Grazing(column) => format!("grazing-{column}"),
        }
    }

    /// Every view the scene supports: the row, a close view per column and for
    /// the calibration group, and a grazing view per column that has a slab.
    pub fn all() -> Vec<Self> {
        let mut views = vec![Self::Row, Self::Close("calibration".into())];
        views.extend(CALIBRATION_COLUMNS.iter().map(|c| Self::Close((*c).into())));
        views.extend(CALIBRATION_COLUMNS.iter().filter(|c| **c != "emissive").map(|c| Self::Grazing((*c).into())));
        views
    }

    pub fn camera(
        &self,
        package: &RenderAssetPackage,
        placement: CalibrationPlacement,
    ) -> Result<GraphicsCamera, GraphicsContractError> {
        let (position, target, fov_y_degrees, width_px, height_px) = match self {
            // Up and in, looking down on the row: from 21 m back the camera stood
            // outside the host world (terrain ends ~9.5 m in front of the row)
            // and half the frame was void.
            // 2.4:1 keeps the 30 mm-equivalent vertical field while spanning the
            // 31 m row (~34 m at ~18 m distance).
            Self::Row => ([0.0, 15.0, 9.0], [0.0, 0.4, 0.0], 43.6, 1536, 640),
            Self::Close(column) => {
                let x = column_center_x(package, column)?;
                ([x, 1.25, 6.0], [x, 0.75, 0.0], 27.0, 960, 640)
            }
            Self::Grazing(column) => {
                let slab = format!("{column}_slab");
                let (lo, hi) = mesh_bounds(package, &slab)?;
                let lean = SLAB_LEAN_DEG.to_radians();
                let center = [(lo[0] + hi[0]) * 0.5, PLINTH_TOP_M + 0.5 * lean.cos(), -0.6 - 0.5 * lean.sin()];
                let normal = [0.0, lean.sin(), lean.cos()];
                let graze = 10f32.to_radians();
                let distance = 1.6;
                let position = [
                    center[0] + distance * graze.cos(),
                    center[1] + distance * graze.sin() * normal[1],
                    center[2] + distance * graze.sin() * normal[2],
                ];
                (position, center, 40.0, 960, 640)
            }
        };
        let world_position = placement.point(position);
        let world_target = placement.point(target);
        let forward = normalize([0, 1, 2].map(|a| world_target[a] - world_position[a]));
        let right = normalize(cross(forward, [0.0, 1.0, 0.0]));
        let up = normalize(cross(right, forward));
        Ok(GraphicsCamera {
            camera_id: format!("calibration-{}", self.name()),
            projection: CameraProjection::Perspective { fov_y_degrees },
            position_xyz_m: world_position,
            forward_xyz: forward,
            up_xyz: up,
            near_plane_m: 0.05,
            far_plane_m: 400.0,
            width_px,
            height_px,
        })
    }
}

fn column_center_x(package: &RenderAssetPackage, column: &str) -> Result<f32, GraphicsContractError> {
    let prefixes: Vec<String> = if column == "calibration" {
        vec!["card_".into(), "ball_".into(), "checker_".into()]
    } else if CALIBRATION_COLUMNS.contains(&column) {
        vec![format!("{column}_")]
    } else {
        return Err(GraphicsContractError::malformed(format!("unknown calibration column {column}")));
    };
    let points = package
        .meshes
        .iter()
        .filter(|mesh| {
            prefixes.iter().any(|prefix| {
                mesh.mesh_id.starts_with(prefix.as_str())
                    && (column != "metal" || !mesh.mesh_id.starts_with("rough_metal"))
            })
        })
        .flat_map(|mesh| mesh.positions_m.iter());
    let (lo, hi) = bounds(points).ok_or_else(|| {
        GraphicsContractError::provenance(format!("calibration asset has no meshes for column {column}"))
    })?;
    Ok((lo[0] + hi[0]) * 0.5)
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}

/// Pixel centre of a local-frame point in a camera's capture, or `None` when it
/// is behind the camera or off frame. Used to sample the grey card.
pub fn project_local_point(
    camera: &GraphicsCamera,
    placement: CalibrationPlacement,
    local: [f32; 3],
) -> Option<(u32, u32)> {
    let CameraProjection::Perspective { fov_y_degrees } = camera.projection else {
        return None;
    };
    let p = placement.point(local);
    let rel = [0, 1, 2].map(|a| p[a] - camera.position_xyz_m[a]);
    let forward = camera.forward_xyz;
    let right = normalize(cross(forward, camera.up_xyz));
    let up = normalize(cross(right, forward));
    let depth = rel[0] * forward[0] + rel[1] * forward[1] + rel[2] * forward[2];
    if depth <= camera.near_plane_m {
        return None;
    }
    let tan_y = (fov_y_degrees.to_radians() * 0.5).tan();
    let tan_x = tan_y * camera.width_px as f32 / camera.height_px as f32;
    let sx = (rel[0] * right[0] + rel[1] * right[1] + rel[2] * right[2]) / (depth * tan_x);
    let sy = (rel[0] * up[0] + rel[1] * up[1] + rel[2] * up[2]) / (depth * tan_y);
    let px = (sx + 1.0) * 0.5 * camera.width_px as f32;
    let py = (1.0 - sy) * 0.5 * camera.height_px as f32;
    (px >= 0.0 && py >= 0.0 && px < camera.width_px as f32 && py < camera.height_px as f32)
        .then_some((px as u32, py as u32))
}

/// The 18% grey card's centre in the local frame: x is the calibration group's
/// centre, the card leans back 15° from its bottom edge at z = −0.6.
pub fn grey_card_local_center(package: &RenderAssetPackage) -> Result<[f32; 3], GraphicsContractError> {
    let (lo, hi) = mesh_bounds(package, "card_grey_018")?;
    let lean = SLAB_LEAN_DEG.to_radians();
    Ok([(lo[0] + hi[0]) * 0.5, PLINTH_TOP_M + 0.45 * lean.cos(), -0.6 - 0.45 * lean.sin()])
}

/// Middle grey on screen: 0.18 linear before tone mapping, through the
/// renderer's ACES fit (`LavaAdapter._tone_map`) and the sRGB transfer:
/// f(0.18) = 0.2669 → sRGB 0.5534 → 141.1 of 255.
pub const GREY_CARD_TARGET_SRGB8: f32 = 141.1;
