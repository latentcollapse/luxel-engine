//! N-2: prefiltered image-based lighting (docs/world/converge/converge1-contracts.md §2).
//!
//! Baked in Rust, never in a shader, from the packet's own environment: the
//! sky the renderer draws (gradient, or the N-1 analytic sky, plus its sun
//! glow and the atmosphere's sun lobe) and the ground below the horizon,
//! WITHOUT the sun disc: the disc is the key light, which is lit directly, so
//! baking it in would count the sun twice. The glow must stay: the analytic
//! sky's luminance spike near the sun carries the authored (blue) hue, and the
//! drawn sky only reads white-warm there because the glow is added on top.
//!
//! Lava samples textures with implicit LOD only, so the prefiltered levels
//! cannot be a mip chain the shader indexes by roughness. They are tiles of
//! one single-level atlas instead, and the shader blends two adjacent levels
//! itself:
//!
//! ```text
//!  x: 0     130    196  230 248 258 264    298
//!     | L0  | L1   | L2 |L3|L4|L5|  IRR  |      y: 0..130
//!     128²  64²    32²  16² 8² 4²   32²         (+1 px gutter round each)
//! ```
//!
//! Every tile is an octahedral map of the sphere (+Y up), gutters mirror
//! across the fold so bilinear filtering is seamless. Radiance is stored as
//! sRGB(radiance / IBL_RADIANCE_SCALE) in RGBA8, the only packet format.
//! L0..L5 are GGX-prefiltered at perceptual roughness k/5 (split-sum, N=V=R);
//! IRR is irradiance / π from 9-coefficient spherical harmonics. A second
//! 64×64 texture holds the split-sum BRDF table (R = A, G = B, Karis 2013).
//!
//! The bake is deterministic and digest-bound: the textures are ordinary
//! packet textures with reserved ids, and packet validation re-bakes them from
//! the packet's environment and requires an exact match, so a packet cannot
//! carry an environment its sky does not produce.

use crate::render_policy::SkyModel;
use crate::{
    EnvironmentIntent, GraphicsContractError, GraphicsScenePacketBody, LightKind, TextureColorSpace,
    TextureReference, procedural_texture,
};

pub const IBL_ENVIRONMENT_TEXTURE_ID: &str = "luxel-ibl-environment";
pub const IBL_BRDF_LUT_TEXTURE_ID: &str = "luxel-ibl-brdf-lut";
const IBL_SOURCE_ARTIFACT_ID: &str = "luxel-ibl-bake";

/// Stored radiance = sRGB(radiance / scale). Near the sun the analytic sky
/// plus glow exceeds 4; 8 keeps it unclipped at ~1.4% quantisation. Mirrored
/// by `LavaAdapter.IBL_RADIANCE_SCALE`.
pub const IBL_RADIANCE_SCALE: f64 = 8.0;
/// Prefiltered tile sizes, roughness k/5 for tile k.
pub const IBL_LEVEL_SIZES: [u32; 6] = [128, 64, 32, 16, 8, 4];
pub const IBL_IRRADIANCE_SIZE: u32 = 32;
pub const IBL_LUT_SIZE: u32 = 64;
const PREFILTER_SAMPLES: u32 = 128;
const LUT_SAMPLES: u32 = 256;
const SH_SAMPLES: u32 = 4096;

/// Tile origins in the atlas (x of the gutter's first column), then the width.
pub fn atlas_layout() -> ([u32; 7], u32, u32) {
    let mut origins = [0u32; 7];
    let mut x = 0;
    for (k, size) in IBL_LEVEL_SIZES.iter().chain(std::iter::once(&IBL_IRRADIANCE_SIZE)).enumerate() {
        origins[k] = x;
        x += size + 2;
    }
    (origins, x, IBL_LEVEL_SIZES[0] + 2)
}

// ------------------------------------------------------------------ sky

/// Preetham et al. (1999) Perez luminance coefficients, (slope, offset) in T.
/// Mirrors `LavaAdapter.PREETHAM_Y`.
const PREETHAM_Y: [(f64, f64); 5] =
    [(0.1787, -1.4630), (-0.3554, 0.4275), (-0.0227, 5.3251), (0.1206, -2.5771), (-0.0670, 0.3703)];

fn perez(turbidity: f64, cos_theta: f64, gamma: f64, cos_gamma: f64) -> f64 {
    let c = PREETHAM_Y.map(|(slope, offset)| slope * turbidity + offset);
    (1.0 + c[0] * (c[1] / cos_theta).exp()) * (1.0 + c[2] * (c[3] * gamma).exp() + c[4] * cos_gamma * cos_gamma)
}

type V3 = [f64; 3];

fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: V3) -> V3 {
    let l = dot(v, v).sqrt();
    if l < 1e-12 { [0.0, 1.0, 0.0] } else { [v[0] / l, v[1] / l, v[2] / l] }
}

fn mix(a: V3, b: V3, t: f64) -> V3 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn luminance(c: V3) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// The environment the shaders' ambient term sees: `LavaAdapter._sky_environment`
/// in f64. Sun disc and glow are excluded (the key light is direct).
#[derive(Clone, Debug, PartialEq)]
pub struct IblEnvironment {
    top: V3,
    horizon: V3,
    ground: V3,
    /// (turbidity, Ŷ, toward-sun) under the analytic model.
    analytic: Option<(f64, f64, V3)>,
    /// Sun lobes the sky pass adds (view-direction sky only): toward-sun, key
    /// light radiance, sky glow gain, atmosphere scatter gain.
    lobes: Option<(V3, V3, f64, f64)>,
}

impl IblEnvironment {
    pub fn from_body(body: &GraphicsScenePacketBody) -> Result<Self, GraphicsContractError> {
        let EnvironmentIntent { sky_top_rgb, sky_horizon_rgb, ground_rgb, .. } = body.environment;
        let wide = |c: [f32; 3]| c.map(f64::from);
        let policy = body.render_policy.as_ref();
        let sky_policy = policy.and_then(|p| p.sky);
        let model = sky_policy.and_then(|s| s.model);
        let key = body.lights.iter().find_map(|light| match light.kind {
            LightKind::Directional { direction_xyz } => Some((
                normalize(direction_xyz_toward(direction_xyz)),
                light.color_rgb.map(|c| f64::from(c) * f64::from(light.intensity)),
            )),
            _ => None,
        });
        let lobes = match (sky_policy, key) {
            (Some(sky), Some((sun, radiance))) => Some((
                sun,
                radiance,
                f64::from(sky.sun_glow_gain_bp) / 10_000.0,
                policy.and_then(|p| p.atmosphere).map_or(0.0, |a| f64::from(a.sun_scatter_gain_bp) / 10_000.0),
            )),
            _ => None,
        };
        let analytic = match model {
            Some(SkyModel::Analytic { turbidity_milli }) => {
                let (sun, _) =
                    key.ok_or_else(|| GraphicsContractError::malformed("analytic sky IBL needs a directional key light"))?;
                if sun[1] <= 0.0 {
                    return Err(GraphicsContractError::malformed(
                        "analytic sky IBL needs the key light above the horizon",
                    ));
                }
                let turbidity = f64::from(turbidity_milli) / 1000.0;
                let theta_s = sun[1].clamp(-1.0, 1.0).acos();
                let y_hat = luminance(wide(sky_top_rgb)) / perez(turbidity, 1.0, theta_s, theta_s.cos());
                Some((turbidity, y_hat, sun))
            }
            _ => None,
        };
        Ok(Self { top: wide(sky_top_rgb), horizon: wide(sky_horizon_rgb), ground: wide(ground_rgb), analytic, lobes })
    }

    fn gradient(&self, d: V3) -> V3 {
        let w = if d[1] > 0.0 { d[1].sqrt() } else { 0.0 };
        mix(self.horizon, self.top, w)
    }

    /// Radiance along unit direction `d`: the sky (with its sun lobes) above
    /// the horizon, blended toward the ground below it.
    pub fn radiance(&self, d: V3) -> V3 {
        let base = self.base(d);
        let Some((sun, radiance, glow_gain, scatter_gain)) = self.lobes else {
            return base;
        };
        // The sky pass adds the glow (0.8 cos^32 + 0.2 cos^4) and the
        // atmosphere's Henyey-Greenstein lobe (g = 0.6) to the sky colour.
        let cos = dot(d, sun);
        let a = cos.max(0.0);
        let glow = glow_gain * (0.8 * a.powi(32) + 0.2 * a.powi(4));
        let g = 0.6;
        let denominator = (1.0 + g * g - 2.0 * g * cos).max(1e-4);
        let scatter = scatter_gain * (1.0 - g * g) / (denominator * denominator.sqrt());
        let lobe = radiance.map(|r| r * (glow + scatter));
        let vertical = d[1].clamp(-1.0, 1.0);
        let weight = if vertical >= 0.0 { 0.0 } else { (-vertical).sqrt() };
        // Below the horizon only the sky share of the blend carries the lobes.
        [0, 1, 2].map(|k| base[k] + lobe[k] * (1.0 - weight))
    }

    /// The shaders' `_sky_environment` (no sun lobes), in f64.
    pub fn base(&self, d: V3) -> V3 {
        let vertical = d[1].clamp(-1.0, 1.0);
        let sky = match self.analytic {
            None => {
                if vertical >= 0.0 {
                    return self.gradient(d);
                }
                return mix(self.horizon, self.ground, (-vertical).sqrt());
            }
            Some((turbidity, y_hat, sun)) => {
                let flat = normalize([d[0], d[1].max(0.0), d[2]]);
                let cos_gamma = dot(flat, sun).clamp(-1.0, 1.0);
                let lum = y_hat * perez(turbidity, flat[1].max(0.001), cos_gamma.acos(), cos_gamma);
                let hue = self.gradient(flat);
                let scale = lum / luminance(hue).max(1e-6);
                [hue[0] * scale, hue[1] * scale, hue[2] * scale]
            }
        };
        if vertical >= 0.0 { sky } else { mix(sky, self.ground, (-vertical).sqrt()) }
    }
}

fn direction_xyz_toward(direction: [f32; 3]) -> V3 {
    direction.map(|v| -f64::from(v))
}

// ------------------------------------------------------------------ maps

fn sign(v: f64) -> f64 {
    if v >= 0.0 { 1.0 } else { -1.0 }
}

/// Octahedral encode (+Y up) of a unit direction to [0, 1]². Mirrors `LavaAdapter._octahedral_uv`.
pub fn octahedral_encode(d: V3) -> [f64; 2] {
    let s = d[0].abs() + d[1].abs() + d[2].abs();
    let (mut x, mut z) = (d[0] / s, d[2] / s);
    if d[1] < 0.0 {
        (x, z) = ((1.0 - z.abs()) * sign(x), (1.0 - x.abs()) * sign(z));
    }
    [x * 0.5 + 0.5, z * 0.5 + 0.5]
}

pub fn octahedral_decode(uv: [f64; 2]) -> V3 {
    let (mut x, mut z) = (uv[0] * 2.0 - 1.0, uv[1] * 2.0 - 1.0);
    let y = 1.0 - x.abs() - z.abs();
    if y < 0.0 {
        (x, z) = ((1.0 - z.abs()) * sign(x), (1.0 - x.abs()) * sign(z));
    }
    normalize([x, y, z])
}

/// Fold a coordinate just outside [0, 1]² back across the octahedral border
/// (mirror across the edge, flip the other axis): the gutter rule.
fn fold(mut uv: [f64; 2]) -> [f64; 2] {
    if uv[0] < 0.0 { uv = [-uv[0], 1.0 - uv[1]]; } else if uv[0] > 1.0 { uv = [2.0 - uv[0], 1.0 - uv[1]]; }
    if uv[1] < 0.0 { uv = [1.0 - uv[0], -uv[1]]; } else if uv[1] > 1.0 { uv = [1.0 - uv[0], 2.0 - uv[1]]; }
    uv
}

fn radical_inverse(mut bits: u32) -> f64 {
    bits = bits.reverse_bits();
    f64::from(bits) / 4_294_967_296.0
}

/// GGX importance sample of a half vector around +Z for alpha = roughness².
fn ggx_half(i: u32, n: u32, alpha: f64) -> V3 {
    let (u1, u2) = (f64::from(i) / f64::from(n), radical_inverse(i));
    let phi = 2.0 * std::f64::consts::PI * u1;
    let cos_theta = ((1.0 - u2) / (1.0 + (alpha * alpha - 1.0) * u2)).sqrt();
    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
    [sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta]
}

fn basis(n: V3) -> (V3, V3) {
    let up = if n[1].abs() < 0.999 { [0.0, 1.0, 0.0] } else { [1.0, 0.0, 0.0] };
    let t = normalize([up[1] * n[2] - up[2] * n[1], up[2] * n[0] - up[0] * n[2], up[0] * n[1] - up[1] * n[0]]);
    let b = [n[1] * t[2] - n[2] * t[1], n[2] * t[0] - n[0] * t[2], n[0] * t[1] - n[1] * t[0]];
    (t, b)
}

/// GGX-prefiltered radiance around `n` (split-sum, N = V = R).
pub fn prefiltered(env: &IblEnvironment, n: V3, roughness: f64) -> V3 {
    if roughness <= 0.0 {
        return env.radiance(n);
    }
    let alpha = roughness * roughness;
    let (t, b) = basis(n);
    let mut sum = [0.0; 3];
    let mut weight = 0.0;
    for i in 0..PREFILTER_SAMPLES {
        let h_t = ggx_half(i, PREFILTER_SAMPLES, alpha);
        let h = normalize([
            t[0] * h_t[0] + b[0] * h_t[1] + n[0] * h_t[2],
            t[1] * h_t[0] + b[1] * h_t[1] + n[1] * h_t[2],
            t[2] * h_t[0] + b[2] * h_t[1] + n[2] * h_t[2],
        ]);
        let vh = dot(n, h);
        let l = [2.0 * vh * h[0] - n[0], 2.0 * vh * h[1] - n[1], 2.0 * vh * h[2] - n[2]];
        let nl = dot(n, l);
        if nl > 0.0 {
            let c = env.radiance(normalize(l));
            for k in 0..3 {
                sum[k] += c[k] * nl;
            }
            weight += nl;
        }
    }
    sum.map(|s| s / weight.max(1e-12))
}

fn sh9(d: V3) -> [f64; 9] {
    let [x, y, z] = d;
    [
        0.282_095,
        0.488_603 * y,
        0.488_603 * z,
        0.488_603 * x,
        1.092_548 * x * y,
        1.092_548 * y * z,
        0.315_392 * (3.0 * z * z - 1.0),
        1.092_548 * x * z,
        0.546_274 * (x * x - y * y),
    ]
}

/// Irradiance / π as SH9 (cosine lobe convolution, Ramamoorthi & Hanrahan 2001).
pub fn irradiance_sh(env: &IblEnvironment) -> [[f64; 3]; 9] {
    let mut c = [[0.0; 3]; 9];
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    for i in 0..SH_SAMPLES {
        let y = 1.0 - 2.0 * (f64::from(i) + 0.5) / f64::from(SH_SAMPLES);
        let r = (1.0 - y * y).sqrt();
        let phi = golden * f64::from(i);
        let d = [r * phi.cos(), y, r * phi.sin()];
        let radiance = env.radiance(d);
        for (k, basis) in sh9(d).iter().enumerate() {
            for ch in 0..3 {
                c[k][ch] += radiance[ch] * basis * 4.0 * std::f64::consts::PI / f64::from(SH_SAMPLES);
            }
        }
    }
    // Â_l / π for l = 0, 1, 2.
    let band = [1.0, 2.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0, 0.25, 0.25, 0.25, 0.25, 0.25];
    for (k, factor) in band.iter().enumerate() {
        for ch in 0..3 {
            c[k][ch] *= factor;
        }
    }
    c
}

pub fn irradiance(sh: &[[f64; 3]; 9], n: V3) -> V3 {
    let basis = sh9(n);
    let mut e = [0.0; 3];
    for (k, b) in basis.iter().enumerate() {
        for ch in 0..3 {
            e[ch] += sh[k][ch] * b;
        }
    }
    e.map(|v| v.max(0.0))
}

/// Split-sum BRDF table entry (A, B) for (N·V, roughness): specular
/// reflectance = F0·A + B (Karis 2013, k = α/2 for IBL).
pub fn brdf_lut(nv: f64, roughness: f64) -> (f64, f64) {
    let alpha = roughness * roughness;
    let v = [(1.0 - nv * nv).max(0.0).sqrt(), 0.0, nv];
    let k = alpha / 2.0;
    let (mut a, mut b) = (0.0, 0.0);
    for i in 0..LUT_SAMPLES {
        let h = ggx_half(i, LUT_SAMPLES, alpha);
        let vh = dot(v, h);
        let l = [2.0 * vh * h[0] - v[0], 2.0 * vh * h[1] - v[1], 2.0 * vh * h[2] - v[2]];
        let (nl, nh) = (l[2], h[2]);
        if nl > 0.0 {
            let g = (nv / (nv * (1.0 - k) + k)) * (nl / (nl * (1.0 - k) + k));
            let g_vis = g * vh.max(0.0) / (nh * nv).max(1e-12);
            let fc = (1.0 - vh.max(0.0)).powi(5);
            a += (1.0 - fc) * g_vis;
            b += fc * g_vis;
        }
    }
    (a / f64::from(LUT_SAMPLES), b / f64::from(LUT_SAMPLES))
}

fn encode_srgb8(linear: f64) -> u8 {
    let c = linear.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// Bake both IBL textures for a packet body.
pub fn bake(body: &GraphicsScenePacketBody) -> Result<[TextureReference; 2], GraphicsContractError> {
    let env = IblEnvironment::from_body(body)?;
    let (origins, width, height) = atlas_layout();
    let mut atlas = vec![0u8; (width * height * 4) as usize];
    let sh = irradiance_sh(&env);
    let tiles = IBL_LEVEL_SIZES.iter().copied().chain(std::iter::once(IBL_IRRADIANCE_SIZE)).enumerate();
    for (k, size) in tiles {
        let is_irradiance = k == IBL_LEVEL_SIZES.len();
        let roughness = k as f64 / 5.0;
        for j in -1..=(size as i64) {
            for i in -1..=(size as i64) {
                let uv = fold([(i as f64 + 0.5) / f64::from(size), (j as f64 + 0.5) / f64::from(size)]);
                let d = octahedral_decode(uv);
                let radiance = if is_irradiance { irradiance(&sh, d) } else { prefiltered(&env, d, roughness) };
                let px = (origins[k] as i64 + 1 + i) as u32;
                let py = (1 + j) as u32;
                let o = ((py * width + px) * 4) as usize;
                for ch in 0..3 {
                    atlas[o + ch] = encode_srgb8(radiance[ch] / IBL_RADIANCE_SCALE);
                }
                atlas[o + 3] = 255;
            }
        }
    }
    let mut lut = Vec::with_capacity((IBL_LUT_SIZE * IBL_LUT_SIZE * 4) as usize);
    for j in 0..IBL_LUT_SIZE {
        for i in 0..IBL_LUT_SIZE {
            let nv = (f64::from(i) + 0.5) / f64::from(IBL_LUT_SIZE);
            let roughness = (f64::from(j) + 0.5) / f64::from(IBL_LUT_SIZE);
            let (a, b) = brdf_lut(nv, roughness);
            lut.extend([(a.clamp(0.0, 1.0) * 255.0).round() as u8, (b.clamp(0.0, 1.0) * 255.0).round() as u8, 0, 255]);
        }
    }
    Ok([
        procedural_texture(IBL_ENVIRONMENT_TEXTURE_ID, IBL_SOURCE_ARTIFACT_ID, TextureColorSpace::Srgb, width, height, atlas),
        procedural_texture(IBL_BRDF_LUT_TEXTURE_ID, IBL_SOURCE_ARTIFACT_ID, TextureColorSpace::Data, IBL_LUT_SIZE, IBL_LUT_SIZE, lut),
    ])
}

fn ibl_enabled(body: &GraphicsScenePacketBody) -> bool {
    body.render_policy.as_ref().and_then(|p| p.ibl).is_some_and(|ibl| ibl.enabled)
}

fn is_reserved(texture: &TextureReference) -> bool {
    texture.texture_id == IBL_ENVIRONMENT_TEXTURE_ID || texture.texture_id == IBL_BRDF_LUT_TEXTURE_ID
}

/// Replace any IBL textures in the body with a fresh bake when the policy
/// enables IBL; remove them otherwise. Call after lights, environment and
/// policy are final.
pub fn apply_ibl(body: &mut GraphicsScenePacketBody) -> Result<(), GraphicsContractError> {
    body.textures.retain(|texture| !is_reserved(texture));
    if ibl_enabled(body) {
        body.textures.extend(bake(body)?);
    }
    Ok(())
}

/// Packet validation: IBL textures are present exactly when IBL is enabled, and
/// are exactly the bake of this packet's environment.
pub fn validate_packet_ibl(body: &GraphicsScenePacketBody) -> Result<(), GraphicsContractError> {
    let present: Vec<&TextureReference> = body.textures.iter().filter(|t| is_reserved(t)).collect();
    if !ibl_enabled(body) {
        return if present.is_empty() {
            Ok(())
        } else {
            Err(GraphicsContractError::provenance("IBL textures are present but render_policy.ibl is not enabled"))
        };
    }
    let expected = bake(body)?;
    if present.len() != expected.len() || !expected.iter().all(|e| present.iter().any(|p| *p == e)) {
        return Err(GraphicsContractError::provenance(
            "IBL textures do not match the bake of this packet's environment",
        ));
    }
    Ok(())
}
