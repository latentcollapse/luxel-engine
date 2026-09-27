use serde_json::json;

use crate::{
    AcceptanceStatus, AffordanceContext, AffordanceStatus, AssetPart, AssetUse, Bounds3,
    EnterableClaim, PhysicalCalibration, PhysicalRole, PhysicalStatus,
    evaluate_enterable_affordance, evaluate_physical_acceptance, evaluate_structural_acceptance,
    inspect_glb,
};

fn triangle_glb() -> Vec<u8> {
    let positions = [
        0.0f32, 0.0, 0.0, // vertex 0
        1.0, 0.0, 0.0, // vertex 1
        0.0, 1.0, 0.0, // vertex 2
    ];
    let indices = [0u16, 1, 2];
    let mut binary = Vec::new();
    for value in positions {
        binary.extend_from_slice(&value.to_le_bytes());
    }
    for value in indices {
        binary.extend_from_slice(&value.to_le_bytes());
    }
    let json_length = binary.len();
    binary.resize((binary.len() + 3) & !3, 0);
    let document = json!({
        "asset": {"version": "2.0", "generator": "wge-test"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"name": "triangle", "mesh": 0}],
        "meshes": [{
            "name": "triangle",
            "primitives": [{
                "attributes": {"POSITION": 0},
                "indices": 1,
                "mode": 4
            }]
        }],
        "buffers": [{"byteLength": json_length}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 36},
            {"buffer": 0, "byteOffset": 36, "byteLength": 6}
        ],
        "accessors": [
            {
                "bufferView": 0,
                "componentType": 5126,
                "count": 3,
                "type": "VEC3",
                "min": [0.0, 0.0, 0.0],
                "max": [1.0, 1.0, 0.0]
            },
            {"bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR"}
        ]
    });
    let mut json_bytes = serde_json::to_vec(&document).expect("test JSON serializes");
    json_bytes.resize((json_bytes.len() + 3) & !3, b' ');
    let total_length = 12 + 8 + json_bytes.len() + 8 + binary.len();
    let mut glb = Vec::with_capacity(total_length);
    glb.extend_from_slice(b"glTF");
    glb.extend_from_slice(&2u32.to_le_bytes());
    glb.extend_from_slice(&(total_length as u32).to_le_bytes());
    glb.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4e4f_534au32.to_le_bytes());
    glb.extend_from_slice(&json_bytes);
    glb.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004e_4942u32.to_le_bytes());
    glb.extend_from_slice(&binary);
    glb
}

#[test]
fn inspection_is_deterministic_and_content_addressed() {
    let bytes = triangle_glb();
    let first = inspect_glb(&bytes).expect("valid generated GLB");
    let second = inspect_glb(&bytes).expect("same GLB parses twice");
    assert_eq!(first, second);
    assert!(
        first
            .identity
            .asset_id
            .ends_with(&first.identity.source_sha256)
    );
    assert_eq!(first.facts.total_vertex_count, 3);
    assert_eq!(first.facts.total_triangle_count, 1);
    assert_eq!(
        first.facts.primitives[0]
            .topology
            .as_ref()
            .unwrap()
            .boundary_edge_count,
        3
    );
    assert_eq!(first.facts.active_node_count, 1);
}

#[test]
fn malformed_container_is_rejected_before_json_parsing() {
    let mut bytes = triangle_glb();
    bytes[8] = bytes[8].wrapping_add(1);
    assert!(inspect_glb(&bytes).is_err());
    assert!(inspect_glb(b"glTF").is_err());
}

#[test]
fn structural_acceptance_does_not_infer_a_role_or_rigging() {
    let report = inspect_glb(&triangle_glb()).expect("valid GLB");
    let unspecified = evaluate_structural_acceptance(&report, AssetUse::Unspecified);
    assert_eq!(unspecified.status, AcceptanceStatus::Indeterminate);
    let character = evaluate_structural_acceptance(&report, AssetUse::Character);
    assert_eq!(character.status, AcceptanceStatus::RequiresWork);
}

#[test]
fn physical_limits_require_explicit_scale_and_axis_calibration() {
    let report = inspect_glb(&triangle_glb()).expect("valid GLB");
    let unknown = evaluate_physical_acceptance(&report, PhysicalRole::HighlandUnderstory, None)
        .expect("missing calibration is an indeterminate verdict");
    assert_eq!(unknown.status, PhysicalStatus::Indeterminate);
    let calibrated = evaluate_physical_acceptance(
        &report,
        PhysicalRole::HighlandUnderstory,
        Some(PhysicalCalibration {
            meters_per_unit: 1.0,
            vertical_axis: crate::Axis::Y,
            placed_scale: 1.0,
        }),
    )
    .expect("valid calibration");
    assert_eq!(calibrated.status, PhysicalStatus::Passed);
}

#[test]
fn enterable_affordance_is_remeasured_from_part_bounds() {
    let parts = vec![
        AssetPart {
            name: "left_wall".into(),
            bounds_local: test_bounds([-4.0, 0.0, 1.0], [-2.0, 5.0, 3.0]),
        },
        AssetPart {
            name: "right_wall".into(),
            bounds_local: test_bounds([2.0, 0.0, 1.0], [4.0, 5.0, 3.0]),
        },
    ];
    let claim = EnterableClaim {
        threshold_clear_m: 4.0,
        interior_ring_m: 3.0,
        local_bearing_degrees: 0.0,
        threshold_band_m: [0.0, 5.0],
        measured_against: None,
    };
    let result = evaluate_enterable_affordance(
        &parts,
        PhysicalRole::FactionFortification,
        Some(&claim),
        AffordanceContext {
            agent_radius_m: 1.0,
            max_climb_m: 0.5,
            meters_per_unit: 1.0,
            placed_scale: 1.0,
        },
    )
    .expect("claim and context are valid");
    assert_eq!(result.status, AffordanceStatus::Passed);
    assert_eq!(result.measured_threshold_m, Some(4.0));
}

fn test_bounds(min: [f64; 3], max: [f64; 3]) -> Bounds3 {
    Bounds3 {
        min,
        max,
        size: std::array::from_fn(|index| max[index] - min[index]),
    }
}
