//! Lane-overlap domain. The transaction kernel calls these names.
//! It does not decide whether a footprint intersects a lane.

pub const OPERATION: &str = "lane_overlap";
pub const GATE_ID: &str = "lane.footprint_clear";
pub const REPAIR_CLASS: &str = "move_placement_off_lane";

/// The gate consumes the worker's intersects flag. It does not recompute geometry.
pub fn passes(intersects: bool) -> bool {
    !intersects
}

pub fn explain(binding: &str, overlap_area: i64) -> String {
    format!(
        "Placement {binding} intersects the protected lane (overlap area {overlap_area}). Move only that placement declaration so its footprint no longer intersects the lane. The lane declaration stays. Gate {GATE_ID} will be rerun."
    )
}
