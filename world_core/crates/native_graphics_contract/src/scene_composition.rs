//! Composition of a validated semantic scene and conditioned render assets
//! into a detached, content-addressed graphics packet.

use std::collections::{BTreeMap, BTreeSet};

use wge_project_ledger::{
    SceneArtifact, SceneImportance, SceneObjectProjection, SceneTransform,
    project_scene_for_graphics, validate_scene_artifact,
};

use crate::calibration::CalibrationRig;
use crate::{
    GraphicsAssetProjection, GraphicsContractError, GraphicsScenePacket, GraphicsScenePacketBody,
    InstanceImportance, InstancePacket, Transform3d, seal_scene_packet,
    validate_graphics_asset_projection,
};

/// Lower a validated semantic scene and its already-conditioned render assets
/// into the existing graphics packet vocabulary. The input body supplies the
/// world terrain/camera/lights/environment; this function only appends the
/// bound scene objects and records the scene identity.
pub fn compose_bound_scene(
    mut body: GraphicsScenePacketBody,
    scene: &SceneArtifact,
    assets: &[GraphicsAssetProjection],
) -> Result<GraphicsScenePacket, GraphicsContractError> {
    compose_bound_scene_with_camera(&mut body, scene, assets, None)
}

/// Compose a bound scene while applying an explicit, typed inspection camera.
/// The camera is part of the sealed packet but remains a derived view rather
/// than semantic world state. This is used for close asset inspection so a
/// certified asset is judged on its own material/detail response instead of
/// being hidden inside a generic calibration composition.
pub fn compose_bound_scene_with_camera(
    body: &mut GraphicsScenePacketBody,
    scene: &SceneArtifact,
    assets: &[GraphicsAssetProjection],
    camera: Option<&crate::GraphicsCamera>,
) -> Result<GraphicsScenePacket, GraphicsContractError> {
    compose_bound_scene_with_view(body, scene, assets, camera, None)
}

/// `compose_bound_scene_with_camera` plus an enumerated calibration light rig
/// (CALIBRATION-1). The rig replaces the base projection's lights, environment
/// and render policy with its constants; it is a name, not free parameters, so
/// the supervisor re-applies it and requires an exact match exactly as it does
/// for the camera. `None` is the camera-only composition, byte-identical.
pub fn compose_bound_scene_with_view(
    body: &mut GraphicsScenePacketBody,
    scene: &SceneArtifact,
    assets: &[GraphicsAssetProjection],
    camera: Option<&crate::GraphicsCamera>,
    rig: Option<CalibrationRig>,
) -> Result<GraphicsScenePacket, GraphicsContractError> {
    let mut body = body.clone();
    validate_scene_artifact(scene).map_err(|error| {
        GraphicsContractError::provenance(format!("scene artifact is not valid: {error}"))
    })?;
    let scene_projection = project_scene_for_graphics(scene).map_err(|error| {
        GraphicsContractError::provenance(format!("scene projection is not valid: {error}"))
    })?;
    if body.world_artifact_id != scene.body.world_artifact_id
        || body.world_artifact_sha256 != scene.body.world_artifact_sha256
    {
        return Err(GraphicsContractError::provenance(
            "graphics body is bound to a different world artifact",
        ));
    }

    let mut asset_by_package = BTreeMap::<&str, &GraphicsAssetProjection>::new();
    for asset in assets {
        validate_graphics_asset_projection(asset)?;
        if asset_by_package
            .insert(asset.render_package_id.as_str(), asset)
            .is_some()
        {
            return Err(GraphicsContractError::provenance(format!(
                "duplicate graphics asset projection {}",
                asset.render_package_id
            )));
        }
    }

    let referenced_packages = scene_projection
        .objects
        .iter()
        .filter_map(|object| object.render_package_id.as_deref())
        .collect::<BTreeSet<_>>();
    if assets
        .iter()
        .any(|asset| !referenced_packages.contains(asset.render_package_id.as_str()))
    {
        return Err(GraphicsContractError::provenance(
            "graphics asset projection list contains an unreferenced package",
        ));
    }

    let mut namespaces = BTreeMap::<&str, AssetNamespace>::new();
    for package_id in &referenced_packages {
        let asset = asset_by_package.get(package_id).ok_or_else(|| {
            GraphicsContractError::provenance(format!(
                "scene references missing graphics asset projection {package_id}"
            ))
        })?;
        let namespace = AssetNamespace::new(asset);
        append_asset_resources(&mut body, &namespace, asset)?;
        namespaces.insert(*package_id, namespace);
    }

    for object in &scene_projection.objects {
        let Some(package_id) = object.render_package_id.as_deref() else {
            continue;
        };
        let asset = asset_by_package.get(package_id).ok_or_else(|| {
            GraphicsContractError::provenance(format!(
                "scene object {} references missing graphics asset projection {}",
                object.object_id, package_id
            ))
        })?;
        let scene_object = scene
            .body
            .objects
            .iter()
            .find(|candidate| candidate.object_id == object.object_id)
            .ok_or_else(|| {
                GraphicsContractError::provenance(format!(
                    "scene projection object {} is absent from the canonical scene",
                    object.object_id
                ))
            })?;
        if asset.source_asset_id != scene_object.source_asset_id
            || asset.source_asset_sha256 != scene_object.source_asset_sha256
        {
            return Err(GraphicsContractError::provenance(format!(
                "scene object {} and graphics asset {} disagree on source identity",
                object.object_id, package_id
            )));
        }
        let namespace = namespaces.get(package_id).expect("namespace was inserted");
        append_scene_instance(&mut body, object, asset, namespace)?;
    }

    match (
        body.scene_artifact_id.as_deref(),
        body.scene_artifact_sha256.as_deref(),
    ) {
        (None, None) => {}
        (Some(scene_id), Some(scene_sha256))
            if scene_id == scene.artifact_id && scene_sha256 == scene.artifact_sha256 => {}
        _ => {
            return Err(GraphicsContractError::provenance(
                "graphics body is already bound to a different or partial scene artifact",
            ));
        }
    }
    body.scene_artifact_id = Some(scene.artifact_id.clone());
    body.scene_artifact_sha256 = Some(scene.artifact_sha256.clone());
    if let Some(camera) = camera {
        body.packet_id = format!("{}-{}", body.packet_id, camera.camera_id);
        body.camera = camera.clone();
        body.capture.camera_id = camera.camera_id.clone();
        body.capture.width_px = camera.width_px;
        body.capture.height_px = camera.height_px;
    }
    if let Some(rig) = rig {
        body.packet_id = format!("{}-rig-{}", body.packet_id, rig.name());
        rig.apply(&mut body)?;
    }
    seal_scene_packet(body)
}

pub(crate) struct AssetNamespace {
    mesh_ids: BTreeMap<String, String>,
    material_ids: BTreeMap<String, String>,
    texture_ids: BTreeMap<String, String>,
}

impl AssetNamespace {
    pub(crate) fn new(asset: &GraphicsAssetProjection) -> Self {
        let prefix = format!("{}::", asset.render_package_id);
        Self {
            mesh_ids: asset
                .meshes
                .iter()
                .map(|mesh| {
                    (
                        mesh.packet.mesh_id.clone(),
                        format!("{prefix}{}", mesh.packet.mesh_id),
                    )
                })
                .collect(),
            material_ids: asset
                .materials
                .iter()
                .map(|material| {
                    (
                        material.material_id.clone(),
                        format!("{prefix}{}", material.material_id),
                    )
                })
                .collect(),
            texture_ids: asset
                .textures
                .iter()
                .map(|texture| {
                    (
                        texture.reference.texture_id.clone(),
                        format!("{prefix}{}", texture.reference.texture_id),
                    )
                })
                .collect(),
        }
    }

    pub(crate) fn mesh(&self, local: &str) -> Result<&String, GraphicsContractError> {
        self.mesh_ids.get(local).ok_or_else(|| {
            GraphicsContractError::provenance(format!("unknown conditioned mesh {local}"))
        })
    }

    pub(crate) fn material(&self, local: &str) -> Result<&String, GraphicsContractError> {
        self.material_ids.get(local).ok_or_else(|| {
            GraphicsContractError::provenance(format!("unknown conditioned material {local}"))
        })
    }

    fn texture(&self, local: &str) -> Result<&String, GraphicsContractError> {
        self.texture_ids.get(local).ok_or_else(|| {
            GraphicsContractError::provenance(format!("unknown conditioned texture {local}"))
        })
    }
}

pub(crate) fn append_asset_resources(
    body: &mut GraphicsScenePacketBody,
    namespace: &AssetNamespace,
    asset: &GraphicsAssetProjection,
) -> Result<(), GraphicsContractError> {
    for material in &asset.materials {
        let mut material = material.clone();
        material.material_id = namespace.material(&material.material_id)?.clone();
        material.texture_ids = material
            .texture_ids
            .iter()
            .map(|texture_id| namespace.texture(texture_id).cloned())
            .collect::<Result<Vec<_>, _>>()?;
        material.normal_texture_id =
            qualify_optional_texture(material.normal_texture_id.as_deref(), namespace)?;
        material.roughness_texture_id =
            qualify_optional_texture(material.roughness_texture_id.as_deref(), namespace)?;
        material.occlusion_texture_id =
            qualify_optional_texture(material.occlusion_texture_id.as_deref(), namespace)?;
        material.emissive_texture_id =
            qualify_optional_texture(material.emissive_texture_id.as_deref(), namespace)?;
        body.materials.push(material);
    }
    for texture in &asset.textures {
        let mut texture = texture.reference.clone();
        texture.texture_id = namespace.texture(&texture.texture_id)?.clone();
        body.textures.push(texture);
    }
    for mesh in &asset.meshes {
        let mut mesh = mesh.packet.clone();
        mesh.mesh_id = namespace.mesh(&mesh.mesh_id)?.clone();
        mesh.material_id = namespace.material(&mesh.material_id)?.clone();
        body.meshes.push(mesh);
    }
    Ok(())
}

fn qualify_optional_texture(
    texture_id: Option<&str>,
    namespace: &AssetNamespace,
) -> Result<Option<String>, GraphicsContractError> {
    texture_id
        .map(|texture_id| namespace.texture(texture_id).cloned())
        .transpose()
}

fn append_scene_instance(
    body: &mut GraphicsScenePacketBody,
    object: &SceneObjectProjection,
    asset: &GraphicsAssetProjection,
    namespace: &AssetNamespace,
) -> Result<(), GraphicsContractError> {
    let local_mesh_id = object
        .render_mesh_id
        .as_deref()
        .unwrap_or(object.mesh_id.as_str());
    let mesh = asset
        .meshes
        .iter()
        .find(|mesh| mesh.packet.mesh_id == local_mesh_id)
        .ok_or_else(|| {
            GraphicsContractError::provenance(format!(
                "scene object {} references unknown render mesh {}",
                object.object_id, local_mesh_id
            ))
        })?;
    let material_id = object
        .material_assignments
        .iter()
        .find(|assignment| assignment.material_id == mesh.packet.material_id)
        .map(|assignment| assignment.material_id.as_str())
        .ok_or_else(|| {
            GraphicsContractError::provenance(format!(
                "scene object {} has no material assignment for conditioned material {}",
                object.object_id, mesh.packet.material_id
            ))
        })?;
    body.instances.push(InstancePacket {
        instance_id: format!("scene::{}", object.object_id),
        mesh_id: namespace.mesh(local_mesh_id)?.clone(),
        material_id: namespace.material(material_id)?.clone(),
        importance: project_importance(object.importance),
        transform: lower_transform(&object.transform)?,
    });
    Ok(())
}

fn project_importance(importance: wge_project_ledger::SceneImportance) -> InstanceImportance {
    match importance {
        SceneImportance::Background => InstanceImportance::Background,
        SceneImportance::Landmark => InstanceImportance::Landmark,
        SceneImportance::GameplayCritical => InstanceImportance::GameplayCritical,
    }
}

fn lower_transform(transform: &SceneTransform) -> Result<Transform3d, GraphicsContractError> {
    let translation_xyz_m = transform.translation_xyz_m.map(|value| {
        if value.is_finite() && value.abs() <= crate::MAX_NATIVE_COORDINATE_M as f64 {
            value as f32
        } else {
            f32::NAN
        }
    });
    let scale_xyz = transform.scale_xyz.map(|value| {
        if value.is_finite() && value > 0.0 && value <= crate::MAX_NATIVE_COORDINATE_M as f64 {
            value as f32
        } else {
            f32::NAN
        }
    });
    let length = transform
        .rotation_xyzw
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    if !translation_xyz_m.iter().all(|value| value.is_finite())
        || !scale_xyz.iter().all(|value| value.is_finite())
        || !length.is_finite()
        || length <= f64::EPSILON
    {
        return Err(GraphicsContractError::malformed(
            "scene object transform is outside the native graphics envelope",
        ));
    }
    let rotation_xyzw = transform.rotation_xyzw.map(|value| (value / length) as f32);
    Ok(Transform3d {
        translation_xyz_m,
        rotation_xyzw,
        scale_xyz,
    })
}
