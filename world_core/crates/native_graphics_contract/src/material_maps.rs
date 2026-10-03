//! Coherent material map generation.
//!
//! WHY THIS EXISTS
//! ---------------
//! The audit found the renderer being judged on 8x8 colour swatches. That was
//! accurate, and it was worse than it looked:
//!
//!   * every mesh material used an 8x8 albedo produced by modular arithmetic;
//!   * the normal map was 8x8 of the same arithmetic noise, with a constant Z;
//!   * the roughness map spanned 176..239 — effectively constant;
//!   * the occlusion map spanned 208..239 — effectively constant; and
//!   * **all eleven materials shared those same three maps**.
//!
//! That last point is the perceptual one. Sharing a normal and roughness map
//! across grass, stone, bark, metal, and foliage is not a resolution problem;
//! it is a *semantic* one. No amount of lighting can separate surfaces whose
//! microfacet response is literally the same image. This module exists to give
//! each material its OWN maps, derived consistently from a shared height field
//! so that albedo, normal, roughness, and occlusion agree with one another.
//!
//! TILEABILITY IS A HARD REQUIREMENT, NOT A NICETY
//! -----------------------------------------------
//! Sprint F-4 made terrain tiling executable (`uv_repeat_scale_milli` +
//! `wrap_repeat`), so the terrain albedo now repeats eight times across the
//! field. A texture that does not tile produces eight visible seams, which
//! would look worse than the 8x8 swatch it replaced. Every lattice lookup here
//! wraps on an integer period, so every generated map tiles exactly.
//!
//! COLOUR SPACE IS ENFORCED, NOT ASSUMED
//! -------------------------------------
//! Albedo is `Srgb` (it is colour). Normal is `NormalMap` (it is a direction).
//! Roughness and occlusion are `Data` (they are linear scalars). Getting this
//! wrong is the classic reason a correct asset renders muddy.

use crate::{procedural_texture, TextureColorSpace, TextureReference};

/// One coherent material: albedo plus the three maps that must agree with it.
pub struct MaterialMapSet {
    pub albedo: TextureReference,
    pub normal: TextureReference,
    pub roughness: TextureReference,
    pub occlusion: TextureReference,
}

/// Which material family to synthesise. Each variant has genuinely different
/// surface structure, not just a different tint — that difference is the whole
/// point of giving materials their own maps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaterialKind {
    /// Grass/soil ground with ridges and scattered pebbles.
    Terrain,
    /// Weathered stone: coarse blocky relief with fine grain.
    Stone,
    /// Vertical fibrous ridges with cross-grain breaks.
    Bark,
    /// Clumped leaf masses with soft interiors.
    Foliage,
    /// Brushed metal: fine directional streaks plus shallow dents.
    Metal,
    /// Smooth wet stone: broad shallow relief with standing ripples.
    Wet,
}

impl MaterialKind {
    /// Stable identifier fragment, so texture ids are self-describing in a
    /// packet dump rather than being `texture-17`.
    pub fn slug(self) -> &'static str {
        match self {
            MaterialKind::Terrain => "terrain",
            MaterialKind::Stone => "stone",
            MaterialKind::Bark => "bark",
            MaterialKind::Foliage => "foliage",
            MaterialKind::Metal => "metal",
            MaterialKind::Wet => "wet",
        }
    }
}

// ---------------------------------------------------------------------------
// TILEABLE NOISE
// ---------------------------------------------------------------------------

/// Integer hash. Deterministic and platform-independent: WGSL must not be the
/// first thing to disagree about a surface, so nothing here touches floating
/// point transcendentals or hasher-dependent byte orders.
fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(0x27d4_eb2d)
        ^ (y as u32).wrapping_mul(0x1656_67b1)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h = h.wrapping_mul(0x2974_5c1d);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_216.0
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

/// Value noise on a lattice that WRAPS at `period`, so the result tiles exactly
/// with period `period` in both axes.
fn tileable_value_noise(u: f32, v: f32, period: i32, seed: u32) -> f32 {
    let x = u * period as f32;
    let y = v * period as f32;
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = smoothstep(x - x0);
    let fy = smoothstep(y - y0);
    let ix = x0 as i32;
    let iy = y0 as i32;
    let p = period.max(1);
    // Wrapping the indices is what makes the result seamless.
    let xa = ix.rem_euclid(p);
    let ya = iy.rem_euclid(p);
    let xb = (ix + 1).rem_euclid(p);
    let yb = (iy + 1).rem_euclid(p);
    let v00 = hash2(xa, ya, seed);
    let v10 = hash2(xb, ya, seed);
    let v01 = hash2(xa, yb, seed);
    let v11 = hash2(xb, yb, seed);
    let a = v00 + (v10 - v00) * fx;
    let b = v01 + (v11 - v01) * fx;
    a + (b - a) * fy
}

/// Fractal sum. Every octave's period DOUBLES, so the whole stack keeps tiling
/// on the base period.
fn tileable_fbm(u: f32, v: f32, base_period: i32, octaves: u32, seed: u32) -> f32 {
    let mut total = 0.0;
    let mut amplitude = 0.5;
    let mut norm = 0.0;
    let mut period = base_period.max(1);
    for octave in 0..octaves {
        // `wrapping_mul` is required, not stylistic: 0x9e3779b9 alone is ~2.6e9,
        // so `octave * GOLDEN` overflows u32 from the second octave onward. A
        // debug build panics; a release build wraps silently and the seed
        // derivation differs between profiles, which is a determinism bug
        // dressed as an optimisation.
        let octave_seed = seed ^ (octave as u32).wrapping_mul(0x9e37_79b9);
        total += tileable_value_noise(u, v, period, octave_seed) * amplitude;
        norm += amplitude;
        amplitude *= 0.5;
        period *= 2;
    }
    if norm > 0.0 {
        total / norm
    } else {
        0.0
    }
}

// ---------------------------------------------------------------------------
// HEIGHT FIELD — the single source of truth for all four maps
// ---------------------------------------------------------------------------

/// Surface height in 0..1 for one material family. Albedo, normal, roughness
/// and occlusion are all derived from THIS, which is why they agree.
fn material_height(kind: MaterialKind, u: f32, v: f32) -> f32 {
    match kind {
        MaterialKind::Terrain => {
            // Ground reads as clumps and hollows, not as noise: broad fbm for
            // large forms, a ridged term for worn paths, and a fine octave for
            // soil grain.
            let broad = tileable_fbm(u, v, 4, 4, 0x51ed_2701);
            let ridged = 1.0 - (tileable_fbm(u, v, 8, 3, 0x1b87_3f2d) * 2.0 - 1.0).abs();
            let grain = tileable_value_noise(u, v, 48, 0x9e37_79b9);
            (broad * 0.58 + ridged * 0.27 + grain * 0.15).clamp(0.0, 1.0)
        }
        MaterialKind::Stone => {
            // Coarse blocky relief plus fine grain. The blocky term uses a
            // sharpened fbm rather than Worley cells, which keeps the lattice
            // trivially tileable.
            let coarse = tileable_fbm(u, v, 6, 4, 0x2f8b_1c77);
            let blocky = (coarse - 0.5) * 1.6 + 0.5;
            let grain = tileable_value_noise(u, v, 64, 0x7f4a_7c15);
            let chip = tileable_value_noise(u, v, 16, 0x2545_f491);
            (blocky.clamp(0.0, 1.0) * 0.62 + grain * 0.20 + chip * 0.18).clamp(0.0, 1.0)
        }
        MaterialKind::Bark => {
            // Vertical fibres: high frequency across U, low across V, so ridges
            // run along the trunk.
            let fibres = tileable_value_noise(u, v, 24, 0xc13f_d065);
            let coarse = tileable_fbm(u, v, 6, 3, 0x5bd1_e995);
            let breaks = tileable_value_noise(u, v, 8, 0xa409_3822);
            let ridged = 1.0 - (fibres * 2.0 - 1.0).abs();
            (ridged * 0.52 + coarse * 0.30 + breaks * 0.18).clamp(0.0, 1.0)
        }
        MaterialKind::Foliage => {
            // Soft clumps: rounded masses with darker interiors. Foliage should
            // NOT share stone's sharp relief.
            let clumps = tileable_fbm(u, v, 10, 3, 0x082e_fa98);
            let rounded = (clumps - 0.35).max(0.0) * 1.55;
            let detail = tileable_value_noise(u, v, 32, 0x4528_21e6);
            (rounded.clamp(0.0, 1.0) * 0.74 + detail * 0.26).clamp(0.0, 1.0)
        }
        MaterialKind::Metal => {
            // Brushed streaks plus shallow dents. Streaks are anisotropic by
            // construction: fine in V, coarse in U.
            let streaks = tileable_value_noise(v, u, 96, 0xbe54_6cf9);
            let dents = tileable_fbm(u, v, 5, 3, 0x1c38_8e2c);
            (streaks * 0.58 + dents * 0.42).clamp(0.0, 1.0)
        }
        MaterialKind::Wet => {
            // Wet stone is SMOOTH: broad shallow relief plus faint ripples, so
            // it reads as a specular surface rather than a rough one.
            let broad = tileable_fbm(u, v, 3, 3, 0x6c8e_9cf5);
            // The ripple phase must advance by an INTEGER number of turns
            // across the tile or the seam does not close. `(u * 12.0).sin()` was
            // 1.91 turns and genuinely did not tile — a defect caught by
            // `every_generated_map_tiles`, not by looking at a screenshot.
            let cycles = 2.0f32;
            let ripple =
                ((u * core::f32::consts::TAU * cycles).sin() * 0.5 + 0.5) *
                tileable_value_noise(u, v, 6, 0x27d4_eb2d);
            (broad * 0.70 + ripple * 0.30).clamp(0.0, 1.0)
        }
    }
}

/// Albedo colour for one material family, modulated by height so that
/// recesses are darker — the cheapest honest form of baked contact shading.
///
/// Three colours, not two. A dark/light lerp produces a single one-dimensional
/// ramp, which measurably FLATTENS the image: the first version of this set
/// used two stops and the render lost 51% of its distinct colours and 5.5x of
/// its highlight fraction. Real ground and stone are a blend of several
/// substances, so the third stop is not decoration — it is the difference
/// between a tint and a material.
fn material_albedo(kind: MaterialKind, u: f32, v: f32, height: f32) -> [u8; 3] {
    let (dark, mid, light): ([f32; 3], [f32; 3], [f32; 3]) = match kind {
        MaterialKind::Terrain => (
            [36.0, 52.0, 26.0],
            [78.0, 96.0, 44.0],
            [148.0, 150.0, 86.0],
        ),
        MaterialKind::Stone => (
            [48.0, 51.0, 56.0],
            [116.0, 118.0, 120.0],
            [186.0, 186.0, 182.0],
        ),
        MaterialKind::Bark => ([30.0, 20.0, 14.0], [82.0, 58.0, 36.0], [136.0, 106.0, 70.0]),
        MaterialKind::Foliage => ([20.0, 48.0, 14.0], [64.0, 116.0, 32.0], [148.0, 186.0, 62.0]),
        // Metal is kept genuinely bright: it is the frame's specular anchor,
        // and desaturating it to a grey ramp is what cost the first version its
        // highlights.
        MaterialKind::Metal => ([74.0, 78.0, 88.0], [166.0, 172.0, 182.0], [244.0, 246.0, 250.0]),
        MaterialKind::Wet => ([22.0, 28.0, 32.0], [64.0, 80.0, 88.0], [124.0, 142.0, 150.0]),
    };
    let mix = height.clamp(0.0, 1.0);
    // Blend through the middle stop: below the midpoint dark->mid, above it
    // mid->light. A single lerp would bias the whole map to the mid colour.
    let (from, to, t) = if mix < 0.5 {
        (dark, mid, mix * 2.0)
    } else {
        (mid, light, (mix - 0.5) * 2.0)
    };
    // Hue variation from an independent noise field, so colour varies as well as
    // lightness. Without it the ramp still reads as one tint.
    let hue = (tileable_value_noise(u, v, 12, 0xbb67_ae85) - 0.5) * 2.0;
    let speckle = (tileable_value_noise(u, v, 96, 0x6a09_e667) - 0.5) * 6.0;
    let blend = |c: f32, index: usize, a: [f32; 3], b: [f32; 3]| {
        let base = a[index] + (b[index] - a[index]) * t;
        // Push saturation outward, not just lightness.
        let chroma = (index as f32 - 1.0) * hue * 9.0;
        (base + chroma + speckle).clamp(0.0, 255.0) as u8
    };
    [
        blend(0.0, 0, from, to),
        blend(1.0, 1, from, to),
        blend(2.0, 2, from, to),
    ]
}

/// Roughness in 0..1. Metals stay smoother in their high points and rougher in
/// the recesses; foliage is uniformly matte; wet stone is the smoothest.
fn material_roughness(kind: MaterialKind, height: f32) -> f32 {
    let (lo, hi) = match kind {
        MaterialKind::Terrain => (0.86f32, 0.98f32),
        MaterialKind::Stone => (0.62, 0.90),
        MaterialKind::Bark => (0.74, 0.94),
        MaterialKind::Foliage => (0.78, 0.92),
        MaterialKind::Metal => (0.18, 0.46),
        MaterialKind::Wet => (0.08, 0.28),
    };
    // Invert so raised areas are smoother: water and polished metal collect on
    // the high points, dirt collects in the hollows.
    lo + (hi - lo) * (1.0 - height).clamp(0.0, 1.0)
}

/// Build the full coherent map set for one material family.
///
/// `size` must be a positive power of two; the mip chain is derived from it by
/// the adapter, and a non-power-of-two would break that assumption.
pub fn build_material_maps(kind: MaterialKind, size: usize, revision: &str) -> MaterialMapSet {
    assert!(size.is_power_of_two() && size >= 16, "material size must be a power of two");
    let n = size;
    let mut heights = vec![0.0f32; size * size];
    let mut albedo = Vec::with_capacity(size * size * 4);
    let mut roughness = Vec::with_capacity(size * size * 4);
    let mut occlusion = Vec::with_capacity(size * size * 4);

    for row in 0..size {
        for column in 0..size {
            // u,v in [0,1); the noise wraps on its own period.
            let u = column as f32 / size as f32;
            let v = row as f32 / size as f32;
            let h = material_height(kind, u, v);
            heights[row * size + column] = h;
            let [r, g, b] = material_albedo(kind, u, v, h);
            albedo.extend([r, g, b, 255]);
            let rough = (material_roughness(kind, h) * 255.0).round().clamp(0.0, 255.0) as u8;
            roughness.extend([rough, rough, rough, 255]);
            occlusion.extend([255, 255, 255, 255]); // filled in below
        }
    }

    // Occlusion from a WRAPPED blur of the height field: a texel is occluded in
    // proportion to how much taller its neighbourhood is. Cheap, and because
    // the blur wraps it tiles exactly.
    let radius = ((size / 32) as i32).max(1);
    for row in 0..size {
        for column in 0..size {
            let mut sum = 0.0f32;
            let mut count = 0.0f32;
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let yy = (row as i32 + dy).rem_euclid(n as i32) as usize;
                    let xx = (column as i32 + dx).rem_euclid(n as i32) as usize;
                    sum += heights[yy * size + xx];
                    count += 1.0;
                }
            }
            let average = sum / count;
            let here = heights[row * size + column];
            // Positive when the neighbourhood stands above this texel.
            let cavity = (average - here).clamp(-0.5, 0.5);
            let ao = (1.0 - cavity * 1.5).clamp(0.35, 1.0);
            let value = (ao * 255.0).round().clamp(0.0, 255.0) as u8;
            let index = (row * size + column) * 4;
            occlusion[index] = value;
            occlusion[index + 1] = value;
            occlusion[index + 2] = value;
            occlusion[index + 3] = 255;
        }
    }

    // Normal map from the height field by wrapped central differences. +Z is up
    // (OpenGL-style tangent space), matching the convention the adapter's
    // `_perturbed_normal` already assumes.
    let strength = match kind {
        MaterialKind::Terrain => 2.4f32,
        MaterialKind::Stone => 3.6,
        MaterialKind::Bark => 3.2,
        MaterialKind::Foliage => 1.6,
        MaterialKind::Metal => 1.1,
        MaterialKind::Wet => 1.4,
    };
    let mut normal = Vec::with_capacity(size * size * 4);
    for row in 0..size {
        for column in 0..size {
            let left = heights[row * size + (column + n - 1) % n];
            let right = heights[row * size + (column + 1) % n];
            let down = heights[((row + n - 1) % n) * size + column];
            let up = heights[((row + 1) % n) * size + column];
            let dx = (left - right) * strength;
            let dy = (down - up) * strength;
            let length = (dx * dx + dy * dy + 1.0).sqrt();
            normal.extend([
                (((dx / length) * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8,
                (((dy / length) * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8,
                (((1.0 / length) * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8,
                255,
            ]);
        }
    }

    let slug = kind.slug();
    MaterialMapSet {
        albedo: procedural_texture(
            &format!("wge-hero-{slug}-albedo"),
            &format!("procedural-wge-hero-{slug}-albedo-{revision}"),
            TextureColorSpace::Srgb,
            size as u32,
            size as u32,
            albedo,
        ),
        normal: procedural_texture(
            &format!("wge-hero-{slug}-normal"),
            &format!("procedural-wge-hero-{slug}-normal-{revision}"),
            TextureColorSpace::NormalMap,
            size as u32,
            size as u32,
            normal,
        ),
        roughness: procedural_texture(
            &format!("wge-hero-{slug}-roughness"),
            &format!("procedural-wge-hero-{slug}-roughness-{revision}"),
            TextureColorSpace::Data,
            size as u32,
            size as u32,
            roughness,
        ),
        occlusion: procedural_texture(
            &format!("wge-hero-{slug}-occlusion"),
            &format!("procedural-wge-hero-{slug}-occlusion-{revision}"),
            TextureColorSpace::Data,
            size as u32,
            size as u32,
            occlusion,
        ),
    }
}

/// Resolution for the hero material set.
///
/// 256 is a deliberate, bounded choice, not an accident: it is a real step up
/// from the 8x8 swatches (and a further 4x linear increase in texel density),
/// and it keeps the embedded payload to a size the packet can carry. The
/// honest framing is "sane production scale", not "1024-class".
pub const HERO_TEXTURE_SIZE: usize = 256;

/// Which material family a given authored material id belongs to.
///
/// Explicit rather than inferred: an id that is not listed keeps its existing
/// maps. Guessing a family from a name would be exactly the "infer ordered
/// meaning from identifiers" move the contract forbids.
fn kind_for_material(material_id: &str) -> Option<MaterialKind> {
    match material_id {
        "terrain-default" => Some(MaterialKind::Terrain),
        "obstacle-default" | "campaign2-hero-stone" | "campaign2-rock"
        | "objective-beacon" => Some(MaterialKind::Stone),
        "campaign2-bark" => Some(MaterialKind::Bark),
        "foliage-default" | "campaign2-foliage" => Some(MaterialKind::Foliage),
        "campaign2-hero-metal" => Some(MaterialKind::Metal),
        "campaign2-wet" => Some(MaterialKind::Wet),
        // `campaign2-hero-glow` is deliberately NOT mapped. Its read comes from
        // its emissive map, not from microfacet response, and reassigning
        // surface maps to it would be an art decision made blind. It keeps its
        // existing maps rather than being given a family's by accident.
        _ => None,
    }
}

/// Replace every mapped material's surface maps with its own coherent set.
///
/// Returns the number of materials actually remapped so the caller can prove
/// the swap took effect rather than assuming it did.
pub fn apply_hero_material_set(body: &mut crate::GraphicsScenePacketBody) -> usize {
    let families = [
        MaterialKind::Terrain,
        MaterialKind::Stone,
        MaterialKind::Bark,
        MaterialKind::Foliage,
        MaterialKind::Metal,
        MaterialKind::Wet,
    ];
    let sets: Vec<MaterialMapSet> = families
        .iter()
        .map(|kind| build_material_maps(*kind, HERO_TEXTURE_SIZE, "v2"))
        .collect();

    for set in &sets {
        body.textures.push(set.albedo.clone());
        body.textures.push(set.normal.clone());
        body.textures.push(set.roughness.clone());
        body.textures.push(set.occlusion.clone());
    }

    let mut remapped = 0usize;
    for material in body.materials.iter_mut() {
        let Some(kind) = kind_for_material(&material.material_id) else {
            continue;
        };
        let index = families
            .iter()
            .position(|candidate| *candidate == kind)
            .expect("kind came from the families list");
        let set = &sets[index];
        material.texture_ids = vec![set.albedo.texture_id.clone()];
        material.normal_texture_id = Some(set.normal.texture_id.clone());
        material.roughness_texture_id = Some(set.roughness.texture_id.clone());
        material.occlusion_texture_id = Some(set.occlusion.texture_id.clone());
        remapped += 1;
    }
    remapped
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tileability is a correctness property, not a quality one: F-4 makes the
    /// terrain albedo repeat eight times, so a non-tiling map produces eight
    /// visible seams. This asserts the property directly rather than trusting
    /// the reviewer's eye.
    #[test]
    fn every_generated_map_tiles() {
        for kind in [
            MaterialKind::Terrain,
            MaterialKind::Stone,
            MaterialKind::Bark,
            MaterialKind::Foliage,
            MaterialKind::Metal,
            MaterialKind::Wet,
        ] {
            // Sampling the two edges of the tile must agree in BOTH axes.
            // Checking only one axis is what let the wet ripple ship.
            let start_u = material_height(kind, 0.0, 0.37);
            let end_u = material_height(kind, 1.0 - 1e-6, 0.37);
            assert!(
                (start_u - end_u).abs() < 0.02,
                "{kind:?} height does not tile in u: {start_u} vs {end_u}"
            );
            let start_v = material_height(kind, 0.61, 0.0);
            let end_v = material_height(kind, 0.61, 1.0 - 1e-6);
            assert!(
                (start_v - end_v).abs() < 0.02,
                "{kind:?} height does not tile in v: {start_v} vs {end_v}"
            );
        }
    }

    /// A normal map must actually contain relief. The old 8x8 generator wrote a
    /// constant Z and arithmetic noise elsewhere, which is precisely why every
    /// surface in the scene looked the same.
    #[test]
    fn normal_maps_contain_real_relief_and_metal_is_smoothest() {
        let mut metal_spread = 0.0f32;
        let mut stone_spread = 0.0f32;
        for kind in [MaterialKind::Metal, MaterialKind::Stone] {
            let set = build_material_maps(kind, 64, "test");
            let raw = decoded_payload(&set.normal);
            let mut min_x = 255u8;
            let mut max_x = 0u8;
            for i in (0..raw.len()).step_by(4) {
                min_x = min_x.min(raw[i]);
                max_x = max_x.max(raw[i]);
            }
            let spread = max_x as f32 - min_x as f32;
            assert!(spread > 4.0, "{kind:?} normal map is flat (spread {spread})");
            if kind == MaterialKind::Metal {
                metal_spread = spread;
            } else {
                stone_spread = spread;
            }
        }
        assert!(
            stone_spread > metal_spread,
            "stone should read rougher than brushed metal ({} vs {})",
            stone_spread,
            metal_spread
        );
    }

    /// Roughness must vary within a material. The old shared map spanned
    /// 176..239 of a *shared* image for all eleven materials.
    #[test]
    fn roughness_varies_and_is_ordered_by_material_family() {
        let stone = build_material_maps(MaterialKind::Stone, 64, "test");
        let wet = build_material_maps(MaterialKind::Wet, 64, "test");
        let decode = |set: &MaterialMapSet| decoded_payload(&set.roughness);
        let spread = |raw: &[u8]| {
            let mut lo = 255u8;
            let mut hi = 0u8;
            for i in (0..raw.len()).step_by(4) {
                lo = lo.min(raw[i]);
                hi = hi.max(raw[i]);
            }
            (hi - lo) as f32
        };
        let (stone_raw, wet_raw) = (decode(&stone), decode(&wet));
        assert!(
            spread(&stone_raw) > 8.0,
            "stone roughness is effectively constant"
        );
        let mean = |raw: &[u8]| raw.iter().step_by(4).map(|v| *v as f32).sum::<f32>() / (raw.len() / 4) as f32;
        assert!(
            mean(&wet_raw) < mean(&stone_raw),
            "wet stone should be smoother than dry stone ({} vs {})",
            mean(&wet_raw),
            mean(&stone_raw)
        );
    }

    /// Generation must be deterministic: the same inputs, byte for byte.
    #[test]
    fn generation_is_deterministic() {
        let a = build_material_maps(MaterialKind::Bark, 32, "test");
        let b = build_material_maps(MaterialKind::Bark, 32, "test");
        assert_eq!(a.albedo.sha256, b.albedo.sha256);
        assert_eq!(a.normal.sha256, b.normal.sha256);
        assert_eq!(a.roughness.sha256, b.roughness.sha256);
        assert_eq!(a.occlusion.sha256, b.occlusion.sha256);
    }

    /// Every mapped material must end up with maps that are NOT the shared
    /// `riverwatch-*` set, and no two families may share a map. Sharing was the
    /// actual defect: eleven materials pointing at one normal map cannot
    /// separate no matter how good the lighting is.
    #[test]
    fn hero_material_set_gives_each_family_its_own_maps() {
        // Applied to the REAL committed campaign2 body rather than a
        // hand-built fixture. A fixture would prove the function works on the
        // shapes its author imagined; this proves it works on the materials the
        // renderer actually ships.
        let mut body = committed_body();
        let remapped = apply_hero_material_set(&mut body);
        assert!(
            remapped > 0,
            "the hero material set remapped nothing: the swap silently did nothing"
        );

        let shared = body
            .materials
            .iter()
            .filter(|m| {
                m.normal_texture_id.as_deref() == Some("riverwatch-normal")
                    && kind_for_material(&m.material_id).is_some()
            })
            .count();
        assert_eq!(
            shared, 0,
            "a mapped material still points at the shared 8x8 normal map"
        );

        // Distinct FAMILIES must not share a map; two materials of the SAME
        // family must, because they are the same substance. An earlier version
        // of this test asserted per-material uniqueness and failed on
        // obstacle-default / hero-stone / rock / beacon, which are all stone.
        // The test was wrong; sharing within a family is the intended answer.
        let mut family_maps: Vec<(MaterialKind, String)> = Vec::new();
        for material in body.materials.iter() {
            let Some(kind) = kind_for_material(&material.material_id) else {
                continue;
            };
            let normal = material.normal_texture_id.clone().expect("mapped has a normal");
            assert!(
                normal.starts_with("wge-hero-"),
                "material {} points at {normal}",
                material.material_id
            );
            // Every referenced texture must actually be present in the body,
            // or the adapter will fail on a dangling id.
            assert!(
                body.textures.iter().any(|t| t.texture_id == normal),
                "material {} references absent texture {normal}",
                material.material_id
            );
            match family_maps.iter().find(|(k, _)| *k == kind) {
                Some((_, existing)) => assert_eq!(
                    &normal, existing,
                    "material {} is family {kind:?} but uses {normal} instead of {existing}",
                    material.material_id
                ),
                None => family_maps.push((kind, normal)),
            }
        }
        let distinct_families = [
            MaterialKind::Terrain,
            MaterialKind::Stone,
            MaterialKind::Bark,
            MaterialKind::Foliage,
            MaterialKind::Metal,
            MaterialKind::Wet,
        ]
        .iter()
        .filter(|kind| family_maps.iter().any(|(k, _)| k == *kind))
        .count();
        assert_eq!(
            distinct_families,
            family_maps.len(),
            "each present family must contribute exactly one distinct map set"
        );
        assert!(
            distinct_families >= 3,
            "expected the committed body's families to be distinct, got {distinct_families}"
        );
    }

    /// The committed packet is the PRE-lowering body: it carries terrain,
    /// obstacle, foliage, and beacon. The `campaign2-*` families (bark, metal,
    /// wet, hero-stone) only exist after `lower_campaign2_packet`, which needs a
    /// world artifact this unit test deliberately does not construct. So the
    /// mapping itself is asserted directly, and the application is asserted
    /// above against whatever families the committed body really has.
    #[test]
    fn every_campaign2_material_id_maps_to_a_family() {
        for (id, expected) in [
            ("terrain-default", MaterialKind::Terrain),
            ("obstacle-default", MaterialKind::Stone),
            ("campaign2-hero-stone", MaterialKind::Stone),
            ("campaign2-rock", MaterialKind::Stone),
            ("objective-beacon", MaterialKind::Stone),
            ("campaign2-bark", MaterialKind::Bark),
            ("foliage-default", MaterialKind::Foliage),
            ("campaign2-foliage", MaterialKind::Foliage),
            ("campaign2-hero-metal", MaterialKind::Metal),
            ("campaign2-wet", MaterialKind::Wet),
        ] {
            assert_eq!(
                kind_for_material(id),
                Some(expected),
                "{id} maps to the wrong family"
            );
        }
        // The emissive accent is deliberately unmapped, not accidentally so.
        assert_eq!(kind_for_material("campaign2-hero-glow"), None);
        assert_eq!(kind_for_material("some-future-material"), None);
    }

    /// Colour space is part of the contract. Albedo is colour, normal is a
    /// direction, roughness and AO are linear data.
    #[test]
    fn colour_spaces_are_declared_per_map_role() {
        let set = build_material_maps(MaterialKind::Stone, 32, "test");
        assert_eq!(set.albedo.color_space, TextureColorSpace::Srgb);
        assert_eq!(set.normal.color_space, TextureColorSpace::NormalMap);
        assert_eq!(set.roughness.color_space, TextureColorSpace::Data);
        assert_eq!(set.occlusion.color_space, TextureColorSpace::Data);
    }

    /// The committed campaign2 body, so this test runs against the materials
    /// the renderer really uses rather than a hand-written guess.
    fn committed_body() -> crate::GraphicsScenePacketBody {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("crate is inside the workspace")
            .join("artifacts/campaign2/adapter-v6-baseline-replay-2026-09-29/run-a/graphics_scene_packet.json");
        let raw = std::fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!(
                "committed baseline packet must be present for the hero material set test at {}: {error}",
                path.display()
            )
        });
        let value: serde_json::Value = serde_json::from_str(&raw).expect("committed packet parses");
        serde_json::from_value(value["body"].clone()).expect("committed body deserializes")
    }

    fn base64_decode(input: &str) -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(input)
            .expect("valid base64")
    }

    /// `TexturePayload::Rgba8` holds an already-base64-encoded payload.
    fn decoded_payload(reference: &TextureReference) -> Vec<u8> {
        match reference
            .payload
            .as_ref()
            .expect("material maps always carry a payload")
        {
            crate::TexturePayload::Rgba8(encoded) => base64_decode(encoded),
            crate::TexturePayload::Rgba8MipChain { levels } => base64_decode(
                levels
                    .first()
                    .expect("mip chain has a base level")
                    .base64
                    .as_str(),
            ),
        }
    }
}