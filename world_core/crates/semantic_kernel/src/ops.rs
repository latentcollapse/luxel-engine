//! Closed operation and gate inventory. Persistence may test membership.
//! It must not implement domain math.

pub const LANE_OP: &str = "lane_overlap";
pub const LANE_GATE: &str = "lane.footprint_clear";
pub const PATH_OP: &str = "path_length";
pub const PATH_GATE: &str = "path.within_budget";

pub fn is_registered_operation(id: &str) -> bool {
    id == LANE_OP || id == PATH_OP
}

pub fn is_registered_gate(id: &str) -> bool {
    id == LANE_GATE || id == PATH_GATE
}
