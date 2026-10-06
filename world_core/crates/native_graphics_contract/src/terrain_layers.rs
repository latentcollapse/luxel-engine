//! Terrain surface layers (N-4, `docs/world/converge/converge1-contracts.md` §4).
//!
//! WHY THIS EXISTS
//! ---------------
//! EDGE-1 showed the converge0 terrain carried almost no pixel-scale content:
//! one 160×160 procedural albedo stretched over 480 m scored at the level of a
//! render with every texture stripped. Real scanned ground cleared the
//! authority's floor ~10×, but exposed two contract gaps this module closes:
//!
//!  * **Tiling must be physical.** `terrain_surface.uv_repeat_scale_milli`
//!    counts repeats across the whole extent (capped at 32), so a 480 m world
//!    could not tile finer than 15 m and 2 m ground rendered ~4× too large.
//!    Each layer here carries `metres_per_repeat_milli` from its scan's
//!    measured size, independent of world extent.
//!  * **Mips are mandatory.** Unmipped scanned ground scored ~5000 bp of pure
//!    aliasing. Every layer texture is built with a full box-filtered chain.
//!
//! HOW CONTENT ENTERS
//! ------------------
//! WGE is source only (see `.gitignore`). A layer set is a committed MANIFEST
//! (URL, byte size, sha256, licence, physical size) whose files are fetched
//! into the ignored artifacts tree. [`load_terrain_layer_set`] re-verifies every
//! digest before a byte is decoded. The resulting [`TerrainLayerSet`] is an
//! explicit input to lowering and to supervisor authorization — exactly like a
//! bound scene's packages — so a lowering never reads files on its own and a
//! packet stays a pure function of (world, layer set).
//!
//! BLENDING
//! --------
//! Layer 0 is the base. Each later layer is painted over the running result
//! with weight = product of its PRESENT coverage terms (slope, height, macro
//! noise). A ramp `[a, b]` rises from a to b; with `a > b` it falls. That is
//! the whole rule set — small enough that the shader and this file cannot
//! disagree about it.

use std::path::Path;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

use crate::{
    AlphaMode, GraphicsContractError, GraphicsScenePacketBody, MaterialIntent, TextureColorSpace,
    TextureMipLevel, TexturePayload, TextureReference, canonical_json, sha256_prefixed,
};

/// Most layers the adapter's layered terrain shader blends (unrolled).
pub const MAX_TERRAIN_LAYERS: usize = 3;

// ---------------------------------------------------------------------------
// PACKET SCHEMA
// ---------------------------------------------------------------------------

/// The layered surface of one terrain. Optional on `TerrainPacket`; absent
/// means the historical single-material terrain, byte-identical.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TerrainLayers {
    pub set_id: String,
    /// Digest of the composed set (layer specs + every texture digest), so a
    /// packet names exactly which content it was built from.
    pub set_sha256: String,
    pub layers: Vec<TerrainLayer>,
    /// Tileable low-frequency noise (`Data`), sampled at the render policy's
    /// `terrain_surface.macro_frequency_milli` for macro albedo variation and
    /// for coverage terms that ask for it.
    pub macro_texture_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TerrainLayer {
    pub layer_id: String,
    /// Material carrying the layer's albedo / normal / roughness / occlusion
    /// maps. Its factors must be neutral: the scans carry the values.
    pub material_id: String,
    /// Physical size of one texture repeat, millimetres (from the scan).
    pub metres_per_repeat_milli: u32,
    /// `None` exactly for layer 0 (the base); `Some` for every later layer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage: Option<LayerCoverage>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LayerCoverage {
    /// Ramp over slope = 1 − n.y, basis points (0 flat, 10000 vertical).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slope_bp: Option<[i32; 2]>,
    /// Ramp over world height, millimetres.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_mm: Option<[i32; 2]>,
    /// Ramp over the macro noise value centred on `threshold_bp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macro_ramp: Option<MacroRamp>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MacroRamp {
    pub threshold_bp: i32,
    pub softness_bp: i32,
}

fn malformed(message: impl Into<String>) -> GraphicsContractError {
    GraphicsContractError::malformed(message)
}

fn ramp_is_valid(ramp: [i32; 2], low: i32, high: i32) -> bool {
    ramp[0] != ramp[1] && ramp.iter().all(|value| (low..=high).contains(value))
}

/// Validate a packet's terrain layers against its materials and textures.
pub fn validate_terrain_layers(
    layers: &TerrainLayers,
    materials: &[MaterialIntent],
    textures: &[TextureReference],
) -> Result<(), GraphicsContractError> {
    crate::valid_id(&layers.set_id, "terrain layer set_id")?;
    crate::valid_sha(&layers.set_sha256, "terrain layer set_sha256")?;
    if !(2..=MAX_TERRAIN_LAYERS).contains(&layers.layers.len()) {
        return Err(malformed(format!(
            "terrain carries {} layers; the layered shader blends 2..={MAX_TERRAIN_LAYERS}",
            layers.layers.len()
        )));
    }
    let texture = |id: &str| textures.iter().find(|texture| texture.texture_id == id);
    let mipped = |id: &str, space: TextureColorSpace, label: &str| -> Result<(), GraphicsContractError> {
        let found = texture(id)
            .ok_or_else(|| GraphicsContractError::provenance(format!("{label} texture {id} is absent")))?;
        if found.color_space != space {
            return Err(malformed(format!("{label} texture {id} must be {space:?}")));
        }
        // EDGE-1: unmipped scanned ground is aliasing, not detail.
        if found.mip_levels < 2 {
            return Err(malformed(format!("{label} texture {id} carries no mip chain")));
        }
        Ok(())
    };
    mipped(&layers.macro_texture_id, TextureColorSpace::Data, "terrain macro")?;
    let mut seen = Vec::new();
    for (index, layer) in layers.layers.iter().enumerate() {
        crate::valid_id(&layer.layer_id, "terrain layer_id")?;
        if seen.contains(&layer.layer_id) {
            return Err(malformed(format!("terrain layer {} is duplicated", layer.layer_id)));
        }
        seen.push(layer.layer_id.clone());
        if !(100..=100_000).contains(&layer.metres_per_repeat_milli) {
            return Err(malformed(format!(
                "terrain layer {} repeats every {} mm; expected 100..=100000",
                layer.layer_id, layer.metres_per_repeat_milli
            )));
        }
        match (index, &layer.coverage) {
            (0, Some(_)) => return Err(malformed("terrain layer 0 is the base and cannot carry coverage")),
            (0, None) => {}
            (_, None) => {
                return Err(malformed(format!("terrain layer {} needs a coverage rule", layer.layer_id)));
            }
            (_, Some(coverage)) => {
                if coverage.slope_bp.is_none() && coverage.height_mm.is_none() && coverage.macro_ramp.is_none() {
                    return Err(malformed(format!(
                        "terrain layer {} coverage has no terms; it would cover everything",
                        layer.layer_id
                    )));
                }
                if coverage.slope_bp.is_some_and(|ramp| !ramp_is_valid(ramp, 0, 10_000)) {
                    return Err(malformed(format!("terrain layer {} slope ramp is invalid", layer.layer_id)));
                }
                if coverage.height_mm.is_some_and(|ramp| !ramp_is_valid(ramp, -1_000_000_000, 1_000_000_000)) {
                    return Err(malformed(format!("terrain layer {} height ramp is invalid", layer.layer_id)));
                }
                if let Some(ramp) = coverage.macro_ramp
                    && (!(0..=10_000).contains(&ramp.threshold_bp) || !(1..=5_000).contains(&ramp.softness_bp))
                {
                    return Err(malformed(format!("terrain layer {} macro ramp is invalid", layer.layer_id)));
                }
            }
        }
        let material = materials
            .iter()
            .find(|material| material.material_id == layer.material_id)
            .ok_or_else(|| {
                GraphicsContractError::provenance(format!(
                    "terrain layer {} references unknown material {}",
                    layer.layer_id, layer.material_id
                ))
            })?;
        // The scans carry the values; a factor here would be a second, hidden
        // source of colour or roughness (the MD-1 failure, again).
        if material.base_color_rgba != [1.0, 1.0, 1.0, 1.0]
            || material.metallic != 0.0
            || material.roughness != 1.0
            || material.emissive_texture_id.is_some()
            || material.emissive_factor_rgb != [0.0, 0.0, 0.0]
        {
            return Err(malformed(format!(
                "terrain layer material {} must have neutral factors (white, dielectric, roughness 1, no emission)",
                material.material_id
            )));
        }
        let [albedo] = material.texture_ids.as_slice() else {
            return Err(malformed(format!("terrain layer material {} needs exactly one albedo", material.material_id)));
        };
        mipped(albedo, TextureColorSpace::Srgb, "terrain layer albedo")?;
        for (id, space, label) in [
            (&material.normal_texture_id, TextureColorSpace::NormalMap, "terrain layer normal"),
            (&material.roughness_texture_id, TextureColorSpace::Data, "terrain layer roughness"),
            (&material.occlusion_texture_id, TextureColorSpace::Data, "terrain layer occlusion"),
        ] {
            let id = id.as_deref().ok_or_else(|| malformed(format!("{label} map is missing")))?;
            mipped(id, space, label)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// COMPOSED SET
// ---------------------------------------------------------------------------

/// A verified, decoded, mipped layer set ready to attach to a packet.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainLayerSet {
    pub layers: TerrainLayers,
    pub textures: Vec<TextureReference>,
    pub materials: Vec<MaterialIntent>,
}

/// A square RGBA8 image (row-major, 4 bytes per texel).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SquareRgba8 {
    pub side: u32,
    pub bytes: Vec<u8>,
}

/// One layer's decoded source images and authored parameters.
#[derive(Clone, Debug)]
pub struct LayerSource {
    pub layer_id: String,
    /// Provenance stem recorded in every texture's `source_artifact_id`.
    pub source: String,
    pub metres_per_repeat_milli: u32,
    pub normal_scale: f32,
    pub coverage: Option<LayerCoverage>,
    pub albedo: SquareRgba8,
    pub normal: SquareRgba8,
    pub roughness: SquareRgba8,
    pub occlusion: SquareRgba8,
}

/// Packet texture sizes per map (powers of two ≤ the source side).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LayerTextureSizes {
    pub albedo: u32,
    pub normal_gl: u32,
    pub roughness: u32,
    pub ao: u32,
}

#[derive(Clone, Copy)]
enum MapKind {
    Srgb,
    Normal,
    Data,
}

fn srgb_decode_table() -> [f32; 256] {
    std::array::from_fn(|value| {
        let c = value as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    })
}

/// Encode linear light to the sRGB byte whose decoded value is nearest. Using
/// the same table both ways keeps encode(decode(b)) == b exactly.
fn srgb_encode(table: &[f32; 256], linear: f32) -> u8 {
    let index = table.partition_point(|value| *value < linear);
    if index == 0 {
        return 0;
    }
    if index >= 256 {
        return 255;
    }
    if (linear - table[index - 1]) <= (table[index] - linear) { (index - 1) as u8 } else { index as u8 }
}

/// Box-filtered pyramid from `image` down to 1×1. Albedo is averaged in linear
/// light (averaging sRGB bytes darkens every mip); normals are averaged as
/// vectors and renormalised; data is averaged directly. Alpha is always 255.
fn pyramid(image: &SquareRgba8, kind: MapKind) -> Vec<Vec<u8>> {
    let table = srgb_decode_table();
    let decode = |byte: u8| -> f32 {
        match kind {
            MapKind::Srgb => table[byte as usize],
            MapKind::Normal => byte as f32 / 255.0 * 2.0 - 1.0,
            MapKind::Data => byte as f32 / 255.0,
        }
    };
    let side = image.side as usize;
    let mut work: Vec<[f32; 3]> = image
        .bytes
        .chunks_exact(4)
        .map(|texel| [decode(texel[0]), decode(texel[1]), decode(texel[2])])
        .collect();
    let encode_level = |values: &[[f32; 3]]| -> Vec<u8> {
        let mut out = Vec::with_capacity(values.len() * 4);
        for value in values {
            let rgb = match kind {
                MapKind::Srgb => value.map(|channel| srgb_encode(&table, channel)),
                MapKind::Normal => {
                    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt().max(1e-6);
                    value.map(|channel| ((channel / length * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8)
                }
                MapKind::Data => value.map(|channel| (channel * 255.0).round().clamp(0.0, 255.0) as u8),
            };
            out.extend([rgb[0], rgb[1], rgb[2], 255]);
        }
        out
    };
    let mut levels = vec![encode_level(&work)];
    let mut current = side;
    while current > 1 {
        let next = current / 2;
        let mut reduced = vec![[0.0f32; 3]; next * next];
        for row in 0..next {
            for column in 0..next {
                let mut sum = [0.0f32; 3];
                for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                    let texel = work[(row * 2 + dy) * current + column * 2 + dx];
                    for channel in 0..3 {
                        sum[channel] += texel[channel];
                    }
                }
                reduced[row * next + column] = sum.map(|channel| channel * 0.25);
            }
        }
        work = reduced;
        current = next;
        levels.push(encode_level(&work));
    }
    levels
}

fn mipped_texture(
    texture_id: String,
    source_artifact_id: String,
    image: &SquareRgba8,
    target_side: u32,
    kind: MapKind,
    space: TextureColorSpace,
) -> Result<TextureReference, GraphicsContractError> {
    if !image.side.is_power_of_two() || image.bytes.len() != (image.side * image.side * 4) as usize {
        return Err(malformed(format!("{texture_id}: source must be a square power-of-two RGBA8 image")));
    }
    if !target_side.is_power_of_two() || target_side > image.side {
        return Err(malformed(format!(
            "{texture_id}: target size {target_side} must be a power of two no larger than the {} source",
            image.side
        )));
    }
    let skip = (image.side / target_side).trailing_zeros() as usize;
    let levels: Vec<Vec<u8>> = pyramid(image, kind).into_iter().skip(skip).collect();
    let mut all = Vec::new();
    let mut mip_levels = Vec::with_capacity(levels.len());
    let mut side = target_side;
    for level in &levels {
        all.extend_from_slice(level);
        mip_levels.push(TextureMipLevel { width_px: side, height_px: side, base64: STANDARD.encode(level) });
        side = (side / 2).max(1);
    }
    Ok(TextureReference {
        texture_id,
        source_artifact_id,
        sha256: sha256_prefixed(&all),
        width_px: target_side,
        height_px: target_side,
        mip_levels: mip_levels.len() as u32,
        color_space: space,
        payload: Some(TexturePayload::Rgba8MipChain { levels: mip_levels }),
    })
}

/// Deterministic tileable fbm noise for macro variation (grey, `Data`).
fn macro_noise_image(side: u32, seed: u32) -> SquareRgba8 {
    let mut bytes = Vec::with_capacity((side * side * 4) as usize);
    for row in 0..side {
        for column in 0..side {
            let u = column as f32 / side as f32;
            let v = row as f32 / side as f32;
            let value = crate::material_maps::tileable_fbm(u, v, 4, 5, seed);
            // Stretch the fbm's concentrated middle so thresholds bite.
            let stretched = ((value - 0.5) * 1.8 + 0.5).clamp(0.0, 1.0);
            let byte = (stretched * 255.0).round() as u8;
            bytes.extend([byte, byte, byte, 255]);
        }
    }
    SquareRgba8 { side, bytes }
}

/// Compose a layer set from decoded sources. Pure and deterministic: the same
/// sources produce bit-identical textures and the same `set_sha256`.
pub fn build_terrain_layer_set(
    set_id: &str,
    sizes: LayerTextureSizes,
    sources: &[LayerSource],
    macro_side: u32,
    macro_seed: u32,
) -> Result<TerrainLayerSet, GraphicsContractError> {
    crate::valid_id(set_id, "terrain layer set_id")?;
    let mut textures = Vec::new();
    let mut materials = Vec::new();
    let mut layers = Vec::new();
    for source in sources {
        let stem = format!("terrain-layer-{set_id}-{}", source.layer_id);
        let map = |suffix: &str| format!("{stem}-{suffix}");
        let provenance = |suffix: &str| format!("{}-{suffix}", source.source);
        let albedo = mipped_texture(map("albedo"), provenance("albedo"), &source.albedo, sizes.albedo, MapKind::Srgb, TextureColorSpace::Srgb)?;
        let normal = mipped_texture(map("normal"), provenance("normal_gl"), &source.normal, sizes.normal_gl, MapKind::Normal, TextureColorSpace::NormalMap)?;
        let roughness = mipped_texture(map("roughness"), provenance("roughness"), &source.roughness, sizes.roughness, MapKind::Data, TextureColorSpace::Data)?;
        let occlusion = mipped_texture(map("occlusion"), provenance("ao"), &source.occlusion, sizes.ao, MapKind::Data, TextureColorSpace::Data)?;
        materials.push(MaterialIntent {
            material_id: stem.clone(),
            base_color_rgba: [1.0, 1.0, 1.0, 1.0],
            metallic: 0.0,
            roughness: 1.0,
            clearcoat: 0.0,
            clearcoat_roughness: 0.5,
            alpha_mode: AlphaMode::Opaque,
            alpha_cutoff: None,
            double_sided: None,
            metallic_from_texture: None,
            texture_ids: vec![albedo.texture_id.clone()],
            normal_texture_id: Some(normal.texture_id.clone()),
            roughness_texture_id: Some(roughness.texture_id.clone()),
            occlusion_texture_id: Some(occlusion.texture_id.clone()),
            emissive_texture_id: None,
            normal_scale: source.normal_scale,
            occlusion_strength: 1.0,
            emissive_factor_rgb: [0.0, 0.0, 0.0],
        });
        textures.extend([albedo, normal, roughness, occlusion]);
        layers.push(TerrainLayer {
            layer_id: source.layer_id.clone(),
            material_id: stem,
            metres_per_repeat_milli: source.metres_per_repeat_milli,
            coverage: source.coverage,
        });
    }
    let macro_texture = mipped_texture(
        format!("terrain-layer-{set_id}-macro"),
        format!("procedural-terrain-macro-{set_id}-seed-{macro_seed}"),
        &macro_noise_image(macro_side, macro_seed),
        macro_side,
        MapKind::Data,
        TextureColorSpace::Data,
    )?;
    let macro_texture_id = macro_texture.texture_id.clone();
    textures.push(macro_texture);
    let digest_input = serde_json::json!({
        "set_id": set_id,
        "layers": layers,
        "materials": materials.iter().map(|m| (&m.material_id, m.normal_scale)).collect::<Vec<_>>(),
        "texture_sha256": textures.iter().map(|t| &t.sha256).collect::<Vec<_>>(),
    });
    let set_sha256 = sha256_prefixed(&canonical_json(&digest_input)?);
    let layers = TerrainLayers { set_id: set_id.into(), set_sha256, layers, macro_texture_id };
    validate_terrain_layers(&layers, &materials, &textures)?;
    Ok(TerrainLayerSet { layers, textures, materials })
}

/// Attach a layer set to a packet body. Refuses id collisions rather than
/// shadowing existing textures or materials.
pub fn apply_terrain_layers(body: &mut GraphicsScenePacketBody, set: &TerrainLayerSet) -> Result<(), GraphicsContractError> {
    for texture in &set.textures {
        if body.textures.iter().any(|existing| existing.texture_id == texture.texture_id) {
            return Err(GraphicsContractError::provenance(format!("terrain layer texture {} collides", texture.texture_id)));
        }
    }
    for material in &set.materials {
        if body.materials.iter().any(|existing| existing.material_id == material.material_id) {
            return Err(GraphicsContractError::provenance(format!("terrain layer material {} collides", material.material_id)));
        }
    }
    body.textures.extend(set.textures.iter().cloned());
    body.materials.extend(set.materials.iter().cloned());
    body.terrain.layers = Some(set.layers.clone());
    Ok(())
}

// ---------------------------------------------------------------------------
// MANIFEST LOADER
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Manifest {
    schema_version: String,
    set_id: String,
    license: String,
    cache_dir: String,
    texture_sizes_px: LayerTextureSizes,
    layers: Vec<ManifestLayer>,
    #[serde(rename = "macro")]
    macro_noise: ManifestMacro,
}

#[derive(Deserialize)]
struct ManifestLayer {
    layer_id: String,
    source_asset: String,
    dimensions_mm: [f64; 2],
    normal_scale_milli: u32,
    maps: ManifestMaps,
    coverage: Option<ManifestCoverage>,
}

#[derive(Deserialize)]
struct ManifestMaps {
    albedo: ManifestFile,
    normal_gl: ManifestFile,
    roughness: ManifestFile,
    ao: ManifestFile,
}

#[derive(Deserialize)]
struct ManifestFile {
    url: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
struct ManifestCoverage {
    slope_bp: Option<[i32; 2]>,
    height_mm: Option<[i32; 2]>,
    #[serde(rename = "macro")]
    macro_ramp: Option<MacroRamp>,
}

#[derive(Deserialize)]
struct ManifestMacro {
    size_px: u32,
    seed: u32,
}

pub const TERRAIN_LAYER_MANIFEST_SCHEMA: &str = "wge.terrain-layer-set-manifest/v1";

/// Which channel of a decoded scan feeds a scalar map.
#[derive(Clone, Copy)]
enum Channel {
    All,
    Green,
    Red,
}

fn decode_verified(path: &Path, file: &ManifestFile, channel: Channel) -> Result<SquareRgba8, GraphicsContractError> {
    let bytes = std::fs::read(path).map_err(|error| {
        GraphicsContractError::provenance(format!(
            "terrain layer file {} is not available ({error}); run tools/fetch_terrain_layers.py",
            path.display()
        ))
    })?;
    if bytes.len() as u64 != file.bytes || sha256_prefixed(&bytes) != file.sha256 {
        return Err(GraphicsContractError::provenance(format!(
            "terrain layer file {} does not match its manifest pin ({})",
            path.display(),
            file.url
        )));
    }
    let image = image::load_from_memory(&bytes)
        .map_err(|error| malformed(format!("terrain layer file {} does not decode: {error}", path.display())))?
        .to_rgba8();
    if image.width() != image.height() || !image.width().is_power_of_two() {
        return Err(malformed(format!("terrain layer file {} must be square power-of-two", path.display())));
    }
    let mut out = image.into_raw();
    for texel in out.chunks_exact_mut(4) {
        let value = match channel {
            Channel::All => continue,
            Channel::Green => texel[1],
            Channel::Red => texel[0],
        };
        texel[0] = value;
        texel[1] = value;
        texel[2] = value;
        texel[3] = 255;
    }
    let side = (out.len() as f64 / 4.0).sqrt() as u32;
    Ok(SquareRgba8 { side, bytes: out })
}

/// Load, verify, decode, and compose the layer set a manifest pins.
///
/// `repo_root` resolves the manifest's `cache_dir`. Every file's byte size and
/// sha256 are checked before decoding; a missing or altered file is refused.
pub fn load_terrain_layer_set(manifest_path: &Path, repo_root: &Path) -> Result<TerrainLayerSet, GraphicsContractError> {
    let text = std::fs::read_to_string(manifest_path).map_err(|error| {
        GraphicsContractError::provenance(format!("terrain layer manifest {}: {error}", manifest_path.display()))
    })?;
    let manifest: Manifest = serde_json::from_str(&text)
        .map_err(|error| malformed(format!("terrain layer manifest {} is malformed: {error}", manifest_path.display())))?;
    if manifest.schema_version != TERRAIN_LAYER_MANIFEST_SCHEMA {
        return Err(malformed(format!("terrain layer manifest schema {} is unsupported", manifest.schema_version)));
    }
    if manifest.license != "CC0-1.0" {
        return Err(GraphicsContractError::provenance(format!(
            "terrain layer set {} declares licence {}; only CC0-1.0 sources are accepted",
            manifest.set_id, manifest.license
        )));
    }
    let mut sources = Vec::with_capacity(manifest.layers.len());
    for layer in &manifest.layers {
        let file_path = |file: &ManifestFile| {
            let name = file.url.rsplit('/').next().unwrap_or_default().to_owned();
            repo_root.join(&manifest.cache_dir).join(&layer.layer_id).join(name)
        };
        if (layer.dimensions_mm[0] - layer.dimensions_mm[1]).abs() > 1.0 {
            return Err(malformed(format!("terrain layer {} scan is not square", layer.layer_id)));
        }
        sources.push(LayerSource {
            layer_id: layer.layer_id.clone(),
            source: format!("cc0-{}", layer.source_asset),
            metres_per_repeat_milli: layer.dimensions_mm[0].round() as u32,
            normal_scale: layer.normal_scale_milli as f32 / 1000.0,
            coverage: layer.coverage.as_ref().map(|coverage| LayerCoverage {
                slope_bp: coverage.slope_bp,
                height_mm: coverage.height_mm,
                macro_ramp: coverage.macro_ramp,
            }),
            albedo: decode_verified(&file_path(&layer.maps.albedo), &layer.maps.albedo, Channel::All)?,
            normal: decode_verified(&file_path(&layer.maps.normal_gl), &layer.maps.normal_gl, Channel::All)?,
            roughness: decode_verified(&file_path(&layer.maps.roughness), &layer.maps.roughness, Channel::Green)?,
            occlusion: decode_verified(&file_path(&layer.maps.ao), &layer.maps.ao, Channel::Red)?,
        });
    }
    build_terrain_layer_set(
        &manifest.set_id,
        manifest.texture_sizes_px,
        &sources,
        manifest.macro_noise.size_px,
        manifest.macro_noise.seed,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(side: u32, rgba: [u8; 4]) -> SquareRgba8 {
        SquareRgba8 { side, bytes: rgba.repeat((side * side) as usize) }
    }

    #[test]
    fn srgb_encode_inverts_decode_exactly() {
        let table = srgb_decode_table();
        for byte in 0..=255u8 {
            assert_eq!(srgb_encode(&table, table[byte as usize]), byte);
        }
    }

    #[test]
    fn albedo_mips_average_in_linear_light() {
        // A 2x2 checker of black and white: the correct 1x1 mip is the sRGB
        // encoding of 50% linear light (188), not the byte average (128).
        let image = SquareRgba8 {
            side: 2,
            bytes: [[0, 0, 0, 255], [255, 255, 255, 255], [255, 255, 255, 255], [0, 0, 0, 255]].concat(),
        };
        let levels = pyramid(&image, MapKind::Srgb);
        assert_eq!(levels.len(), 2);
        assert_eq!(levels[1][0], 188);
        let data = pyramid(&image, MapKind::Data);
        assert_eq!(data[1][0], 128);
    }

    #[test]
    fn normal_mips_stay_unit_length() {
        // Two opposite tilts average to straight up, renormalised.
        let image = SquareRgba8 {
            side: 2,
            bytes: [[218, 128, 218, 255], [38, 128, 218, 255], [218, 128, 218, 255], [38, 128, 218, 255]].concat(),
        };
        let level = &pyramid(&image, MapKind::Normal)[1];
        assert_eq!(&level[0..3], &[128, 128, 255]);
    }

    #[test]
    fn target_size_selects_the_matching_level() {
        let texture = mipped_texture("t".into(), "s".into(), &flat(64, [10, 20, 30, 255]), 16, MapKind::Data, TextureColorSpace::Data).unwrap();
        assert_eq!((texture.width_px, texture.mip_levels), (16, 5));
        assert!(mipped_texture("t".into(), "s".into(), &flat(64, [0; 4]), 128, MapKind::Data, TextureColorSpace::Data).is_err());
    }
}
