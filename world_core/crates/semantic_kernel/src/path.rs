//! Path-length domain. Manhattan length of a two-point path versus a budget.

pub const OPERATION: &str = crate::ops::PATH_OP;
pub const GATE_ID: &str = crate::ops::PATH_GATE;
pub const REPAIR_CLASS: &str = "shorten_path";
pub const MEASUREMENT_SCHEMA: &str = "luxel.path-length/v0";

pub fn passes(length: i64, budget: i64) -> bool {
    length <= budget
}

pub fn explain(binding: &str, length: i64, budget: i64) -> String {
    format!(
        "Path {binding} has Manhattan length {length}, which exceeds budget {budget}. Shorten only that path declaration. Gate {GATE_ID} will be rerun."
    )
}
