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
    /// NOTE: not yet executed by the renderer; the adapter refuses a non-zero
    /// value rather than accepting it and ignoring it.
    pub macro_variation_bp: i32,
    /// Macro-variation spatial frequency in milli-units per world metre.
    /// NOTE: not yet executed by the renderer, for the same reason.
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
        }
    }
}

/// A `RenderPolicy` with every field resolved. This is what the adapter binds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedRenderPolicy {
    pub grade: GradePolicy,
    pub bloom: BloomPolicy,
    pub vignette: VignettePolicy,
    pub dither: DitherPolicy,
    pub terrain_surface: TerrainSurfacePolicy,
    pub sampler: SamplerPolicy,
    pub shadow: ShadowPolicy,
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
    Ok(())
}

/// Validate the packet's policy section if present.
pub fn validate_packet_render_policy(
    body: &GraphicsScenePacketBody,
) -> Result<(), GraphicsContractError> {
    if let Some(policy) = &body.render_policy {
        validate_render_policy(policy)?;
    }
    Ok(())
}
