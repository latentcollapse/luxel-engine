//! CONVERGE-2 N-5: the hero kit (WGE_CONVERGE2_CONTRACTS.md §2).
//!
//! The kit is an explicit, digest-verified input to Campaign 2 lowering, the
//! same seam N-4 uses for terrain layers: `load_kit_set` reads the committed
//! lock (`tools/kit/kit1.lock.json`, written by `tools/build_kit.py`), checks
//! every GLB's size and sha256, conditions it through the same render request
//! as the calibration scene (any finding is refused), and projects it. Lowering
//! never reads files; the supervisor re-derives the authorized packet with the
//! same `KitSet`.
//!
//! `apply_kit` replaces the procedural shrine and tree balls of the converge
//! world with the kit: the ruin at the shrine site, trees at the authored tree
//! spots, rocks around the ruin, ferns as ground cover. Everything placed is a
//! constant here, relative to the shrine anchor, so a packet is a pure function
//! of (world, view, arm, terrain layers, kit).

use std::collections::BTreeSet;
use std::path::Path;

use serde::Deserialize;
use wge_asset_contract::{RenderPreparationStatus, condition_render_asset};

use crate::asset_projection::{
    GraphicsAssetProjection, project_render_asset, validate_graphics_asset_projection,
};
use crate::scene_composition::{AssetNamespace, append_asset_resources};
use crate::scatter::Placement;
use crate::{
    GraphicsContractError, GraphicsScenePacketBody, InstanceImportance, InstancePacket, InstanceVariation, Transform3d,
    sha256_prefixed,
};

pub const KIT_LOCK_SCHEMA: &str = "wge.kit-lock/v1";
/// The assets a kit must provide, by name.
pub const KIT_ASSETS: [&str; 5] = ["ruin", "rock_a", "rock_b", "tree", "fern"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LockAsset {
    glb: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Lock {
    schema_version: String,
    set_id: String,
    blender: String,
    assets: std::collections::BTreeMap<String, LockAsset>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct KitAsset {
    pub name: String,
    pub glb_sha256: String,
    pub projection: GraphicsAssetProjection,
}

/// A verified, conditioned kit. `lock_sha256` binds the whole set.
#[derive(Clone, Debug, PartialEq)]
pub struct KitSet {
    pub set_id: String,
    pub lock_sha256: String,
    pub assets: Vec<KitAsset>,
}

impl KitSet {
    pub fn asset(&self, name: &str) -> Result<&KitAsset, GraphicsContractError> {
        self.assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or_else(|| GraphicsContractError::provenance(format!("kit {} has no asset {name}", self.set_id)))
    }
}

/// Load and verify a kit from its lock. `repo_root` resolves the lock's
/// repo-relative GLB paths.
pub fn load_kit_set(lock_path: &Path, repo_root: &Path) -> Result<KitSet, GraphicsContractError> {
    let text = std::fs::read(lock_path)
        .map_err(|error| GraphicsContractError::provenance(format!("kit lock {}: {error}", lock_path.display())))?;
    let lock: Lock = serde_json::from_slice(&text)
        .map_err(|error| GraphicsContractError::malformed(format!("kit lock {} is malformed: {error}", lock_path.display())))?;
    if lock.schema_version != KIT_LOCK_SCHEMA {
        return Err(GraphicsContractError::malformed(format!("kit lock schema {} is unsupported", lock.schema_version)));
    }
    let names: BTreeSet<&str> = lock.assets.keys().map(String::as_str).collect();
    if names != KIT_ASSETS.into_iter().collect() {
        return Err(GraphicsContractError::malformed(format!(
            "kit {} provides {names:?}; expected exactly {KIT_ASSETS:?}",
            lock.set_id
        )));
    }
    let _ = &lock.blender;
    let mut assets = Vec::with_capacity(KIT_ASSETS.len());
    for name in KIT_ASSETS {
        let entry = &lock.assets[name];
        let path = repo_root.join(&entry.glb);
        let glb = std::fs::read(&path).map_err(|error| {
            GraphicsContractError::provenance(format!(
                "kit asset {name} at {}: {error} (build it with tools/build_kit.py)",
                path.display()
            ))
        })?;
        let digest = sha256_prefixed(&glb);
        if glb.len() as u64 != entry.bytes || digest != entry.sha256 {
            return Err(GraphicsContractError::provenance(format!(
                "kit asset {name} is {} bytes {digest}; the lock pins {} bytes {}",
                glb.len(),
                entry.bytes,
                entry.sha256
            )));
        }
        let receipt = condition_render_asset(&glb, &crate::calibration::render_request())
            .map_err(|error| GraphicsContractError::provenance(format!("kit asset {name} conditioning: {error}")))?;
        if receipt.status != RenderPreparationStatus::Ready || !receipt.findings.is_empty() {
            return Err(GraphicsContractError::provenance(format!(
                "kit asset {name} failed render conditioning: {:?}",
                receipt.findings
            )));
        }
        let package = receipt.package.as_ref().expect("a ready receipt has a package");
        let projection = project_render_asset(package)?;
        validate_graphics_asset_projection(&projection)?;
        assets.push(KitAsset { name: name.to_owned(), glb_sha256: digest, projection });
    }
    Ok(KitSet { set_id: lock.set_id, lock_sha256: sha256_prefixed(&text), assets })
}

/// Instance ids of the procedural content the kit replaces.
const REPLACED_INSTANCE_PREFIXES: [&str; 4] =
    ["campaign2-hero-", "campaign2-trunk-", "campaign2-crown-", "campaign2-lobe-"];
/// Resources that only the replaced instances use; pruned when unreferenced.
const REPLACED_RESOURCE_PREFIXES: [&str; 3] = ["campaign2-hero", "campaign2-foliage", "campaign2-bark"];

/// Ruin: scale 0.8 (the gate is ~5.7 m tall), yawed so the arch faces the
/// close / medium / wide cameras, which all look from the same quadrant, and
/// the broken wall end falls away from the wet pool.
const RUIN_YAW_DEG: f32 = 126.0;
const RUIN_SCALE: f32 = 0.8;
/// Rocks: (x, z offset from the shrine anchor, asset, yaw degrees, scale).
const ROCKS: [(f32, f32, &str, f32, f32); 5] = [
    (3.0, 2.0, "rock_a", 20.0, 0.8),
    (-2.5, -2.8, "rock_b", 140.0, 1.0),
    (8.5, -1.5, "rock_b", 75.0, 0.7),
    (-6.3, 4.4, "rock_a", 300.0, 0.7),
    (1.5, 5.0, "rock_a", 250.0, 0.6),
];
/// Rocks sit this far into the ground so they do not perch on it.
const ROCK_SINK_M: f32 = 0.18;
/// Ferns: (x, z offset from the shrine anchor, yaw degrees).
const FERNS: [(f32, f32, f32); 12] = [
    (3.8, 3.4, 10.0),
    (2.0, 2.9, 95.0),
    (-3.3, -3.6, 200.0),
    (-1.6, -3.4, 290.0),
    (9.3, -0.6, 45.0),
    (-7.0, -4.0, 130.0),
    (-10.2, 1.8, 250.0),
    (5.2, -6.0, 15.0),
    (10.8, -4.0, 170.0),
    (-7.5, 7.6, 320.0),
    (12.0, 7.2, 60.0),
    (-2.4, 5.6, 230.0),
];

/// Fallen blocks (kit2's ruin, CONVERGE-3 R-1) sink this fraction of their
/// height into the ground under them, wherever that ground is.
const FALLEN_SINK_FRACTION: f32 = 0.15;

/// Rotate `v` by the unit quaternion `q` (x, y, z, w).
fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    let t = [2.0 * (y * v[2] - z * v[1]), 2.0 * (z * v[0] - x * v[2]), 2.0 * (x * v[1] - y * v[0])];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

fn yaw(degrees: f32) -> [f32; 4] {
    let half = degrees.to_radians() * 0.5;
    [0.0, half.sin(), 0.0, half.cos()]
}

/// Footprint discs (centre x, z; radius) of the ruin's two standing pieces at
/// `anchor`: the gate on the anchor, the wall end at its layout offset
/// (`tools/build_kit.py` RUIN_PIECES: 0.3, 8.3 m), both under the ruin's yaw
/// and scale. Scatter keeps ground cover and rocks out of them.
pub fn ruin_footprints(anchor: [f32; 3]) -> Vec<([f32; 2], f32)> {
    let end = rotate(yaw(RUIN_YAW_DEG), [0.3 * RUIN_SCALE, 0.0, 8.3 * RUIN_SCALE]);
    vec![
        ([anchor[0], anchor[2]], 3.9 * RUIN_SCALE),
        ([anchor[0] + end[0], anchor[2] + end[2]], 3.3 * RUIN_SCALE),
    ]
}

/// Where the kit's trees, rocks and ferns go.
pub enum KitLayout<'a> {
    /// converge2: the authored tree spots (x, z, scale) and the `ROCKS` and
    /// `FERNS` constants, exactly as N-5 placed them.
    Authored { trees: &'a [(f32, f32, f32)] },
    /// converge3 (N-7): scattered placements, each with its own tint.
    Scattered(&'a ScatteredLayout),
}

/// N-7 placements per kit asset (`crate::scatter`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScatteredLayout {
    pub trees: Vec<Placement>,
    pub rocks: Vec<(&'static str, Placement)>,
    pub ferns: Vec<Placement>,
}

/// The kit's half of a converge2/converge3 packet. `ground(x, z)` is the
/// terrain height at a world position.
pub fn apply_kit(
    body: &mut GraphicsScenePacketBody,
    kit: &KitSet,
    anchor: [f32; 3],
    layout: KitLayout<'_>,
    ground: impl Fn(f32, f32) -> f32,
) -> Result<(), GraphicsContractError> {
    body.instances.retain(|instance| {
        !REPLACED_INSTANCE_PREFIXES.iter().any(|prefix| instance.instance_id.starts_with(prefix))
    });
    prune_replaced_resources(body);

    let mut namespaces = std::collections::BTreeMap::new();
    for asset in &kit.assets {
        let namespace = AssetNamespace::new(&asset.projection);
        append_asset_resources(body, &namespace, &asset.projection)?;
        namespaces.insert(asset.name.as_str(), namespace);
    }
    // The ruin is placed first, so its meshes' instances start here.
    let ruin_start = body.instances.len();
    let mut place = |name: &str, tag: String, importance: InstanceImportance, translation: [f32; 3], rotation: [f32; 4], scale: f32, variation: Option<InstanceVariation>| -> Result<(), GraphicsContractError> {
        let asset = kit.asset(name)?;
        let namespace = &namespaces[name];
        for (k, mesh) in asset.projection.meshes.iter().enumerate() {
            body.instances.push(InstancePacket {
                instance_id: format!("kit-{tag}-{k:02}"),
                mesh_id: namespace.mesh(&mesh.packet.mesh_id)?.clone(),
                material_id: namespace.material(&mesh.packet.material_id)?.clone(),
                importance,
                transform: Transform3d { translation_xyz_m: translation, rotation_xyzw: rotation, scale_xyz: [scale; 3] },
                variation,
            });
        }
        Ok(())
    };
    let [ax, _, az] = anchor;
    place("ruin", "ruin".into(), InstanceImportance::Landmark, anchor, yaw(RUIN_YAW_DEG), RUIN_SCALE, None)?;
    match layout {
        KitLayout::Authored { trees } => {
            for (index, (x, z, scale)) in trees.iter().copied().enumerate() {
                let spin = index as f32 * 137.5;
                place("tree", format!("tree-{index:02}"), InstanceImportance::Background, [x, ground(x, z), z], yaw(spin), scale, None)?;
            }
            for (index, (dx, dz, asset, spin, scale)) in ROCKS.into_iter().enumerate() {
                let (x, z) = (ax + dx, az + dz);
                place(asset, format!("rock-{index:02}"), InstanceImportance::Background, [x, ground(x, z) - ROCK_SINK_M * scale, z], yaw(spin), scale, None)?;
            }
            for (index, (dx, dz, spin)) in FERNS.into_iter().enumerate() {
                let (x, z) = (ax + dx, az + dz);
                place("fern", format!("fern-{index:02}"), InstanceImportance::Background, [x, ground(x, z), z], yaw(spin), 1.0, None)?;
            }
        }
        KitLayout::Scattered(scattered) => {
            let tinted = |p: &Placement| Some(InstanceVariation { tint_rgb: p.tint_rgb });
            for (index, p) in scattered.trees.iter().enumerate() {
                place("tree", format!("tree-s{index:04}"), InstanceImportance::Background, p.translation, p.rotation_xyzw, p.scale, tinted(p))?;
            }
            for (index, (asset, p)) in scattered.rocks.iter().enumerate() {
                place(asset, format!("rock-s{index:04}"), InstanceImportance::Background, p.translation, p.rotation_xyzw, p.scale, tinted(p))?;
            }
            for (index, p) in scattered.ferns.iter().enumerate() {
                place("fern", format!("fern-s{index:04}"), InstanceImportance::Background, p.translation, p.rotation_xyzw, p.scale, tinted(p))?;
            }
        }
    }
    // The ruin's fallen blocks lie metres from its anchor, where the ground is
    // not at the anchor's height: each one is grounded on the terrain under it.
    let ruin = kit.asset("ruin")?;
    for (k, mesh) in ruin.projection.meshes.iter().enumerate() {
        if !mesh.packet.mesh_id.contains("_fallen_") {
            continue;
        }
        let instance = &mut body.instances[ruin_start + k];
        let t = &instance.transform;
        let world: Vec<[f32; 3]> = mesh
            .packet
            .positions_m
            .iter()
            .map(|p| {
                let r = rotate(t.rotation_xyzw, [p[0] * t.scale_xyz[0], p[1] * t.scale_xyz[1], p[2] * t.scale_xyz[2]]);
                [r[0] + t.translation_xyz_m[0], r[1] + t.translation_xyz_m[1], r[2] + t.translation_xyz_m[2]]
            })
            .collect();
        let lowest_gap = world.iter().map(|w| w[1] - ground(w[0], w[2])).fold(f32::INFINITY, f32::min);
        let (low, high) = world.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), w| (lo.min(w[1]), hi.max(w[1])));
        instance.transform.translation_xyz_m[1] += -FALLEN_SINK_FRACTION * (high - low) - lowest_gap;
    }
    Ok(())
}

/// Drop the replaced procedural meshes, materials and textures that nothing
/// references any more. Only resources under `REPLACED_RESOURCE_PREFIXES` are
/// candidates, so shared content (terrain, pool, beacon) is never touched.
fn prune_replaced_resources(body: &mut GraphicsScenePacketBody) {
    let candidate = |id: &str| REPLACED_RESOURCE_PREFIXES.iter().any(|prefix| id.starts_with(prefix));
    let used_meshes: BTreeSet<String> = body.instances.iter().map(|i| i.mesh_id.clone()).collect();
    body.meshes.retain(|mesh| !candidate(&mesh.mesh_id) || used_meshes.contains(&mesh.mesh_id));
    let mut used_materials: BTreeSet<String> = body.instances.iter().map(|i| i.material_id.clone()).collect();
    used_materials.extend(body.meshes.iter().map(|m| m.material_id.clone()));
    used_materials.insert(body.terrain.material_id.clone());
    if let Some(layers) = &body.terrain.layers {
        used_materials.extend(layers.layers.iter().map(|layer| layer.material_id.clone()));
    }
    body.materials.retain(|material| !candidate(&material.material_id) || used_materials.contains(&material.material_id));
    let mut used_textures = BTreeSet::new();
    for material in &body.materials {
        used_textures.extend(material.texture_ids.iter().cloned());
        for id in [&material.normal_texture_id, &material.roughness_texture_id, &material.occlusion_texture_id, &material.emissive_texture_id]
            .into_iter()
            .flatten()
        {
            used_textures.insert(id.clone());
        }
    }
    if let Some(layers) = &body.terrain.layers {
        used_textures.insert(layers.macro_texture_id.clone());
    }
    body.textures.retain(|texture| !candidate(&texture.texture_id) || used_textures.contains(&texture.texture_id));
}
