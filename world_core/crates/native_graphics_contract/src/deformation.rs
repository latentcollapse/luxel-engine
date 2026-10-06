//! Scene packet v7 — TetCage deformation extension (LUXEL_TETCAGE_SEAM_DESIGN §3 P1).
//!
//! P1 GATE, verbatim from the seam design: "existing suites byte-identical with
//! flag off; flagged run presents deformed frames with receipts."
//!
//! ## Why v7 is a separate schema and not a v6 field
//!
//! The deformation section changes what a packet MEANS, not just what it
//! carries: a receiver that ignores the field renders a static mesh, and one
//! that honours it does not. Those are different artifacts and must not share
//! a schema version. So v7 = v6 + `deformation`, and the two are MUTUALLY
//! EXCLUSIVE by validation: a v6 packet carrying a deformation section is
//! rejected, and a v7 packet missing one is rejected. Neither direction is
//! inferable by a receiver.
//!
//! ## The flag
//!
//! `LUXEL_TETCAGE_DEFORM_V7` gates whether a PRODUCER attaches a deformation
//! section. It does not gate validation — the validator accepts a well-formed
//! v7 packet whether or not the flag is set, because a flag is a producer
//! policy, and a receiver must be able to check a packet it did not produce.
//! With the flag off, producers emit v6 and the body has no `deformation` key,
//! so the canonical bytes are identical to v6 by construction (the field is
//! `Option` + `skip_serializing_if = "Option::is_none"`). Byte-identity is
//! therefore a property of the type, not of a test that happens to pass.
//!
//! ## Static fallback
//!
//! A flagged packet may declare a deformation the active backend cannot run.
//! That is a NORMAL outcome, not an error: the frame presents the undeformed
//! mesh and the receipt records `static_fallback = true` with the reason. The
//! contract makes fallback *evidence* rather than a silent degradation — a
//! receipt claiming deformed frames while falling back is detectable, and
//! `validate_frame_receipt` rejects a telemetry block that contradicts its own
//! packet (see `validate_deformation_receipt`).

use serde::{Deserialize, Serialize};

use crate::{BufferReference, GraphicsContractError, GraphicsScenePacketBody};

/// The v7 schema string. v6 remains [`crate::SCENE_PACKET_SCHEMA`].
pub const SCENE_PACKET_SCHEMA_V7: &str = "luxel.graphics-scene-packet/v7";

/// Producer-side feature flag. Unset or empty = OFF = v6 output, byte-identical
/// to the pre-P1 campaign.
pub const DEFORMATION_V7_ENV: &str = "LUXEL_TETCAGE_DEFORM_V7";

/// True when the producer flag is on. Read once per process by producers;
/// validation deliberately does NOT consult it (see module docs).
pub fn deformation_v7_enabled() -> bool {
    std::env::var_os(DEFORMATION_V7_ENV).is_some_and(|value| !value.is_empty())
}

/// Typed rejection reasons. Each maps to a STABLE error code, because a
/// rejection a caller cannot branch on is a string, not a type — the point of
/// "typed rejections" is that a supervisor can decide between "fix the scene"
/// and "fall back to static" without matching on prose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeformationRejection {
    /// Deformation names a mesh the packet does not carry.
    UnknownMesh,
    /// A required buffer has the wrong element count for the mesh.
    BufferLengthMismatch,
    /// A buffer's declared stride does not match its payload encoding.
    BufferStrideMismatch,
    /// instance count is zero.
    ZeroInstances,
    /// A numeric parameter is NaN or infinite.
    NonFiniteParameter,
    /// scale_m is not strictly positive (it is a divisor in the field).
    NonPositiveScale,
    /// The family is not implemented by this contract version.
    UnsupportedFamily,
    /// v6 packet carrying a deformation section, or v7 packet without one.
    SchemaDeformationMismatch,
    /// A frame receipt's deformation telemetry contradicts its packet.
    ReceiptContradictsPacket,
}

impl DeformationRejection {
    /// Stable machine-readable code. These are contract surface: they appear in
    /// receipts and in supervisor logs, so they are append-only.
    pub const fn code(self) -> &'static str {
        match self {
            Self::UnknownMesh => "deformation.unknown_mesh",
            Self::BufferLengthMismatch => "deformation.buffer_length_mismatch",
            Self::BufferStrideMismatch => "deformation.buffer_stride_mismatch",
            Self::ZeroInstances => "deformation.zero_instances",
            Self::NonFiniteParameter => "deformation.non_finite_parameter",
            Self::NonPositiveScale => "deformation.non_positive_scale",
            Self::UnsupportedFamily => "deformation.unsupported_family",
            Self::SchemaDeformationMismatch => "deformation.schema_mismatch",
            Self::ReceiptContradictsPacket => "deformation.receipt_contradicts_packet",
        }
    }

    /// Whether a receiver should fall back to the static mesh rather than
    /// treat this as a hard failure. This is the one decision a supervisor
    /// must make without prose, so it is part of the type rather than a
    /// convention: geometry-level rejections are scene bugs (fail), while
    /// capacity/capability rejections are runtime conditions (fall back).
    pub const fn is_static_fallback(self) -> bool {
        matches!(self, Self::UnsupportedFamily)
    }
}

pub fn deformation_error(
    rejection: DeformationRejection,
    message: impl Into<String>,
) -> GraphicsContractError {
    GraphicsContractError {
        code: rejection.code(),
        message: message.into(),
    }
}

/// The deformation families P0 certified. One family, deliberately: the
/// contract ships what has been MEASURED (Spiral H/P0 wind), and a family is
/// added here only when it has a parity pin. `UnsupportedFamily` is therefore
/// reachable for any future string, which is what keeps this list honest.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeformationFamily {
    Wind,
}

/// The deformation field, in the exact parameterisation P0 measured:
/// `t = clamp(y/scale, 0, 1); lean = A*scale*t*t*sin(phi + 2.1x/scale + 1.3z/scale)`.
/// Transcribed from TetLab H/E (the CUDA authority), matching the GLSL shader
/// cell for cell — the same constants the pinned comparators were measured at.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeformationField {
    pub family: DeformationFamily,
    pub amplitude: f32,
    pub phase_rad: f32,
    pub scale_m: f32,
}

/// The deformation binding for one mesh: the resident cage plus the per-vertex
/// barycentric bindings, exactly the A_param contract (0 B/frame).
///
/// `cage` holds 12 f32 per corner (x,y,z of corners a,b,c,d) and 4 corners per
/// tetrahedron, so `count == 4 * tet_count`. `corner_indices` and
/// `bary_weights` hold 4 entries per mesh vertex. The shared-cage invariant —
/// every instance reads the SAME cage, which is what makes 0 B/frame true —
/// is what makes `instances` a plain count rather than a list.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DeformationIntent {
    pub mesh_id: String,
    pub field: DeformationField,
    pub cage: BufferReference,
    pub corner_indices: BufferReference,
    pub bary_weights: BufferReference,
    pub instances: u32,
}

impl DeformationIntent {
    /// Byte cost of the resident state, i.e. what is uploaded ONCE at setup
    /// and never again. The per-frame cost is 0 by construction; `bytes_per_frame`
    /// in the telemetry is a MEASURED claim about that, and
    /// `validate_deformation_receipt` refuses a receipt that contradicts it.
    pub fn resident_bytes(&self) -> usize {
        self.cage.byte_length + self.corner_indices.byte_length + self.bary_weights.byte_length
    }

    pub fn cage_corner_count(&self) -> usize {
        self.cage.count / 3
    }

    /// Tetrahedron count implied by the cage arity.
    pub fn tet_count(&self) -> usize {
        self.cage.count / 12
    }
}

/// Deformation evidence carried in a frame receipt. Optional so that receipts
/// from a flag-off frame stay byte-identical to the pre-P1 campaign.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeformationTelemetry {
    pub instances: u32,
    pub cage_corners: u32,
    /// Measured deform+reconstruct wall for the frame, microseconds. This is
    /// the P0 economics number entering the product path.
    pub deform_wall_us: u64,
    /// Measured per-frame upload cost. MUST be 0 for the resident-cage contract;
    /// a non-zero value means the implementation broke that invariant and is a
    /// typed rejection rather than a curiosity.
    pub bytes_per_frame: u64,
    /// True when the frame presented the STATIC mesh because the deformation
    /// could not run. Fallback is evidence, not silence.
    pub static_fallback: bool,
    /// Present iff `static_fallback` — the typed reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
}

fn finite(value: f32) -> bool {
    value.is_finite()
}

fn validate_field(field: &DeformationField) -> Result<(), GraphicsContractError> {
    if !finite(field.amplitude) || !finite(field.phase_rad) || !finite(field.scale_m) {
        return Err(deformation_error(
            DeformationRejection::NonFiniteParameter,
            format!(
                "deformation field parameters must be finite (amplitude={}, phase_rad={}, scale_m={})",
                field.amplitude, field.phase_rad, field.scale_m
            ),
        ));
    }
    if field.scale_m <= 0.0 {
        return Err(deformation_error(
            DeformationRejection::NonPositiveScale,
            format!(
                "deformation scale_m must be strictly positive (it is a divisor in the field): {}",
                field.scale_m
            ),
        ));
    }
    Ok(())
}

fn buffer_len_matches(buffer: &BufferReference, expected: usize, role: &str) -> Result<(), GraphicsContractError> {
    if buffer.count != expected {
        return Err(deformation_error(
            DeformationRejection::BufferLengthMismatch,
            format!(
                "{} has {} elements, expected {} for this mesh",
                role, buffer.count, expected
            ),
        ));
    }
    if buffer.stride_bytes != 4 {
        return Err(deformation_error(
            DeformationRejection::BufferStrideMismatch,
            format!(
                "{} declares stride {} B, expected 4 (f32/u32 scalars)",
                role, buffer.stride_bytes
            ),
        ));
    }
    Ok(())
}

/// Validate the deformation binding against the packet that carries it.
///
/// Split out from the packet validator so it can be exercised directly with
/// synthetic bodies (every rejection is reachable without building a whole
/// scene) and so the v7 rules live in one place.
pub fn validate_deformation(
    body: &GraphicsScenePacketBody,
    intent: &DeformationIntent,
) -> Result<(), GraphicsContractError> {
    let Some(mesh) = body.meshes.iter().find(|m| m.mesh_id == intent.mesh_id) else {
        return Err(deformation_error(
            DeformationRejection::UnknownMesh,
            format!("deformation names mesh_id {} which the packet does not carry", intent.mesh_id),
        ));
    };

    validate_field(&intent.field)?;

    if intent.instances == 0 {
        return Err(deformation_error(
            DeformationRejection::ZeroInstances,
            "deformation declares zero instances; that is the static mesh spelled the long way",
        ));
    }

    // The cage is a flat f32 buffer, so `count` is ELEMENT count (3 floats
    // per corner), not corner count. Arity therefore has to be checked in
    // elements: 12 floats per tetrahedron (4 corners x 3). Getting this wrong
    // is easy and silent — it is the same class as F-P0.3 — so the relation is
    // spelled out rather than implied.
    if intent.cage.count == 0 || intent.cage.count % 12 != 0 {
        return Err(deformation_error(
            DeformationRejection::BufferLengthMismatch,
            format!(
                "cage element count {} is not a positive multiple of 12 (12 f32 per tetrahedron: 4 corners x 3)",
                intent.cage.count
            ),
        ));
    }
    if !matches!(intent.cage.payload, crate::BufferPayload::F32(_)) {
        return Err(deformation_error(
            DeformationRejection::BufferStrideMismatch,
            "cage payload must be f32",
        ));
    }
    if intent.cage.byte_length != intent.cage.count * 4 {
        return Err(deformation_error(
            DeformationRejection::BufferLengthMismatch,
            format!(
                "cage byte_length {} does not match 4 B x {} f32 elements",
                intent.cage.byte_length, intent.cage.count
            ),
        ));
    }

    let vertices = mesh.positions_m.len();
    buffer_len_matches(&intent.corner_indices, 4 * vertices, "corner_indices")?;
    buffer_len_matches(&intent.bary_weights, 4 * vertices, "bary_weights")?;

    // Weights must be non-negative and finite: a negative barycentric weight
    // produces geometry that is finite but wrong, which no error metric on the
    // output can distinguish from a legitimate field. This is the one
    // content check, and it exists because silent-wrong beats loud-wrong.
    if let crate::BufferPayload::F32(values) = &intent.bary_weights.payload {
        for (index, weight) in values.iter().enumerate() {
            if !finite(*weight) || *weight < 0.0 {
                return Err(deformation_error(
                    DeformationRejection::NonFiniteParameter,
                    format!(
                        "bary_weights[{}] = {} is not a finite non-negative weight",
                        index, weight
                    ),
                ));
            }
        }
    }

    Ok(())
}

/// Enforce the v6/v7 mutual exclusion. Called from the scene-packet validator.
pub fn validate_schema_deformation(body: &GraphicsScenePacketBody) -> Result<(), GraphicsContractError> {
    let is_v7 = body.schema_version == SCENE_PACKET_SCHEMA_V7;
    if !is_v7 && body.deformation.is_some() {
        return Err(deformation_error(
            DeformationRejection::SchemaDeformationMismatch,
            format!(
                "packet declares schema {} but carries a deformation section; v7 is required to carry deformation",
                body.schema_version
            ),
        ));
    }
    if is_v7 && body.deformation.is_none() {
        return Err(deformation_error(
            DeformationRejection::SchemaDeformationMismatch,
            "packet declares schema v7 but carries no deformation section; v7 exists to carry one",
        ));
    }
    Ok(())
}

/// Check that a frame receipt's deformation telemetry is consistent with the
/// packet it claims to have rendered.
///
/// This is what makes "presents deformed frames with receipts" a checkable
/// claim. A receipt may legitimately report `static_fallback = true`, but it
/// may not simultaneously claim a deform wall cost for frames it did not
/// deform, and it may not report per-frame bytes for a resident-cage contract.
pub fn validate_deformation_receipt(
    intent: Option<&DeformationIntent>,
    telemetry: Option<&DeformationTelemetry>,
) -> Result<(), GraphicsContractError> {
    match (intent, telemetry) {
        (None, None) => Ok(()),
        (None, Some(_)) => Err(deformation_error(
            DeformationRejection::ReceiptContradictsPacket,
            "frame receipt carries deformation telemetry but its packet declared no deformation",
        )),
        (Some(_), None) => Err(deformation_error(
            DeformationRejection::ReceiptContradictsPacket,
            "packet carries a deformation section but the frame receipt reports no deformation telemetry",
        )),
        (Some(intent), Some(telemetry)) => {
            if telemetry.static_fallback {
                if telemetry.deform_wall_us != 0 {
                    return Err(deformation_error(
                        DeformationRejection::ReceiptContradictsPacket,
                        format!(
                            "receipt reports static_fallback but also claims a {} us deform wall; a frame that did not deform has no deform cost",
                            telemetry.deform_wall_us
                        ),
                    ));
                }
                if telemetry.fallback_reason.is_none() {
                    return Err(deformation_error(
                        DeformationRejection::ReceiptContradictsPacket,
                        "receipt reports static_fallback without a fallback_reason; fallback must be evidence, not silence",
                    ));
                }
                return Ok(());
            }
            if telemetry.bytes_per_frame != 0 {
                return Err(deformation_error(
                    DeformationRejection::ReceiptContradictsPacket,
                    format!(
                        "receipt reports {} B/frame for a resident-cage deformation; the A_param contract is 0 B/frame and a non-zero value means the invariant broke",
                        telemetry.bytes_per_frame
                    ),
                ));
            }
            if telemetry.instances != intent.instances {
                return Err(deformation_error(
                    DeformationRejection::ReceiptContradictsPacket,
                    format!(
                        "receipt reports {} instances, packet declares {}",
                        telemetry.instances, intent.instances
                    ),
                ));
            }
            if telemetry.cage_corners as usize != intent.cage_corner_count() {
                return Err(deformation_error(
                    DeformationRejection::ReceiptContradictsPacket,
                    format!(
                        "receipt reports {} cage corners, packet carries {}",
                        telemetry.cage_corners,
                        intent.cage_corner_count()
                    ),
                ));
            }
            Ok(())
        }
    }
}