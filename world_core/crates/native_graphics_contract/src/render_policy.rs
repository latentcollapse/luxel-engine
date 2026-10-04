//! RenderPolicy — the typed execution channel between style intent and renderer state.
//!
//! ## Why this module exists
//!
//! The graphical parity audit (GP-01) found that `StylePlan` had a vocabulary and
//! no executor: `capability_for_axis` returned a capability *id string* and stopped,
//! so a `StyleProfile` could declare `lighting.shadow_softness = 2000`, validate,
//! be digested, be bound into a `ConstructionPlan` — and reach the renderer as
//! nothing. Every renderer behaviour was a hard-coded constant in `LavaAdapter.jl`.
//!
//! This module is the missing channel. It is deliberately NOT a universal
//! style compiler. It carries exactly the parameters the renderer *already
//! implements*, and nothing else. If a parameter is not here, it is not
//! styleable, and the honest report is "unsupported" — not a decorative schema
//! that silently does nothing.
//!
//! ## The identity argument (FR-0015 discipline)
//!
//! A comparator pin carries its (config, N, protocol) domain; widening a pin's
//! domain converts a measurement into fiction. The same applies here: a render
//! policy is only meaningful *against the renderer that consumed it*. So the
//! policy digest is bound into the packet body (hence the packet digest), and
//! the frame receipt carries the policy digest that produced the frame. A
//! receipt claiming policy A while the packet carries policy B is a
//! contradiction, and is rejected — the same shape as `validate_deformation_receipt`.
//!
//! ## Compatibility
//!
//! `render_policy` is `Option<RenderPolicy>` with `skip_serializing_if`, exactly
//! like `deformation`. Absent means "the renderer used its declared defaults",
//! so a v6 packet's canonical bytes are unchanged by this module's existence.
//! Byte-identity is a property of the type, not of a test that happens to pass.
//!
//! Defaults are *declared constants in this file*, mirrored by the adapter, and
//! both are covered by a cross-language test. A default that lives only in the
//! renderer is a constant nobody can audit.

use serde::{Deserialize, Serialize};

use crate::{GraphicsContractError, GraphicsScenePacketBody};

/// Basis points per whole (10000 = 1.0). Policy amounts are integers so the
/// canonical JSON is exact and the digest is stable across platforms — floats
/// in a digest-bearing struct are a portability hazard.
pub const POLICY_SCALE: i64 = 10_000;

/// Post-process grade: lift / gamma / gain, applied in display-referred space
/// after tone mapping. This is the smallest grade that is not a single
/// multiplier, because a single multiplier is already `exposure`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GradePolicy {
    /// Additive lift per channel in basis points of full scale. Range
    /// [-1000, 1000] — beyond that it is not a grade, it is a different image.
    pub lift_r_bp: i32,
    pub lift_g_bp: i32,
    pub lift_b_bp: i32,
    /// Power applied to display-referred colour. Range [2500, 40000] =
    /// [0.25, 4.0]. 10000 is identity.
    pub gamma_bp: i32,
    /// Multiplicative gain per channel in basis points. Range [0, 20000].
    pub gain_r_bp: i32,
    pub gain_g_bp: i32,
    pub gain_b_bp: i32,
    /// Saturation in basis points. 10000 = identity, 0 = monochrome.
    /// This is the single most style-load-bearing post knob: it is the
    /// difference between "dark photoreal" and "painterly" and it is honest
    /// to expose because the renderer genuinely implements it.
    pub saturation_bp: i32,
}

impl Default for GradePolicy {
    fn default() -> Self {
        Self {
            lift_r_bp: 0,
            lift_g_bp: 0,
            lift_b_bp: 0,
            gamma_bp: POLICY_SCALE as i32,
            gain_r_bp: POLICY_SCALE as i32,
            gain_g_bp: POLICY_SCALE as i32,
            gain_b_bp: POLICY_SCALE as i32,
            saturation_bp: POLICY_SCALE as i32,
        }
    }
}

/// Bloom: a threshold + knee + radius-less intensity. Deliberately minimal —
/// a "restrained bloom" that a style profile can turn up without being able to
/// invent a lens-flare grammar the renderer does not have.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BloomPolicy {
    /// Luminance above which bloom contributes, in basis points of full scale.
    pub threshold_bp: i32,
    /// Bloom contribution, basis points. 0 = off.
    pub intensity_bp: i32,
}

impl Default for BloomPolicy {
    fn default() -> Self {
        Self {
            threshold_bp: 9000,
            intensity_bp: 0,
        }
    }
}

/// Vignette: strength and radius in basis points. Radial darkening toward the
/// frame edge; the cheapest legitimate way to stop a frame reading as a
/// uniformly-lit technical capture.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct VignettePolicy {
    pub strength_bp: i32,
    /// Radius at which falloff begins, basis points of the half-diagonal.
    pub radius_bp: i32,
    /// Edge softness (the `1 - smoothstep` width), basis points.
    pub softness_bp: i32,
}

impl Default for VignettePolicy {
    fn default() -> Self {
        Self {
            strength_bp: 0,
            radius_bp: 6000,
            softness_bp: 3000,
        }
    }
}

/// Dither: the anti-banding term. `amplitude_bp` is in units of 1/255 of a
/// quantisation step, so 1000 = 1 LSB peak-to-peak. Deterministic ordered
/// dither only — a hash-based or temporal dither would make the certified
/// frame depend on a noise sequence, which is exactly the determinism this
/// engine is built on.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DitherPolicy {
    /// Peak amplitude in thousandths of a quantisation step. Range [0, 2000].
    pub amplitude_milli_lsb: i32,
}

impl Default for DitherPolicy {
    fn default() -> Self {
        // Default OFF, not on. The baseline captures are frozen and must stay
        // byte-identical until a candidate deliberately changes them; a default
        // that silently alters every frame is a default that lies in a receipt.
        Self {
            amplitude_milli_lsb: 0,
        }
    }
}

/// Terrain surface policy: the channel that ends whole-world 0..1 UVs.
///
/// UNITS, because they were wrong before and the wrongness was load-bearing.
/// An earlier revision of this field was named `uv_scale_milli` and documented
/// as "texture repeats per world metre", but the adapter has always applied it
/// as a MULTIPLIER ON THE WHOLE-FIELD 0..1 UV. Those are not the same unit: at
/// the documented 1000 (1.0 repeat/metre) a 96 m field would tile 96 times,
/// whereas the actual baseline is a single stretch. A style author reading the
/// old doc would have asked for 250 expecting 24 tiles and received a 4x
/// magnification of a stretched image. The name now says what it does.
///
/// The physically meaningful quantity (metres per texture repeat) is derived
/// from this value AND the packet's terrain extent, which is why it is not a
/// field here: it is not packet-independent, and a policy field must be.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TerrainSurfacePolicy {
    /// Texture repeats across the whole terrain extent, in milli-units.
    /// Range [1, 32000]. 1000 == the baseline single stretch; 8000 tiles the
    /// material 8 times across the field.
    pub uv_repeat_scale_milli: i32,
    /// Whether terrain sampling wraps (`true`) or clamps to the edge (`false`).
    /// Clamp is the baseline. Tiling REQUIRES wrap, and wrap without tiling is
    /// meaningless, so this is a separate flag rather than something inferred
    /// from the repeat scale — a clamped 8x tile would silently smear the
    /// border texel across most of the field.
    pub wrap_repeat: bool,
    /// Macro-variation amplitude applied to albedo, basis points. Range [0, 5000].
    /// Executed on layered terrain (N-4): albedo × (1 + amplitude × (2m − 1))
    /// for macro noise m. Packet validation refuses a non-zero value on a
    /// terrain without layers, which has no macro texture to sample.
    pub macro_variation_bp: i32,
    /// Macro noise frequency, milli-cycles per world metre (15 = one cycle per
    /// ~67 m). Also the frequency layer coverage `macro_ramp` terms sample at.
    pub macro_frequency_milli: i32,
}

impl Default for TerrainSurfacePolicy {
    fn default() -> Self {
        // 1000 milli == exactly one repeat == the whole-world 0..1 UV the
        // baseline already used, so the default reproduces current behaviour
        // bit-for-bit. Clamp likewise: the baseline sampler is
        // CLAMP_TO_EDGE, and REPEAT at uv == 1.0 would wrap to the first texel
        // and change the frozen captures.
        Self {
            uv_repeat_scale_milli: 1000,
            wrap_repeat: false,
            macro_variation_bp: 0,
            macro_frequency_milli: 40,
        }
    }
}

/// Shadow policy: the two properties the adapter previously hard-coded.
///
/// `darkness_bp` exists because the audit found the adapter returning
/// `0.25 + 0.75 * pcf` — a shadowed surface could never receive less than 25%
/// of direct light, so contact shadows were not merely absent but
/// UNREPRESENTABLE. Expressing the floor as policy means the baseline value
/// stays reproducible (7500 == the old 0.25 floor) while a candidate can
/// choose 10000 for a genuinely dark shadow.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShadowPolicy {
    /// How dark a fully-occluded sample gets, basis points of direct light.
    /// 10000 == full shadow; the baseline 7500 leaves a 25% floor.
    pub darkness_bp: i32,
    /// PCF tap spread in milli-texels of the shadow map. 1000 == the
    /// baseline single-texel spread; larger is softer.
    pub filter_radius_milli: i32,
}

impl Default for ShadowPolicy {
    fn default() -> Self {
        // 7500/1000 reproduces `0.25 + 0.75 * pcf` exactly.
        Self {
            darkness_bp: 7500,
            filter_radius_milli: 1000,
        }
    }
}

/// Sampler policy. Anisotropy is here because the adapter builds its own
/// sampler handle and currently hard-codes anisotropy off; the device's
/// `maxAnisotropy` is the ceiling and the receipt records what was requested.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SamplerPolicy {
    /// Anisotropic sample count, 1..=16. 1 = isotropic (current behaviour).
    pub anisotropy: u8,
}

impl Default for SamplerPolicy {
    fn default() -> Self {
        Self { anisotropy: 1 }
    }
}

/// Mesh surface policy: the mesh counterpart of `TerrainSurfacePolicy::wrap_repeat`.
///
/// The adapter has always sampled mesh materials with CLAMP_TO_EDGE because
/// every authored mesh carried parametric 0..1 UVs. Parametric UVs stretch a
/// texture across whatever the band happens to measure (a 0.16 m pedestal band
/// carried one texture across ~24.5 m of circumference, a ~150:1 smear — audit
/// MD-4). Metric UVs fix that, but metric UVs exceed 1.0 and therefore REQUIRE
/// a repeat wrap; with clamp they would smear the border texel instead.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MeshSurfacePolicy {
    /// `true` samples mesh materials with REPEAT, `false` with the baseline clamp.
    pub wrap_repeat: bool,
}

impl Default for MeshSurfacePolicy {
    fn default() -> Self {
        Self { wrap_repeat: false }
    }
}

/// View-relative shadow frustum fit.
///
/// ABSENT means the historical fit: one orthographic shadow frame enclosing the
/// whole terrain and every instance. That fit ties shadow resolution to world
/// size — extending a 96 m field to 480 m would drop the single 512² map from
/// ~3.4 to ~0.7 texels per metre — so a world cannot grow without this axis.
/// PRESENT fits the shadow frame to the camera frustum truncated at
/// `view_distance_m`, the classic single-cascade "shadow distance". Receivers
/// beyond it are unshadowed, which is the honest trade at one cascade.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ShadowFitPolicy {
    /// Far edge of the shadowed frustum slice, whole metres. Range [8, 2000].
    pub view_distance_m: i32,
}

/// View-direction sky.
///
/// ABSENT means the historical sky draw: a full-screen triangle whose three
/// vertices sample the environment at NDC y = -1 and y = 3 only, so the sky is
/// a screen-space blend of `ground_rgb` and `sky_top_rgb` that never uses
/// `sky_horizon_rgb` and ignores camera pitch. PRESENT evaluates the
/// environment gradient per pixel along the camera ray (so the horizon colour
/// sits on the real horizon), shows the horizon colour below the horizon, and
/// draws a sun disc and glow along the key light.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SkyPolicy {
    /// Angular radius of the visible sun disc, milli-degrees. Range [0, 5000].
    /// 0 draws no disc.
    pub sun_disc_radius_milli_deg: i32,
    /// Disc radiance as a multiple of the key light's colour × intensity, in
    /// basis points. Range [0, 400000] (0–40×).
    pub sun_disc_gain_bp: i32,
    /// Forward-scattering glow around the sun as a multiple of the key light's
    /// colour × intensity, basis points. Range [0, 20000] (0–2×).
    pub sun_glow_gain_bp: i32,
    /// Sky radiance model. ABSENT means `Gradient`, and the key is omitted
    /// from canonical JSON, so CONVERGE-0 packets keep their bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<SkyModel>,
}

/// How the sky's radiance is computed along a view ray.
///
/// `Gradient` is the CONVERGE-0 sky: horizon→zenith along `sqrt(ray.y)`, two
/// authored colours. It has no horizon glow and no anti-solar darkening, so it
/// reads as a backdrop. `Analytic` takes the sky's LUMINANCE from the Perez
/// distribution with the Preetham et al. (1999) turbidity fit, driven by the
/// sun's elevation: brighter toward the sun along the horizon, darker opposite
/// it. Its HUE stays the authored gradient: Preetham's chromaticity fits give a
/// salmon-to-magenta horizon at low turbidity (Zotti et al. 2007), measured
/// here at r/g/b 0.51/0.44/0.43 across the sun at T = 3.
///
/// Under `Analytic` the environment's `sky_top_rgb` is the zenith colour and
/// sets the brightness every other direction is relative to. The same function
/// drives ambient light and the atmosphere's in-scattering, so the sky the eye
/// sees and the light the materials receive cannot diverge.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SkyModel {
    /// An empty struct variant, not a unit variant: serde does not apply
    /// `deny_unknown_fields` to unit variants of an internally tagged enum, so
    /// `{"kind": "gradient", "turbidity_milli": 3000}` would be accepted.
    Gradient {},
    Analytic {
        /// Atmospheric turbidity, milli-units. Range [2000, 10000]: the span
        /// the Preetham fit was made over (2 = very clear, 10 = hazy). Outside
        /// it the fitted coefficients produce negative radiance.
        turbidity_milli: i32,
    },
}

/// Exponential, height-dependent aerial perspective (N-1).
///
/// ABSENT means the historical linear fog: `w = min(distance × fog_density,
/// 0.92)` toward a constant `fog_color_rgb`, which never converges on the sky
/// behind a distant surface.
///
/// PRESENT replaces it with extinction σ(h) = σ₀·exp(−k·h) above the world
/// datum (y = 0). Along a ray of length d from height h_c to h_p the optical
/// depth integrates in closed form:
///
/// τ = σ₀·exp(−k·h_c)·d·(1 − exp(−k·Δh))/(k·Δh),  Δh = h_p − h_c
///
/// and a surface fades by `1 − exp(−τ)` toward the light scattered into its
/// ray: the sky radiance along that same ray plus a Henyey–Greenstein forward
/// lobe toward the sun (`sun_scatter_gain_bp`). The sky is drawn with the same
/// lobe and is not itself fogged, since it already is the atmosphere along an
/// infinite ray. So a distant ridge converges on exactly the sky behind it,
/// haze toward the sun is warmer and brighter, and the zenith keeps its colour
/// however dense the ground haze is. `fog_color_rgb` and `fog_density` are not
/// used when this axis is present.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AtmospherePolicy {
    /// k, milli per metre. Range [0, 1000]: 0 is a homogeneous haze, 1000 a
    /// 1 m scale height.
    pub height_falloff_milli_per_m: i32,
    /// σ₀, extinction at the datum in basis points per metre (1 bp = 1e-4 m⁻¹,
    /// so 30 bp ≈ a 1 km visual range). Range [0, 1000].
    pub density_at_ground_bp: i32,
    /// Forward in-scattering toward the sun as a multiple of the key light's
    /// colour × intensity, basis points. Range [0, 20000] (0–2×).
    pub sun_scatter_gain_bp: i32,
}

/// Diagnostic overrides for calibration views (CALIBRATION-1).
///
/// PRESENT forces the albedo of every lit surface (terrain, layers and meshes)
/// to `albedo_override_bp / 10000` in linear light, so material identity can be
/// judged from roughness, normals, AO and metalness with colour removed.
/// Emission is untouched. ABSENT renders authored albedo, byte-identical.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DebugPolicy {
    /// Forced linear albedo, basis points. Range [0, 10000].
    pub albedo_override_bp: i32,
}

/// The complete typed policy set. Every field is `Option`, so an absent field
/// means "declared default" and the packet stays minimal — but the DEFAULTS are
/// here, not in the renderer, so they are auditable and testable.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RenderPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<GradePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bloom: Option<BloomPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vignette: Option<VignettePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dither: Option<DitherPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain_surface: Option<TerrainSurfacePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sampler: Option<SamplerPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<ShadowPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mesh_surface: Option<MeshSurfacePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_fit: Option<ShadowFitPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sky: Option<SkyPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atmosphere: Option<AtmospherePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<DebugPolicy>,
}

impl RenderPolicy {
    /// Resolve every optional field to its declared default. The adapter calls
    /// this so there is exactly ONE place where "what does absent mean" is
    /// decided, and it is this file rather than a shader.
    pub fn resolved(&self) -> ResolvedRenderPolicy {
        ResolvedRenderPolicy {
            grade: self.grade.unwrap_or_default(),
            bloom: self.bloom.unwrap_or_default(),
            vignette: self.vignette.unwrap_or_default(),
            dither: self.dither.unwrap_or_default(),
            terrain_surface: self.terrain_surface.unwrap_or_default(),
            sampler: self.sampler.unwrap_or_default(),
            shadow: self.shadow.unwrap_or_default(),
            mesh_surface: self.mesh_surface.unwrap_or_default(),
            shadow_fit: self.shadow_fit,
            sky: self.sky,
            atmosphere: self.atmosphere,
            debug: self.debug,
        }
    }
}

/// A `RenderPolicy` with every field resolved. This is what the adapter binds.
///
/// `shadow_fit`, `sky`, `atmosphere` and `debug` stay optional after
/// resolution: their absence selects a different code path (the historical
/// whole-world fit, screen-space sky, linear fog, authored albedo), not a
/// default parameter value, so there is no honest default to resolve to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedRenderPolicy {
    pub grade: GradePolicy,
    pub bloom: BloomPolicy,
    pub vignette: VignettePolicy,
    pub dither: DitherPolicy,
    pub terrain_surface: TerrainSurfacePolicy,
    pub sampler: SamplerPolicy,
    pub shadow: ShadowPolicy,
    pub mesh_surface: MeshSurfacePolicy,
    pub shadow_fit: Option<ShadowFitPolicy>,
    pub sky: Option<SkyPolicy>,
    pub atmosphere: Option<AtmospherePolicy>,
    pub debug: Option<DebugPolicy>,
}

impl Default for ResolvedRenderPolicy {
    fn default() -> Self {
        RenderPolicy::default().resolved()
    }
}

fn bounded_i32(
    value: i32,
    low: i32,
    high: i32,
    label: &str,
) -> Result<(), GraphicsContractError> {
    if value < low || value > high {
        return Err(GraphicsContractError::malformed(format!(
            "render policy {label} is {value}, outside [{low}, {high}]"
        )));
    }
    Ok(())
}

/// Validate a policy. Every bound is stated, not implied: a policy a renderer
/// silently clamps is a policy whose receipt lies about what was rendered.
pub fn validate_render_policy(policy: &RenderPolicy) -> Result<(), GraphicsContractError> {
    if let Some(grade) = &policy.grade {
        bounded_i32(grade.lift_r_bp, -1000, 1000, "grade.lift_r_bp")?;
        bounded_i32(grade.lift_g_bp, -1000, 1000, "grade.lift_g_bp")?;
        bounded_i32(grade.lift_b_bp, -1000, 1000, "grade.lift_b_bp")?;
        bounded_i32(grade.gamma_bp, 2500, 40000, "grade.gamma_bp")?;
        bounded_i32(grade.gain_r_bp, 0, 20000, "grade.gain_r_bp")?;
        bounded_i32(grade.gain_g_bp, 0, 20000, "grade.gain_g_bp")?;
        bounded_i32(grade.gain_b_bp, 0, 20000, "grade.gain_b_bp")?;
        bounded_i32(grade.saturation_bp, 0, 20000, "grade.saturation_bp")?;
    }
    if let Some(bloom) = &policy.bloom {
        bounded_i32(bloom.threshold_bp, 0, POLICY_SCALE as i32, "bloom.threshold_bp")?;
        bounded_i32(bloom.intensity_bp, 0, 8000, "bloom.intensity_bp")?;
    }
    if let Some(vignette) = &policy.vignette {
        bounded_i32(vignette.strength_bp, 0, 8000, "vignette.strength_bp")?;
        bounded_i32(vignette.radius_bp, 1000, 10000, "vignette.radius_bp")?;
        bounded_i32(vignette.softness_bp, 0, 8000, "vignette.softness_bp")?;
        if vignette.strength_bp > 0 && vignette.radius_bp >= 10000 {
            return Err(GraphicsContractError::malformed(
                "render policy vignette is fully engaged but its radius leaves no falloff",
            ));
        }
    }
    if let Some(dither) = &policy.dither {
        bounded_i32(
            dither.amplitude_milli_lsb,
            0,
            2000,
            "dither.amplitude_milli_lsb",
        )?;
    }
    if let Some(terrain) = &policy.terrain_surface {
        bounded_i32(
            terrain.uv_repeat_scale_milli,
            1,
            32000,
            "terrain_surface.uv_repeat_scale_milli",
        )?;
        bounded_i32(terrain.macro_variation_bp, 0, 5000, "terrain_surface.macro_variation_bp")?;
        bounded_i32(
            terrain.macro_frequency_milli,
            1,
            4000,
            "terrain_surface.macro_frequency_milli",
        )?;
        // Cross-field, not a per-field bound: asking to tile more than once
        // while sampling with CLAMP_TO_EDGE smears the border texel across most
        // of the field. That is a broken configuration, not a look.
        if terrain.uv_repeat_scale_milli > 1000 && !terrain.wrap_repeat {
            return Err(GraphicsContractError::malformed(format!(
                "render policy terrain tiles {} times but wrap_repeat is false: \
                 clamped tiling smears the border texel. Set wrap_repeat, or use \
                 uv_repeat_scale_milli = 1000",
                terrain.uv_repeat_scale_milli as f64 / 1000.0
            )));
        }
    }
    if let Some(sampler) = &policy.sampler {
        if sampler.anisotropy == 0 || sampler.anisotropy > 16 {
            return Err(GraphicsContractError::malformed(format!(
                "render policy sampler.anisotropy is {}, outside [1, 16]",
                sampler.anisotropy
            )));
        }
    }
    if let Some(shadow) = &policy.shadow {
        bounded_i32(shadow.darkness_bp, 0, POLICY_SCALE as i32, "shadow.darkness_bp")?;
        bounded_i32(
            shadow.filter_radius_milli,
            100,
            8000,
            "shadow.filter_radius_milli",
        )?;
    }
    if let Some(fit) = &policy.shadow_fit {
        bounded_i32(fit.view_distance_m, 8, 2000, "shadow_fit.view_distance_m")?;
    }
    if let Some(sky) = &policy.sky {
        bounded_i32(
            sky.sun_disc_radius_milli_deg,
            0,
            5000,
            "sky.sun_disc_radius_milli_deg",
        )?;
        bounded_i32(sky.sun_disc_gain_bp, 0, 400_000, "sky.sun_disc_gain_bp")?;
        bounded_i32(sky.sun_glow_gain_bp, 0, 20000, "sky.sun_glow_gain_bp")?;
        if let Some(SkyModel::Analytic { turbidity_milli }) = sky.model {
            bounded_i32(turbidity_milli, 2000, 10000, "sky.model.turbidity_milli")?;
        }
    }
    if let Some(debug) = &policy.debug {
        bounded_i32(debug.albedo_override_bp, 0, POLICY_SCALE as i32, "debug.albedo_override_bp")?;
    }
    if let Some(atmosphere) = &policy.atmosphere {
        bounded_i32(
            atmosphere.height_falloff_milli_per_m,
            0,
            1000,
            "atmosphere.height_falloff_milli_per_m",
        )?;
        bounded_i32(atmosphere.density_at_ground_bp, 0, 1000, "atmosphere.density_at_ground_bp")?;
        bounded_i32(atmosphere.sun_scatter_gain_bp, 0, 20000, "atmosphere.sun_scatter_gain_bp")?;
        // The haze fades distance into the sky actually behind it, which needs
        // a per-pixel sky to fade into; the screen-space sky has no view ray.
        if policy.sky.is_none() {
            return Err(GraphicsContractError::malformed(
                "render policy atmosphere requires the view-direction sky (render_policy.sky)",
            ));
        }
    }
    Ok(())
}

/// Validate the packet's policy section if present.
pub fn validate_packet_render_policy(
    body: &GraphicsScenePacketBody,
) -> Result<(), GraphicsContractError> {
    if let Some(policy) = &body.render_policy {
        validate_render_policy(policy)?;
        // Macro variation samples the layer set's macro noise texture; on a
        // single-material terrain there is nothing to sample, so the axis
        // would be decorative. Refuse rather than ignore.
        if policy.terrain_surface.is_some_and(|terrain| terrain.macro_variation_bp > 0)
            && body.terrain.layers.is_none()
        {
            return Err(GraphicsContractError::malformed(
                "render policy terrain_surface.macro_variation_bp needs a layered terrain (terrain.layers)",
            ));
        }
    }
    Ok(())
}
