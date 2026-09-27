//! Semantic transaction kernel for the lane-overlap specimen.
//!
//! Julia returns measurements. This crate owns the predicate, the generation
//! pointer, and receipt minting. Authoring text is never executed here.

mod lane;
mod ops;
mod path;
mod semantic_ir;
mod store;
mod worker;

use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub use lane::{GATE_ID, OPERATION, REPAIR_CLASS};
pub use semantic_ir::normalize_semantic_ir;
pub use store::{STORE_SCHEMA, create_store, open_store};
pub use worker::{
    ErosionSupervisorOptions, Worker, julia_version, project_digest, run_erosion_supervisor,
    script_digest, solver_image,
};

pub const IR_SCHEMA: &str = "wge.semantic-ir/v0";
pub const MEASUREMENT_SCHEMA: &str = "wge.lane-overlap/v0";

#[derive(Debug)]
pub enum KernelFailure {
    Operator { code: String, detail: String },
    Provenance { code: String, detail: String },
}

impl KernelFailure {
    pub fn operator(code: &str, detail: impl Into<String>) -> Self {
        Self::Operator {
            code: code.to_owned(),
            detail: detail.into(),
        }
    }

    pub fn provenance(code: &str, detail: impl Into<String>) -> Self {
        Self::Provenance {
            code: code.to_owned(),
            detail: detail.into(),
        }
    }

    pub fn class(&self) -> &'static str {
        match self {
            Self::Operator { .. } => "OperatorFailure",
            Self::Provenance { .. } => "ProvenanceFailure",
        }
    }

    pub fn code(&self) -> &str {
        match self {
            Self::Operator { code, .. } | Self::Provenance { code, .. } => code,
        }
    }

    pub fn detail(&self) -> &str {
        match self {
            Self::Operator { detail, .. } | Self::Provenance { detail, .. } => detail,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "event": "failure",
            "class": self.class(),
            "code": self.code(),
            "detail": self.detail(),
        })
    }
}

impl std::fmt::Display for KernelFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{} {}: {}",
            self.class(),
            self.code(),
            self.detail()
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Span {
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
}

#[derive(Clone, Debug)]
struct Node {
    binding: String,
    constructor: String,
    world_id: String,
    footprint: Rect,
    span: Span,
}

#[derive(Clone, Debug)]
struct Rect {
    x0: i64,
    x1: i64,
    y0: i64,
    y1: i64,
}

#[derive(Clone, Debug)]
struct PathDecl {
    binding: String,
    world_id: String,
    x0: i64,
    x1: i64,
    y0: i64,
    y1: i64,
    budget: i64,
    span: Span,
}

#[derive(Clone, Debug)]
pub struct SpecimenIr {
    pub module_id: String,
    pub registry_digest: String,
    lane: Node,
    place: Node,
    path: Option<PathDecl>,
    pub digest: String,
}

pub struct RunOptions {
    pub store: PathBuf,
    pub invalid_source: PathBuf,
    pub invalid_ir: PathBuf,
    pub repaired_source: PathBuf,
    pub repaired_ir: PathBuf,
    pub tampered_source: PathBuf,
    pub tampered_ir: PathBuf,
    pub registry: PathBuf,
    pub julia: PathBuf,
    pub project: PathBuf,
    pub manifest: PathBuf,
    pub worker_script: PathBuf,
    pub solver_image_override: Option<String>,
    pub stop_after: Option<String>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn sha256_prefixed(bytes: &[u8]) -> String {
    format!("sha256:{}", sha256_hex(bytes))
}

pub fn canonical_json(value: &Value) -> String {
    serde_json::to_string(&canonicalize(value)).expect("json values are serializable")
}

fn canonicalize(value: &Value) -> Value {
    match value {
        Value::Object(object) => {
            let mut ordered = std::collections::BTreeMap::new();
            for (key, child) in object {
                ordered.insert(key.clone(), canonicalize(child));
            }
            Value::Object(ordered.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}

pub fn registry_digest(path: &Path) -> Result<String, KernelFailure> {
    let bytes = fs::read(path).map_err(|error| {
        KernelFailure::provenance(
            "registry_unreadable",
            format!("{}: {error}", path.display()),
        )
    })?;
    Ok(sha256_prefixed(&bytes))
}

fn read_json(path: &Path) -> Result<Value, KernelFailure> {
    let text = fs::read_to_string(path).map_err(|error| {
        KernelFailure::operator("unreadable_input", format!("{}: {error}", path.display()))
    })?;
    serde_json::from_str(&text).map_err(|error| {
        KernelFailure::operator("malformed_ir", format!("{}: {error}", path.display()))
    })
}

fn object<'a>(value: &'a Value, path: &str) -> Result<&'a Map<String, Value>, KernelFailure> {
    value
        .as_object()
        .ok_or_else(|| KernelFailure::operator("malformed_ir", format!("{path} must be an object")))
}

fn require<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    path: &str,
) -> Result<&'a Value, KernelFailure> {
    object
        .get(key)
        .ok_or_else(|| KernelFailure::operator("malformed_ir", format!("{path}.{key} is required")))
}

fn reject_unknown(
    object: &Map<String, Value>,
    allowed: &[&str],
    path: &str,
) -> Result<(), KernelFailure> {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(KernelFailure::operator(
                "unknown_field",
                format!("{path}.{key} is not a closed field"),
            ));
        }
    }
    Ok(())
}

fn parse_span(value: &Value, path: &str) -> Result<Span, KernelFailure> {
    let object = object(value, path)?;
    reject_unknown(object, &["column", "end_column", "end_line", "line"], path)?;
    Ok(Span {
        line: usize_field(require(object, "line", path)?, &format!("{path}.line"))?,
        column: usize_field(require(object, "column", path)?, &format!("{path}.column"))?,
        end_line: usize_field(
            require(object, "end_line", path)?,
            &format!("{path}.end_line"),
        )?,
        end_column: usize_field(
            require(object, "end_column", path)?,
            &format!("{path}.end_column"),
        )?,
    })
}

fn usize_field(value: &Value, path: &str) -> Result<usize, KernelFailure> {
    let number = value.as_u64().ok_or_else(|| {
        KernelFailure::operator("malformed_ir", format!("{path} must be an integer"))
    })?;
    usize::try_from(number)
        .map_err(|_| KernelFailure::operator("malformed_ir", format!("{path} is out of range")))
}

fn i64_field(value: &Value, path: &str) -> Result<i64, KernelFailure> {
    value.as_i64().ok_or_else(|| {
        KernelFailure::operator("malformed_ir", format!("{path} must be an integer"))
    })
}

fn parse_rect(value: &Value, path: &str) -> Result<Rect, KernelFailure> {
    let object = object(value, path)?;
    reject_unknown(object, &["x0", "x1", "y0", "y1"], path)?;
    let rect = Rect {
        x0: i64_field(require(object, "x0", path)?, &format!("{path}.x0"))?,
        x1: i64_field(require(object, "x1", path)?, &format!("{path}.x1"))?,
        y0: i64_field(require(object, "y0", path)?, &format!("{path}.y0"))?,
        y1: i64_field(require(object, "y1", path)?, &format!("{path}.y1"))?,
    };
    if rect.x1 <= rect.x0 || rect.y1 <= rect.y0 {
        return Err(KernelFailure::operator(
            "degenerate_rectangle",
            format!("{path} has no area"),
        ));
    }
    Ok(rect)
}

fn parse_node(value: &Value, path: &str) -> Result<Node, KernelFailure> {
    let object = object(value, path)?;
    reject_unknown(
        object,
        &["binding", "constructor", "footprint", "id", "span"],
        path,
    )?;
    let constructor = require(object, "constructor", path)?
        .as_str()
        .ok_or_else(|| {
            KernelFailure::operator(
                "malformed_ir",
                format!("{path}.constructor must be a string"),
            )
        })?
        .to_owned();
    if constructor != "lane" && constructor != "place" {
        return Err(KernelFailure::operator(
            "unknown_constructor",
            format!("{constructor} is outside the specimen vocabulary"),
        ));
    }
    Ok(Node {
        binding: require(object, "binding", path)?
            .as_str()
            .unwrap_or("")
            .to_owned(),
        constructor,
        world_id: require(object, "id", path)?
            .as_str()
            .unwrap_or("")
            .to_owned(),
        footprint: parse_rect(
            require(object, "footprint", path)?,
            &format!("{path}.footprint"),
        )?,
        span: parse_span(require(object, "span", path)?, &format!("{path}.span"))?,
    })
}

pub fn load_ir(path: &Path, source: &str, registry: &str) -> Result<SpecimenIr, KernelFailure> {
    let value = read_json(path)?;
    let raw_object = object(&value, "ir")?;
    reject_unknown(
        raw_object,
        &["declarations", "module_id", "registry_digest", "schema"],
        "ir",
    )?;
    let schema = require(raw_object, "schema", "ir")?.as_str().unwrap_or("");
    if schema != IR_SCHEMA {
        return Err(KernelFailure::operator(
            "unknown_constructor",
            format!("schema {schema} is not {IR_SCHEMA}"),
        ));
    }
    let registry_digest = require(raw_object, "registry_digest", "ir")?
        .as_str()
        .unwrap_or("")
        .to_owned();
    if registry_digest != registry {
        return Err(KernelFailure::provenance(
            "registry_mismatch",
            format!("ir registry {registry_digest} does not match pinned {registry}"),
        ));
    }
    let value = normalize_semantic_ir(&value)?;
    let object = object(&value, "ir")?;
    let declarations = require(object, "declarations", "ir")?
        .as_array()
        .ok_or_else(|| {
            KernelFailure::operator("malformed_ir", "ir.declarations must be an array")
        })?;
    if declarations.len() != 2 && declarations.len() != 3 {
        return Err(KernelFailure::operator(
            "malformed_ir",
            "specimen requires one lane, one place, and at most one path",
        ));
    }
    let mut lane = None;
    let mut place = None;
    let mut route = None;
    for (index, declaration) in declarations.iter().enumerate() {
        let label = format!("ir.declarations[{index}]");
        let constructor = declaration
            .get("constructor")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        match constructor {
            "lane" => {
                if lane.is_some() {
                    return Err(KernelFailure::operator("malformed_ir", "duplicate lane"));
                }
                lane = Some(parse_node(declaration, &label)?);
            }
            "place" => {
                if place.is_some() {
                    return Err(KernelFailure::operator("malformed_ir", "duplicate place"));
                }
                place = Some(parse_node(declaration, &label)?);
            }
            "path" => {
                if route.is_some() {
                    return Err(KernelFailure::operator("malformed_ir", "duplicate path"));
                }
                route = Some(parse_path(declaration, &label)?);
            }
            other => {
                return Err(KernelFailure::operator(
                    "unknown_constructor",
                    format!("{other} is outside the specimen vocabulary"),
                ));
            }
        }
    }
    let lane =
        lane.ok_or_else(|| KernelFailure::operator("malformed_ir", "specimen requires one lane"))?;
    let place = place
        .ok_or_else(|| KernelFailure::operator("malformed_ir", "specimen requires one place"))?;
    if lane.binding.is_empty()
        || place.binding.is_empty()
        || lane.world_id.is_empty()
        || place.world_id.is_empty()
    {
        return Err(KernelFailure::operator(
            "malformed_ir",
            "bindings and ids must be non-empty",
        ));
    }
    confirm_span(source, &lane)?;
    confirm_span(source, &place)?;
    if let Some(ref path_decl) = route {
        confirm_path_span(source, path_decl)?;
    }
    let canonical = canonical_json(&value);
    if canonical.is_empty() {
        return Err(KernelFailure::operator(
            "malformed_ir",
            "canonical IR was empty",
        ));
    }
    Ok(SpecimenIr {
        module_id: require(object, "module_id", "ir")?
            .as_str()
            .unwrap_or("")
            .to_owned(),
        registry_digest,
        lane,
        place,
        path: route,
        digest: sha256_prefixed(canonical.as_bytes()),
    })
}

fn parse_path(value: &Value, path: &str) -> Result<PathDecl, KernelFailure> {
    let object = object(value, path)?;
    reject_unknown(
        object,
        &[
            "binding",
            "budget",
            "constructor",
            "id",
            "span",
            "x0",
            "x1",
            "y0",
            "y1",
        ],
        path,
    )?;
    if require(object, "constructor", path)?.as_str() != Some("path") {
        return Err(KernelFailure::operator(
            "unknown_constructor",
            "expected path",
        ));
    }
    Ok(PathDecl {
        binding: require(object, "binding", path)?
            .as_str()
            .unwrap_or("")
            .to_owned(),
        world_id: require(object, "id", path)?
            .as_str()
            .unwrap_or("")
            .to_owned(),
        budget: i64_field(require(object, "budget", path)?, &format!("{path}.budget"))?,
        x0: i64_field(require(object, "x0", path)?, &format!("{path}.x0"))?,
        x1: i64_field(require(object, "x1", path)?, &format!("{path}.x1"))?,
        y0: i64_field(require(object, "y0", path)?, &format!("{path}.y0"))?,
        y1: i64_field(require(object, "y1", path)?, &format!("{path}.y1"))?,
        span: parse_span(require(object, "span", path)?, &format!("{path}.span"))?,
    })
}

fn confirm_path_span(source: &str, path: &PathDecl) -> Result<(), KernelFailure> {
    let actual = declaration_span(source, &path.binding, "path")?;
    if actual != path.span {
        return Err(KernelFailure::provenance(
            "span_mismatch",
            "path span does not match the declaration line",
        ));
    }
    Ok(())
}

fn confirm_span(source: &str, node: &Node) -> Result<(), KernelFailure> {
    let actual = declaration_span(source, &node.binding, &node.constructor)?;
    if actual != node.span {
        return Err(KernelFailure::provenance(
            "span_mismatch",
            format!("{} span does not match the declaration line", node.binding),
        ));
    }
    Ok(())
}

fn declaration_span(source: &str, binding: &str, constructor: &str) -> Result<Span, KernelFailure> {
    let prefix = format!("{binding} = {constructor}(");
    for (index, line) in source.lines().enumerate() {
        if line.starts_with(&prefix) && line.ends_with(')') {
            return Ok(Span {
                line: index + 1,
                column: 0,
                end_line: index + 1,
                end_column: line.len(),
            });
        }
    }
    Err(KernelFailure::provenance(
        "missing_node",
        format!("source has no {binding} = {constructor}(...) declaration"),
    ))
}

fn span_range(source: &str, span: &Span) -> Result<(usize, usize), KernelFailure> {
    if span.line != span.end_line {
        return Err(KernelFailure::operator(
            "multiline_declaration",
            "spike declarations are single-line",
        ));
    }
    let bytes = source.as_bytes();
    let mut line_no = 1usize;
    let mut line_start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            if line_no == span.line {
                return range_inside(line_start, index, span);
            }
            line_no += 1;
            line_start = index + 1;
        }
    }
    if line_no == span.line {
        return range_inside(line_start, bytes.len(), span);
    }
    Err(KernelFailure::provenance(
        "span_mismatch",
        "span line is outside the source",
    ))
}

fn range_inside(
    line_start: usize,
    line_end: usize,
    span: &Span,
) -> Result<(usize, usize), KernelFailure> {
    let start = line_start
        .checked_add(span.column)
        .ok_or_else(|| KernelFailure::provenance("span_mismatch", "span overflow"))?;
    let end = line_start
        .checked_add(span.end_column)
        .ok_or_else(|| KernelFailure::provenance("span_mismatch", "span overflow"))?;
    if start > line_end || end > line_end || start > end {
        return Err(KernelFailure::provenance(
            "span_mismatch",
            "span is outside its declaration line",
        ));
    }
    Ok((start, end))
}

fn outside(source: &str, span: &Span) -> Result<String, KernelFailure> {
    let (start, end) = span_range(source, span)?;
    let mut text = String::new();
    text.push_str(&source[..start]);
    text.push_str(&source[end..]);
    Ok(text)
}

pub fn authorize_edit(
    base: &str,
    edited: &str,
    binding: &str,
    constructor: &str,
) -> Result<(), KernelFailure> {
    let base_span = declaration_span(base, binding, constructor)?;
    let edited_span = declaration_span(edited, binding, constructor)?;
    if outside(base, &base_span)? != outside(edited, &edited_span)? {
        return Err(KernelFailure::provenance(
            "unauthorized_span",
            "edit changes source outside the placement declaration",
        ));
    }
    Ok(())
}

fn measurement_job(lane: &Rect, footprint: &Rect) -> Vec<u8> {
    let value = json!({
        "footprint": rect_value(footprint),
        "lane": rect_value(lane),
        "op": "lane_overlap"
    });
    canonical_json(&value).into_bytes()
}

fn rect_value(rect: &Rect) -> Value {
    json!({"x0": rect.x0, "x1": rect.x1, "y0": rect.y0, "y1": rect.y1})
}

#[derive(Clone, Debug)]
struct Measurement {
    intersects: bool,
    overlap: Rect,
    overlap_area: i64,
    raw: Vec<u8>,
    sha256: String,
}

fn ingest_measurement(raw: &[u8]) -> Result<Measurement, KernelFailure> {
    if let Ok(value) = serde_json::from_slice::<Value>(raw)
        && value.get("class").and_then(|class| class.as_str()) == Some("OperatorFailure")
    {
        return Err(KernelFailure::operator(
            value
                .get("code")
                .and_then(|code| code.as_str())
                .unwrap_or("worker_error"),
            value
                .get("detail")
                .and_then(|detail| detail.as_str())
                .unwrap_or("worker rejected the job"),
        ));
    }
    let value: Value = serde_json::from_slice(raw)
        .map_err(|error| KernelFailure::operator("malformed_measurement", error.to_string()))?;
    let object = object(&value, "measurement")?;
    reject_unknown(
        object,
        &["intersects", "overlap", "overlap_area", "schema"],
        "measurement",
    )?;
    if object.contains_key("gate_passed") || object.contains_key("receipt") {
        return Err(KernelFailure::operator(
            "forged_authority",
            "measurement must not carry a gate or a receipt",
        ));
    }
    let schema = require(object, "schema", "measurement")?
        .as_str()
        .unwrap_or("");
    if schema != MEASUREMENT_SCHEMA {
        return Err(KernelFailure::operator(
            "malformed_measurement",
            "unexpected measurement schema",
        ));
    }
    let intersects = require(object, "intersects", "measurement")?
        .as_bool()
        .ok_or_else(|| {
            KernelFailure::operator("malformed_measurement", "intersects must be a boolean")
        })?;
    let overlap_area = i64_field(
        require(object, "overlap_area", "measurement")?,
        "measurement.overlap_area",
    )?;
    let overlap = parse_overlap(require(object, "overlap", "measurement")?)?;
    if intersects != (overlap_area > 0) {
        return Err(KernelFailure::provenance(
            "inconsistent_measurement",
            "intersects does not agree with overlap_area",
        ));
    }
    if !intersects
        && (overlap.x0, overlap.x1, overlap.y0, overlap.y1, overlap_area) != (0, 0, 0, 0, 0)
    {
        return Err(KernelFailure::provenance(
            "inconsistent_measurement",
            "a miss must report a zero overlap box",
        ));
    }
    let canonical = canonical_json(&value);
    if canonical.as_bytes() != raw {
        return Err(KernelFailure::operator(
            "noncanonical_measurement",
            "kernel canonical form does not match the worker bytes",
        ));
    }
    let sha256 = sha256_prefixed(raw);
    Ok(Measurement {
        intersects,
        overlap,
        overlap_area,
        raw: raw.to_vec(),
        sha256,
    })
}

fn parse_overlap(value: &Value) -> Result<Rect, KernelFailure> {
    let object = object(value, "measurement.overlap")?;
    reject_unknown(object, &["x0", "x1", "y0", "y1"], "measurement.overlap")?;
    Ok(Rect {
        x0: i64_field(
            require(object, "x0", "measurement.overlap")?,
            "measurement.overlap.x0",
        )?,
        x1: i64_field(
            require(object, "x1", "measurement.overlap")?,
            "measurement.overlap.x1",
        )?,
        y0: i64_field(
            require(object, "y0", "measurement.overlap")?,
            "measurement.overlap.y0",
        )?,
        y1: i64_field(
            require(object, "y1", "measurement.overlap")?,
            "measurement.overlap.y1",
        )?,
    })
}

fn ingest_length(raw: &[u8]) -> Result<(i64, String, Vec<u8>), KernelFailure> {
    if let Ok(value) = serde_json::from_slice::<Value>(raw)
        && value.get("class").and_then(|class| class.as_str()) == Some("OperatorFailure")
    {
        return Err(KernelFailure::operator(
            value
                .get("code")
                .and_then(|code| code.as_str())
                .unwrap_or("worker_error"),
            value
                .get("detail")
                .and_then(|detail| detail.as_str())
                .unwrap_or("worker rejected the job"),
        ));
    }
    let value: Value = serde_json::from_slice(raw)
        .map_err(|error| KernelFailure::operator("malformed_measurement", error.to_string()))?;
    let object = object(&value, "measurement")?;
    reject_unknown(object, &["length", "schema"], "measurement")?;
    if object.contains_key("gate_passed") || object.contains_key("passed") {
        return Err(KernelFailure::operator(
            "forged_authority",
            "measurement must not carry a gate",
        ));
    }
    if require(object, "schema", "measurement")?.as_str() != Some(path::MEASUREMENT_SCHEMA) {
        return Err(KernelFailure::operator(
            "malformed_measurement",
            "unexpected measurement schema",
        ));
    }
    let length = i64_field(
        require(object, "length", "measurement")?,
        "measurement.length",
    )?;
    if length < 0 {
        return Err(KernelFailure::operator(
            "malformed_measurement",
            "length must be non-negative",
        ));
    }
    let canonical = canonical_json(&value);
    if canonical.as_bytes() != raw {
        return Err(KernelFailure::operator(
            "noncanonical_measurement",
            "kernel canonical form does not match the worker bytes",
        ));
    }
    Ok((length, sha256_prefixed(raw), raw.to_vec()))
}

fn path_job(decl: &PathDecl) -> Vec<u8> {
    canonical_json(&json!({
        "op": path::OPERATION,
        "path": {"x0": decl.x0, "x1": decl.x1, "y0": decl.y0, "y1": decl.y1}
    }))
    .into_bytes()
}

struct CertifiedOutput {
    operation: String,
    gate: String,
    sha256: String,
    raw: Vec<u8>,
}

fn predicate_passes(measurement: &Measurement) -> bool {
    lane::passes(measurement.intersects)
}

pub(crate) fn ident(prefix: &str, material: &str) -> String {
    format!("{prefix}-{}", &sha256_hex(material.as_bytes())[..16])
}

fn emit(value: Value) {
    println!("{}", canonical_json(&value));
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), KernelFailure> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)
        .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
    fs::rename(&tmp, path)
        .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
    Ok(())
}

fn write_json(path: &Path, value: &Value) -> Result<(), KernelFailure> {
    store::put_immutable(path, canonical_json(value).as_bytes())
}

pub fn current_generation(store: &Path) -> Result<String, KernelFailure> {
    Ok(store::open_store(store)?.generation_id)
}

fn store_evidence(
    store: &Path,
    candidate_id: &str,
    parent: &str,
    solver: &str,
    ir_digest: &str,
    proposal_id: &str,
    measurement: &Measurement,
) -> Result<String, KernelFailure> {
    store::put_blob(store, &measurement.sha256, &measurement.raw)?;
    let binding_id = ident(
        "B",
        &format!(
            "{candidate_id}\n{parent}\n{}\n{solver}\n{}",
            measurement.sha256, ir_digest
        ),
    );
    write_json(
        &store.join("bindings").join(format!("{binding_id}.json")),
        &json!({
            "binding_id": binding_id,
            "candidate_id": candidate_id,
            "evidence_sha256": measurement.sha256,
            "gate_id": GATE_ID,
            "input_digest": ir_digest,
            "operation": OPERATION,
            "parent_id": parent,
            "proposal_id": proposal_id,
            "schema": "wge.evidence-binding/v0",
            "solver_image": solver
        }),
    )?;
    Ok(binding_id)
}

fn repair_for(
    candidate_id: &str,
    parent_id: &str,
    authoring_digest: &str,
    registry: &str,
    solver: &str,
    ir: &SpecimenIr,
    measurement: &Measurement,
) -> Value {
    let failure_id = ident(
        "F",
        &format!("{candidate_id}\n{}\n{parent_id}", measurement.sha256),
    );
    json!({
        "allowed_repair_class": REPAIR_CLASS,
        "base_authoring_digest": authoring_digest,
        "candidate_id": candidate_id,
        "class": "SemanticRepair",
        "editable_spans": [span_json(&ir.place.span)],
        "explanation": lane::explain(&ir.place.binding, measurement.overlap_area),
        "failure_id": failure_id,
        "gate_id": GATE_ID,
        "measurement": {
            "intersects": measurement.intersects,
            "overlap_area": measurement.overlap_area,
            "sha256": measurement.sha256
        },
        "measurement_sha256": measurement.sha256,
        "node_id": ir.place.binding,
        "parent_id": parent_id,
        "registry_digest": registry,
        "rerun_predicate": GATE_ID,
        "schema": "wge.semantic-repair/v0",
        "solver_image": solver,
        "source_span": span_json(&ir.place.span),
        "world_id": ir.place.world_id
    })
}

fn span_json(span: &Span) -> Value {
    json!({
        "column": span.column,
        "end_column": span.end_column,
        "end_line": span.end_line,
        "line": span.line
    })
}

struct Solved {
    candidate_id: String,
    proposal_id: String,
    measurement: Measurement,
    ir: SpecimenIr,
    authoring_digest: String,
}

fn solve_candidate(
    store: &Path,
    worker: &mut Worker,
    source: &str,
    ir: SpecimenIr,
    solver: &str,
    parent: &str,
) -> Result<Solved, KernelFailure> {
    let authoring_digest = sha256_prefixed(source.as_bytes());
    let candidate_id = ident(
        "C",
        &format!(
            "{parent}\n{}\n{}\n{authoring_digest}",
            ir.digest, ir.registry_digest
        ),
    );
    let proposal_id = ident("P", &format!("{candidate_id}\n{authoring_digest}"));
    emit(json!({
        "authoring_digest": authoring_digest,
        "candidate_id": candidate_id,
        "effect": "generation.propose",
        "event": "candidate",
        "ir_digest": ir.digest,
        "module_id": ir.module_id,
        "owner": "rust-kernel",
        "parent_id": parent,
        "registry_digest": ir.registry_digest
    }));
    let job = measurement_job(&ir.lane.footprint, &ir.place.footprint);
    emit(json!({
        "effect": "solver.invoke",
        "event": "solver",
        "julia_op": "lane_overlap",
        "owner": "rust-kernel",
        "pid": worker.pid,
        "solver_image": solver
    }));
    let (raw, warm_us) = worker.request(&job)?;
    let measurement = ingest_measurement(&raw)?;
    store_evidence(
        store,
        &candidate_id,
        parent,
        solver,
        &ir.digest,
        &proposal_id,
        &measurement,
    )?;
    emit(json!({
        "event": "measurement",
        "intersects": measurement.intersects,
        "job_us": warm_us,
        "overlap": {
            "x0": measurement.overlap.x0,
            "x1": measurement.overlap.x1,
            "y0": measurement.overlap.y0,
            "y1": measurement.overlap.y1
        },
        "overlap_area": measurement.overlap_area,
        "owner": "julia-worker",
        "sha256": measurement.sha256
    }));
    Ok(Solved {
        candidate_id,
        proposal_id,
        measurement,
        ir,
        authoring_digest,
    })
}

fn reject_candidate(
    store: &Path,
    solved: &Solved,
    parent: &str,
    solver: &str,
) -> Result<Value, KernelFailure> {
    let repair = repair_for(
        &solved.candidate_id,
        parent,
        &solved.authoring_digest,
        &solved.ir.registry_digest,
        solver,
        &solved.ir,
        &solved.measurement,
    );
    let failure_id = repair["failure_id"].as_str().unwrap_or("").to_owned();
    write_json(
        &store.join("repairs").join(format!("{failure_id}.json")),
        &repair,
    )?;
    write_proposal(
        store,
        solved,
        parent,
        solver,
        OPERATION,
        "rejected",
        Some(&failure_id),
        None,
        None,
    )?;
    emit(json!({
        "class": "SemanticRepair",
        "event": "repair",
        "failure_id": failure_id,
        "gate_id": GATE_ID,
        "measurement_sha256": solved.measurement.sha256,
        "node_id": solved.ir.place.binding,
        "owner": "rust-kernel",
        "repair_class": REPAIR_CLASS,
        "result": "fail",
        "source_span": span_json(&solved.ir.place.span)
    }));
    Ok(repair)
}

#[allow(clippy::too_many_arguments)]
fn write_proposal(
    store: &Path,
    solved: &Solved,
    parent: &str,
    solver: &str,
    operation: &str,
    status: &str,
    repair_id: Option<&str>,
    generation_id: Option<&str>,
    receipt_id: Option<&str>,
) -> Result<(), KernelFailure> {
    write_json(
        &store
            .join("candidates")
            .join(format!("{}.json", solved.candidate_id)),
        &json!({
            "authoring_digest": solved.authoring_digest,
            "candidate_id": solved.candidate_id,
            "effect": "generation.propose",
            "ir_digest": solved.ir.digest,
            "measurement_sha256": solved.measurement.sha256,
            "parent_id": parent,
            "predicate": status,
            "proposal_id": solved.proposal_id,
            "registry_digest": solved.ir.registry_digest,
            "repair_id": repair_id,
            "schema": "wge.candidate/v0",
            "solver_image": solver,
            "status": status
        }),
    )?;
    write_json(
        &store
            .join("proposals")
            .join(format!("{}.json", solved.proposal_id)),
        &json!({
            "authoring_digest": solved.authoring_digest,
            "candidate_id": solved.candidate_id,
            "generation_id": generation_id,
            "ir_digest": solved.ir.digest,
            "measurement_sha256": solved.measurement.sha256,
            "operation": operation,
            "parent_id": parent,
            "proposal_id": solved.proposal_id,
            "receipt_id": receipt_id,
            "registry_digest": solved.ir.registry_digest,
            "repair_id": repair_id,
            "role": "world_author",
            "schema": "wge.proposal/v0",
            "solver_image": solver,
            "status": status
        }),
    )
}

fn commit_candidate(
    store: &Path,
    solved: &Solved,
    parent: &str,
    solver: &str,
) -> Result<(), KernelFailure> {
    let generation_id = store::semantic_generation_id(
        parent,
        &solved.ir.digest,
        &solved.measurement.sha256,
        solver,
        &solved.ir.registry_digest,
    );
    let receipt_id = ident(
        "R",
        &format!(
            "{generation_id}\n{}\n{GATE_ID}\n{solver}\n{}",
            solved.measurement.sha256, solved.ir.registry_digest
        ),
    );
    let receipt = json!({
        "gate_id": GATE_ID,
        "gate_result": "pass",
        "generation_id": generation_id,
        "ir_digest": solved.ir.digest,
        "measurement_sha256": solved.measurement.sha256,
        "minted_by": "rust-kernel",
        "operation": "generation.commit",
        "parent_id": parent,
        "receipt_id": receipt_id,
        "registry_digest": solved.ir.registry_digest,
        "schema": "wge.receipt/v0",
        "solver_image": solver
    });
    write_json(
        &store.join("receipts").join(format!("{receipt_id}.json")),
        &receipt,
    )?;
    write_proposal(
        store,
        solved,
        parent,
        solver,
        OPERATION,
        "committed",
        None,
        Some(&generation_id),
        Some(&receipt_id),
    )?;
    let generation = json!({
        "artifact_index": {OPERATION: solved.measurement.sha256},
        "commit_receipt_id": receipt_id,
        "determinism": "exact",
        "evidence_index": {GATE_ID: solved.measurement.sha256},
        "generation_id": generation_id,
        "ir_digest": solved.ir.digest,
        "measurement_sha256": solved.measurement.sha256,
        "parent_id": parent,
        "registry_digest": solved.ir.registry_digest,
        "schema": "wge.generation/v0",
        "seed": null,
        "solver_image": solver
    });
    write_json(
        &store
            .join("generations")
            .join(format!("{generation_id}.json")),
        &generation,
    )?;
    store::replace_pointer(store, &generation_id)?;
    emit(json!({
        "event": "predicate",
        "gate_id": GATE_ID,
        "owner": "rust-kernel",
        "result": "pass"
    }));
    emit(json!({
        "candidate_id": solved.candidate_id,
        "event": "receipt",
        "generation_id": generation_id,
        "measurement_sha256": solved.measurement.sha256,
        "minted_by": "rust-kernel",
        "operation": "generation.commit",
        "receipt_id": receipt_id,
        "solver_image": solver
    }));
    emit(json!({
        "event": "commit",
        "generation_id": generation_id,
        "owner": "rust-kernel",
        "parent_id": parent
    }));
    Ok(())
}

pub fn run_specimen(options: &RunOptions) -> Result<(), KernelFailure> {
    let registry = registry_digest(&options.registry)?;
    let version = julia_version(&options.julia)?;
    let solver = solver_image(
        &options.worker_script,
        &options.project.join("Project.toml"),
        &options.manifest,
        &version,
    )?;
    if let Some(claimed) = &options.solver_image_override
        && claimed != &solver
    {
        return Err(KernelFailure::provenance(
            "solver_image_mismatch",
            format!("claimed solver image {claimed} does not match pinned {solver}"),
        ));
    }
    let script_hash = script_digest(&options.worker_script)?;
    if !store::is_initialized(&options.store) {
        store::create_store(&options.store, &registry)?;
    }
    let opened = store::open_store(&options.store)?;
    emit(json!({
        "event": "current",
        "generation_id": opened.generation_id,
        "owner": "rust-kernel",
        "store_schema": STORE_SCHEMA
    }));
    if opened.generation_id != "G0" {
        let current_ir = opened
            .generation
            .get("ir_digest")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let current_solver = opened
            .generation
            .get("solver_image")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let repaired_source = fs::read_to_string(&options.repaired_source)
            .map_err(|error| KernelFailure::operator("unreadable_input", error.to_string()))?;
        let repaired_ir = load_ir(&options.repaired_ir, &repaired_source, &registry)?;
        if current_ir == repaired_ir.digest && current_solver == solver {
            emit(json!({
                "event": "idempotent",
                "generation_id": opened.generation_id,
                "owner": "rust-kernel"
            }));
            return Ok(());
        }
        return Err(KernelFailure::provenance(
            "unexpected_current",
            format!(
                "store is already at {} and was not reset",
                opened.generation_id
            ),
        ));
    }

    let invalid_source = fs::read_to_string(&options.invalid_source)
        .map_err(|error| KernelFailure::operator("unreadable_input", error.to_string()))?;
    let repaired_source = fs::read_to_string(&options.repaired_source)
        .map_err(|error| KernelFailure::operator("unreadable_input", error.to_string()))?;
    let tampered_source = fs::read_to_string(&options.tampered_source)
        .map_err(|error| KernelFailure::operator("unreadable_input", error.to_string()))?;
    let invalid_ir = load_ir(&options.invalid_ir, &invalid_source, &registry)?;
    let repaired_ir = load_ir(&options.repaired_ir, &repaired_source, &registry)?;
    load_ir(&options.tampered_ir, &tampered_source, &registry)?;

    let mut worker = Worker::spawn(&options.julia, &options.project, &options.worker_script)?;
    if worker.script_sha256 != script_hash {
        return Err(KernelFailure::provenance(
            "solver_image_mismatch",
            format!(
                "worker reported script {} but kernel hashed {script_hash}; stderr: {}",
                worker.script_sha256,
                worker.stderr_excerpt()
            ),
        ));
    }
    emit(json!({
        "cold_ms": worker.cold_ms,
        "event": "worker_ready",
        "pid": worker.pid,
        "script_sha256": script_hash,
        "solver_image": solver
    }));

    let first = solve_candidate(
        &options.store,
        &mut worker,
        &invalid_source,
        invalid_ir,
        &solver,
        "G0",
    )?;
    if predicate_passes(&first.measurement) {
        return Err(KernelFailure::operator(
            "specimen_shape",
            "the invalid document was supposed to intersect the lane",
        ));
    }
    emit(json!({
        "event": "predicate",
        "gate_id": GATE_ID,
        "owner": "rust-kernel",
        "result": "fail"
    }));
    reject_candidate(&options.store, &first, "G0", &solver)?;
    if options.stop_after.as_deref() == Some("repair") {
        emit(json!({"event": "stopped", "after": "repair", "generation_id": "G0"}));
        return Ok(());
    }
    let pointer_after_failure = fs::read(options.store.join("pointer.json"))
        .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
    atomic_write(
        &options.store.join("history").join("after_c1_pointer.json"),
        &pointer_after_failure,
    )?;
    let current = current_generation(&options.store)?;
    emit(json!({"event": "current", "generation_id": current, "owner": "rust-kernel"}));
    if current != "G0" {
        return Err(KernelFailure::provenance(
            "pointer_moved",
            "failed candidate moved the current pointer",
        ));
    }

    match authorize_edit(&invalid_source, &tampered_source, "blocked_keep", "place") {
        Err(failure) => emit(json!({
            "class": failure.class(),
            "code": failure.code(),
            "detail": failure.detail(),
            "event": "span_rejection",
            "owner": "rust-kernel"
        })),
        Ok(()) => {
            return Err(KernelFailure::operator(
                "span_check_missed",
                "tampered document was authorized",
            ));
        }
    }
    if current_generation(&options.store)? != "G0" {
        return Err(KernelFailure::provenance(
            "pointer_moved",
            "span rejection moved the current pointer",
        ));
    }

    authorize_edit(&invalid_source, &repaired_source, "blocked_keep", "place")?;
    let second = solve_candidate(
        &options.store,
        &mut worker,
        &repaired_source,
        repaired_ir,
        &solver,
        "G0",
    )?;
    if !predicate_passes(&second.measurement) {
        return Err(KernelFailure::operator(
            "specimen_shape",
            "the repaired document still intersects the lane",
        ));
    }
    if second.measurement.sha256 == first.measurement.sha256 {
        return Err(KernelFailure::provenance(
            "measurement_collision",
            "passing and failing measurements hashed identically",
        ));
    }
    commit_candidate(&options.store, &second, "G0", &solver)?;
    emit(json!({
        "event": "current",
        "generation_id": current_generation(&options.store)?,
        "julia_jobs": 2,
        "owner": "rust-kernel",
        "worker_pid": worker.pid
    }));
    emit(json!({
        "canonical_ir_sha256": second.ir.digest,
        "canonical_measurement_sha256": second.measurement.sha256,
        "event": "canonical_result",
        "failing_measurement_sha256": first.measurement.sha256
    }));
    emit(json!({
        "cold_ms": worker.cold_ms,
        "event": "latency"
    }));
    drop(worker);
    Ok(())
}

fn pinned_solver(options: &RunOptions) -> Result<(String, String), KernelFailure> {
    let version = julia_version(&options.julia)?;
    let solver = solver_image(
        &options.worker_script,
        &options.project.join("Project.toml"),
        &options.manifest,
        &version,
    )?;
    let script_hash = script_digest(&options.worker_script)?;
    Ok((solver, script_hash))
}

fn spawn_checked(options: &RunOptions, script_hash: &str) -> Result<Worker, KernelFailure> {
    let worker = Worker::spawn(&options.julia, &options.project, &options.worker_script)?;
    if worker.script_sha256 != script_hash {
        return Err(KernelFailure::provenance(
            "solver_image_mismatch",
            format!(
                "worker reported {} but kernel hashed {script_hash}",
                worker.script_sha256
            ),
        ));
    }
    emit(json!({
        "cold_ms": worker.cold_ms,
        "event": "worker_ready",
        "pid": worker.pid,
        "script_sha256": script_hash
    }));
    Ok(worker)
}

pub fn certify_source(options: &RunOptions) -> Result<(), KernelFailure> {
    let registry = registry_digest(&options.registry)?;
    let (solver, script_hash) = pinned_solver(options)?;
    if !store::is_initialized(&options.store) {
        store::create_store(&options.store, &registry)?;
    }
    let opened = store::open_store(&options.store)?;
    emit(
        json!({"event": "current", "generation_id": opened.generation_id, "owner": "rust-kernel"}),
    );
    let source = fs::read_to_string(&options.repaired_source)
        .map_err(|error| KernelFailure::operator("unreadable_input", error.to_string()))?;
    let ir = load_ir(&options.repaired_ir, &source, &registry)?;
    let parent = if opened.generation_id == "G0" {
        "G0".to_owned()
    } else {
        opened
            .generation
            .get("parent_id")
            .and_then(|value| value.as_str())
            .unwrap_or("G0")
            .to_owned()
    };
    let mut worker = spawn_checked(options, &script_hash)?;
    let result = certify_against(&options.store, &mut worker, &source, ir, &solver, &parent)?;
    drop(worker);
    match result {
        CertifyResult::Committed(id) => {
            emit(json!({"event": "current", "generation_id": id, "owner": "rust-kernel"}));
            Ok(())
        }
        CertifyResult::Rejected => {
            emit(
                json!({"event": "current", "generation_id": store::open_store(&options.store)?.generation_id, "owner": "rust-kernel"}),
            );
            Ok(())
        }
    }
}

enum CertifyResult {
    Committed(String),
    Rejected,
}

fn certify_against(
    store: &Path,
    worker: &mut Worker,
    source: &str,
    ir: SpecimenIr,
    solver: &str,
    parent: &str,
) -> Result<CertifyResult, KernelFailure> {
    let authoring_digest = sha256_prefixed(source.as_bytes());
    let candidate_id = ident(
        "C",
        &format!(
            "{parent}\n{}\n{}\n{authoring_digest}",
            ir.digest, ir.registry_digest
        ),
    );
    let proposal_id = ident("P", &format!("{candidate_id}\n{authoring_digest}"));
    emit(json!({
        "authoring_digest": authoring_digest,
        "candidate_id": candidate_id,
        "effect": "generation.propose",
        "event": "candidate",
        "ir_digest": ir.digest,
        "owner": "rust-kernel",
        "parent_id": parent
    }));
    let mut outputs = Vec::new();
    let mut failures = Vec::new();

    let lane_job = measurement_job(&ir.lane.footprint, &ir.place.footprint);
    emit(
        json!({"effect": "solver.invoke", "event": "solver", "julia_op": ops::LANE_OP, "owner": "rust-kernel", "pid": worker.pid, "solver_image": solver}),
    );
    let (raw, us) = worker.request(&lane_job)?;
    let lane_m = ingest_measurement(&raw)?;
    store_evidence(
        store,
        &candidate_id,
        parent,
        solver,
        &ir.digest,
        &proposal_id,
        &lane_m,
    )?;
    emit(
        json!({"event": "measurement", "job_us": us, "operation": ops::LANE_OP, "owner": "julia-worker", "sha256": lane_m.sha256, "intersects": lane_m.intersects, "overlap_area": lane_m.overlap_area}),
    );
    if predicate_passes(&lane_m) {
        emit(
            json!({"event": "predicate", "gate_id": ops::LANE_GATE, "owner": "rust-kernel", "result": "pass"}),
        );
        outputs.push(CertifiedOutput {
            operation: ops::LANE_OP.into(),
            gate: ops::LANE_GATE.into(),
            sha256: lane_m.sha256.clone(),
            raw: lane_m.raw.clone(),
        });
    } else {
        emit(
            json!({"event": "predicate", "gate_id": ops::LANE_GATE, "owner": "rust-kernel", "result": "fail"}),
        );
        let solved = Solved {
            candidate_id: candidate_id.clone(),
            proposal_id: proposal_id.clone(),
            measurement: lane_m,
            ir: ir.clone(),
            authoring_digest: authoring_digest.clone(),
        };
        failures.push(reject_candidate(store, &solved, parent, solver)?);
    }

    if let Some(ref decl) = ir.path {
        emit(
            json!({"effect": "solver.invoke", "event": "solver", "julia_op": path::OPERATION, "owner": "rust-kernel", "pid": worker.pid, "solver_image": solver}),
        );
        let (raw, us) = worker.request(&path_job(decl))?;
        let (length, sha, raw) = ingest_length(&raw)?;
        store::put_blob(store, &sha, &raw)?;
        let binding_id = ident(
            "B",
            &format!("{candidate_id}\n{parent}\n{sha}\n{solver}\n{}", ir.digest),
        );
        write_json(
            &store.join("bindings").join(format!("{binding_id}.json")),
            &json!({
                "binding_id": binding_id,
                "candidate_id": candidate_id,
                "evidence_sha256": sha,
                "gate_id": ops::PATH_GATE,
                "input_digest": ir.digest,
                "operation": path::OPERATION,
                "parent_id": parent,
                "proposal_id": proposal_id,
                "schema": "wge.evidence-binding/v0",
                "solver_image": solver
            }),
        )?;
        emit(
            json!({"event": "measurement", "job_us": us, "length": length, "operation": path::OPERATION, "owner": "julia-worker", "sha256": sha}),
        );
        if path::passes(length, decl.budget) {
            emit(
                json!({"event": "predicate", "gate_id": ops::PATH_GATE, "owner": "rust-kernel", "result": "pass"}),
            );
            outputs.push(CertifiedOutput {
                operation: path::OPERATION.into(),
                gate: path::GATE_ID.into(),
                sha256: sha,
                raw,
            });
        } else {
            emit(
                json!({"event": "predicate", "gate_id": ops::PATH_GATE, "owner": "rust-kernel", "result": "fail"}),
            );
            let failure_id = ident("F", &format!("{candidate_id}\n{sha}\n{parent}"));
            let repair = json!({
                "allowed_repair_class": path::REPAIR_CLASS,
                "base_authoring_digest": authoring_digest,
                "candidate_id": candidate_id,
                "class": "SemanticRepair",
                "editable_spans": [span_json(&decl.span)],
                "explanation": path::explain(&decl.binding, length, decl.budget),
                "failure_id": failure_id,
                "gate_id": ops::PATH_GATE,
                "measurement": {"length": length, "sha256": sha},
                "measurement_sha256": sha,
                "node_id": decl.binding,
                "parent_id": parent,
                "registry_digest": ir.registry_digest,
                "rerun_predicate": ops::PATH_GATE,
                "schema": "wge.semantic-repair/v0",
                "solver_image": solver,
                "source_span": span_json(&decl.span),
                "world_id": decl.world_id
            });
            write_json(
                &store.join("repairs").join(format!("{failure_id}.json")),
                &repair,
            )?;
            emit(json!({
                "class": "SemanticRepair",
                "event": "repair",
                "failure_id": failure_id,
                "gate_id": ops::PATH_GATE,
                "node_id": decl.binding,
                "owner": "rust-kernel",
                "repair_class": path::REPAIR_CLASS,
                "result": "fail",
                "source_span": span_json(&decl.span)
            }));
            failures.push(repair);
        }
    }

    if !failures.is_empty() {
        write_json(
            &store
                .join("candidates")
                .join(format!("{candidate_id}.json")),
            &json!({
                "authoring_digest": authoring_digest,
                "candidate_id": candidate_id,
                "effect": "generation.propose",
                "ir_digest": ir.digest,
                "parent_id": parent,
                "proposal_id": proposal_id,
                "schema": "wge.candidate/v0",
                "solver_image": solver,
                "status": "rejected"
            }),
        )?;
        write_json(
            &store.join("proposals").join(format!("{proposal_id}.json")),
            &json!({
                "authoring_digest": authoring_digest,
                "candidate_id": candidate_id,
                "generation_id": null,
                "ir_digest": ir.digest,
                "operation": if ir.path.is_some() { "lane_overlap+path_length" } else { ops::LANE_OP },
                "parent_id": parent,
                "proposal_id": proposal_id,
                "receipt_id": null,
                "registry_digest": ir.registry_digest,
                "role": "world_author",
                "schema": "wge.proposal/v0",
                "solver_image": solver,
                "status": "rejected"
            }),
        )?;
        return Ok(CertifyResult::Rejected);
    }

    let solved = Solved {
        candidate_id: candidate_id.clone(),
        proposal_id: proposal_id.clone(),
        measurement: Measurement {
            intersects: false,
            overlap: Rect {
                x0: 0,
                x1: 0,
                y0: 0,
                y1: 0,
            },
            overlap_area: 0,
            raw: outputs[0].raw.clone(),
            sha256: outputs[0].sha256.clone(),
        },
        ir: ir.clone(),
        authoring_digest,
    };
    let generation_id = commit_outputs(store, &solved, parent, solver, &outputs)?;
    Ok(CertifyResult::Committed(generation_id))
}

fn commit_outputs(
    store: &Path,
    solved: &Solved,
    parent: &str,
    solver: &str,
    outputs: &[CertifiedOutput],
) -> Result<String, KernelFailure> {
    if outputs.len() == 1 && outputs[0].operation == ops::LANE_OP {
        commit_candidate(store, solved, parent, solver)?;
        return current_generation(store);
    }
    let mut artifacts = std::collections::BTreeMap::new();
    let mut evidence = std::collections::BTreeMap::new();
    for output in outputs {
        artifacts.insert(output.operation.clone(), output.sha256.clone());
        evidence.insert(output.gate.clone(), output.sha256.clone());
    }
    let generation_id = store::semantic_generation_id_multi(
        parent,
        &solved.ir.digest,
        solver,
        &solved.ir.registry_digest,
        &artifacts,
        &evidence,
    );
    let mut receipt_index = serde_json::Map::new();
    for output in outputs {
        let receipt_id = store::receipt_id_for(
            &generation_id,
            &output.sha256,
            &output.gate,
            solver,
            &solved.ir.registry_digest,
        );
        let receipt = json!({
            "gate_id": output.gate,
            "gate_result": "pass",
            "generation_id": generation_id,
            "ir_digest": solved.ir.digest,
            "measurement_sha256": output.sha256,
            "minted_by": "rust-kernel",
            "operation": "generation.commit",
            "parent_id": parent,
            "receipt_id": receipt_id,
            "registry_digest": solved.ir.registry_digest,
            "schema": "wge.receipt/v0",
            "solver_image": solver
        });
        write_json(
            &store.join("receipts").join(format!("{receipt_id}.json")),
            &receipt,
        )?;
        receipt_index.insert(output.gate.clone(), Value::String(receipt_id.clone()));
        emit(json!({
            "event": "receipt",
            "gate_id": output.gate,
            "generation_id": generation_id,
            "measurement_sha256": output.sha256,
            "minted_by": "rust-kernel",
            "receipt_id": receipt_id
        }));
    }
    let operation = outputs
        .iter()
        .map(|output| output.operation.as_str())
        .collect::<Vec<_>>()
        .join("+");
    write_proposal(
        store,
        solved,
        parent,
        solver,
        &operation,
        "committed",
        None,
        Some(&generation_id),
        receipt_index
            .values()
            .next()
            .and_then(|value| value.as_str()),
    )?;
    let generation = json!({
        "artifact_index": Value::Object(artifacts.iter().map(|(k,v)| (k.clone(), Value::String(v.clone()))).collect()),
        "commit_receipt_id": null,
        "determinism": "exact",
        "evidence_index": Value::Object(evidence.iter().map(|(k,v)| (k.clone(), Value::String(v.clone()))).collect()),
        "generation_id": generation_id,
        "ir_digest": solved.ir.digest,
        "measurement_sha256": null,
        "parent_id": parent,
        "receipt_index": receipt_index,
        "registry_digest": solved.ir.registry_digest,
        "schema": "wge.generation/v0",
        "seed": null,
        "solver_image": solver
    });
    write_json(
        &store
            .join("generations")
            .join(format!("{generation_id}.json")),
        &generation,
    )?;
    store::replace_pointer(store, &generation_id)?;
    emit(
        json!({"event": "commit", "generation_id": generation_id, "owner": "rust-kernel", "parent_id": parent, "operations": outputs.iter().map(|o| o.operation.clone()).collect::<Vec<_>>()}),
    );
    Ok(generation_id)
}

pub fn apply_persisted_repair(options: &RunOptions) -> Result<(), KernelFailure> {
    let registry = registry_digest(&options.registry)?;
    let (solver, script_hash) = pinned_solver(options)?;
    let opened = store::open_store(&options.store)?;
    let mut repairs = Vec::new();
    for entry in fs::read_dir(options.store.join("repairs"))
        .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?
    {
        let entry =
            entry.map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
        if entry.path().extension().and_then(|ext| ext.to_str()) == Some("json") {
            repairs.push(read_json(&entry.path())?);
        }
    }
    if repairs.len() != 1 {
        return Err(KernelFailure::operator(
            "missing_repair",
            "expected exactly one persisted repair",
        ));
    }
    let repair = &repairs[0];
    let parent = repair
        .get("parent_id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if opened.generation_id != parent {
        return Err(KernelFailure::provenance(
            "stale_repair",
            format!(
                "repair parent {parent} is not the current generation {}",
                opened.generation_id
            ),
        ));
    }
    let base = fs::read_to_string(&options.invalid_source)
        .map_err(|error| KernelFailure::operator("unreadable_input", error.to_string()))?;
    let base_digest = sha256_prefixed(base.as_bytes());
    if repair
        .get("base_authoring_digest")
        .and_then(|value| value.as_str())
        != Some(base_digest.as_str())
    {
        return Err(KernelFailure::provenance(
            "stale_repair",
            "base authoring digest does not match the repair",
        ));
    }
    let repaired = fs::read_to_string(&options.repaired_source)
        .map_err(|error| KernelFailure::operator("unreadable_input", error.to_string()))?;
    let repair_class = repair
        .get("allowed_repair_class")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let node = repair
        .get("node_id")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if repair_class == path::REPAIR_CLASS {
        authorize_edit(&base, &repaired, node, "path")?;
    } else {
        authorize_edit(&base, &repaired, "blocked_keep", "place")?;
    }
    let ir = load_ir(&options.repaired_ir, &repaired, &registry)?;
    if repair.get("solver_image").and_then(|value| value.as_str()) != Some(solver.as_str()) {
        return Err(KernelFailure::provenance(
            "solver_image_mismatch",
            "repair was produced under a different solver image",
        ));
    }
    let mut worker = spawn_checked(options, &script_hash)?;
    if ir.path.is_some() {
        match certify_against(&options.store, &mut worker, &repaired, ir, &solver, parent)? {
            CertifyResult::Committed(id) => {
                emit(json!({"event": "current", "generation_id": id, "owner": "rust-kernel"}));
                Ok(())
            }
            CertifyResult::Rejected => Err(KernelFailure::operator(
                "specimen_shape",
                "persisted repair still fails a required gate",
            )),
        }
    } else {
        let solved = solve_candidate(&options.store, &mut worker, &repaired, ir, &solver, parent)?;
        if !predicate_passes(&solved.measurement) {
            return Err(KernelFailure::operator(
                "specimen_shape",
                "persisted repair still fails the gate",
            ));
        }
        commit_candidate(&options.store, &solved, parent, &solver)?;
        emit(
            json!({"event": "current", "generation_id": current_generation(&options.store)?, "owner": "rust-kernel"}),
        );
        Ok(())
    }
}

pub fn submit_equivalent(options: &RunOptions) -> Result<(), KernelFailure> {
    let registry = registry_digest(&options.registry)?;
    let (solver, script_hash) = pinned_solver(options)?;
    let opened = store::open_store(&options.store)?;
    let parent = opened
        .generation
        .get("parent_id")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            KernelFailure::provenance("identity_conflict", "current generation has no parent")
        })?
        .to_owned();
    let before = fs::read(
        options
            .store
            .join("generations")
            .join(format!("{}.json", opened.generation_id)),
    )
    .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
    let source = fs::read_to_string(&options.repaired_source)
        .map_err(|error| KernelFailure::operator("unreadable_input", error.to_string()))?;
    let ir = load_ir(&options.repaired_ir, &source, &registry)?;
    let authoring_digest = sha256_prefixed(source.as_bytes());
    let candidate_id = ident(
        "C",
        &format!(
            "{parent}\n{}\n{}\n{authoring_digest}",
            ir.digest, ir.registry_digest
        ),
    );
    let proposal_id = ident("P", &format!("{candidate_id}\n{authoring_digest}"));
    let mut worker = spawn_checked(options, &script_hash)?;
    if ir.path.is_some() {
        match certify_against(&options.store, &mut worker, &source, ir, &solver, &parent)? {
            CertifyResult::Committed(id) if id == opened.generation_id => {}
            CertifyResult::Committed(id) => {
                return Err(KernelFailure::provenance(
                    "not_equivalent",
                    format!(
                        "submission identifies {id}, not current {}",
                        opened.generation_id
                    ),
                ));
            }
            CertifyResult::Rejected => {
                return Err(KernelFailure::operator(
                    "specimen_shape",
                    "equivalent source did not pass the required gates",
                ));
            }
        }
    } else {
        let solved = solve_candidate(&options.store, &mut worker, &source, ir, &solver, &parent)?;
        if !predicate_passes(&solved.measurement) {
            return Err(KernelFailure::operator(
                "specimen_shape",
                "equivalent source did not pass the gate",
            ));
        }
        let generation_id = store::semantic_generation_id(
            &parent,
            &solved.ir.digest,
            &solved.measurement.sha256,
            &solver,
            &solved.ir.registry_digest,
        );
        if generation_id != opened.generation_id {
            return Err(KernelFailure::provenance(
                "not_equivalent",
                format!(
                    "submission identifies {generation_id}, not current {}",
                    opened.generation_id
                ),
            ));
        }
        commit_candidate(&options.store, &solved, &parent, &solver)?;
    }
    let after = fs::read(
        options
            .store
            .join("generations")
            .join(format!("{}.json", opened.generation_id)),
    )
    .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
    if before != after {
        return Err(KernelFailure::provenance(
            "identity_conflict",
            "equivalent submission changed generation bytes",
        ));
    }
    emit(json!({
        "authoring_digest": authoring_digest,
        "event": "equivalent",
        "generation_id": opened.generation_id,
        "proposal_id": proposal_id,
        "unchanged": true
    }));
    Ok(())
}

pub fn inspect_store(store: &Path) -> Result<(), KernelFailure> {
    let opened = store::open_store(store)?;
    let mut proposals = Vec::new();
    if let Ok(entries) = fs::read_dir(store.join("proposals")) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|ext| ext.to_str()) == Some("json") {
                proposals.push(read_json(&entry.path())?);
            }
        }
    }
    proposals.sort_by(|left, right| {
        left.get("proposal_id")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .cmp(
                right
                    .get("proposal_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
            )
    });
    emit(json!({
        "artifact_index": opened.generation.get("artifact_index"),
        "commit_receipt_id": opened.generation.get("commit_receipt_id"),
        "evidence_index": opened.generation.get("evidence_index"),
        "event": "inspect",
        "generation_id": opened.generation_id,
        "ir_digest": opened.generation.get("ir_digest"),
        "measurement_sha256": opened.generation.get("measurement_sha256"),
        "owner": "rust-kernel",
        "parent_id": opened.generation.get("parent_id"),
        "proposals": proposals,
        "receipt_index": opened.generation.get("receipt_index"),
        "solver_image": opened.generation.get("solver_image")
    }));
    Ok(())
}

pub fn reject_author_effect(effect: &str) -> Result<(), KernelFailure> {
    if effect == "generation.commit" || effect == "solver.invoke" {
        return Err(KernelFailure::operator(
            "effect_forbidden",
            format!("{effect} is kernel-owned and is not an author effect"),
        ));
    }
    if effect.starts_with("filesystem.")
        || effect.starts_with("process.")
        || effect.starts_with("network.")
        || effect.starts_with("package.")
    {
        return Err(KernelFailure::operator(
            "host_effect_not_semantic",
            format!("{effect} is a host effect, not a semantic effect"),
        ));
    }
    if effect != "generation.propose" {
        return Err(KernelFailure::operator(
            "unknown_effect",
            format!("{effect} is not in the specimen vocabulary"),
        ));
    }
    Ok(())
}

pub fn bind_evidence(
    store: &Path,
    candidate_id: &str,
    evidence_sha: &str,
) -> Result<(), KernelFailure> {
    let _ = store::open_store(store)?;
    let bytes = fs::read(store::evidence_path(store, evidence_sha)).map_err(|_| {
        KernelFailure::provenance("missing_evidence", format!("no blob for {evidence_sha}"))
    })?;
    if sha256_prefixed(&bytes) != evidence_sha {
        return Err(KernelFailure::provenance(
            "evidence_hash_mismatch",
            "stored bytes do not match the evidence id",
        ));
    }
    let mut matched = false;
    let dir = store.join("bindings");
    if dir.is_dir() {
        for entry in fs::read_dir(&dir)
            .map_err(|error| KernelFailure::operator("store_io", error.to_string()))?
        {
            let entry =
                entry.map_err(|error| KernelFailure::operator("store_io", error.to_string()))?;
            let value = read_json(&entry.path())?;
            let recorded = value
                .get("binding_id")
                .and_then(|item| item.as_str())
                .unwrap_or("");
            let path = entry.path();
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("")
                .to_owned();
            if recorded != stem {
                return Err(KernelFailure::provenance(
                    "identity_conflict",
                    "binding filename does not match binding_id",
                ));
            }
            let bound_candidate = value
                .get("candidate_id")
                .and_then(|item| item.as_str())
                .unwrap_or("");
            let parent = value
                .get("parent_id")
                .and_then(|item| item.as_str())
                .unwrap_or("");
            let bound_evidence = value
                .get("evidence_sha256")
                .and_then(|item| item.as_str())
                .unwrap_or("");
            let solver = value
                .get("solver_image")
                .and_then(|item| item.as_str())
                .unwrap_or("");
            let input = value
                .get("input_digest")
                .and_then(|item| item.as_str())
                .unwrap_or("");
            let recomputed = ident(
                "B",
                &format!("{bound_candidate}\n{parent}\n{bound_evidence}\n{solver}\n{input}"),
            );
            if recomputed != recorded {
                return Err(KernelFailure::provenance(
                    "identity_conflict",
                    "binding id does not match its candidate and evidence",
                ));
            }
            if bound_candidate == candidate_id && bound_evidence == evidence_sha {
                matched = true;
            }
        }
    }
    if !matched {
        return Err(KernelFailure::provenance(
            "stale_evidence",
            format!("no evidence binding ties {evidence_sha} to {candidate_id}"),
        ));
    }
    Ok(())
}

pub fn verify_current(store: &Path) -> Result<Value, KernelFailure> {
    let opened = store::open_store(store)?;
    let generation_id = opened.generation_id;
    let generation = opened.generation;
    if generation_id == "G0" {
        return Ok(generation);
    }
    let receipt_ids = generation_receipt_ids(&generation)?;
    let mut receipts = Vec::new();
    for receipt_id in &receipt_ids {
        receipts.push(verify_receipt(store, &generation_id, receipt_id)?);
    }
    if receipt_ids.len() == 1
        && generation
            .get("commit_receipt_id")
            .and_then(|id| id.as_str())
            .is_some()
    {
        return Ok(receipts.remove(0));
    }
    Ok(json!({
        "generation_id": generation_id,
        "receipt_ids": receipt_ids,
        "receipts": receipts,
        "verified": true
    }))
}

fn generation_receipt_ids(generation: &Value) -> Result<Vec<String>, KernelFailure> {
    if let Some(single) = generation
        .get("commit_receipt_id")
        .and_then(|id| id.as_str())
    {
        return Ok(vec![single.to_owned()]);
    }
    let index = generation
        .get("receipt_index")
        .and_then(|value| value.as_object())
        .ok_or_else(|| {
            KernelFailure::provenance("missing_receipt", "committed generation has no receipt")
        })?;
    if index.is_empty() {
        return Err(KernelFailure::provenance(
            "missing_receipt",
            "committed generation has no receipt",
        ));
    }
    index
        .values()
        .map(|value| {
            value.as_str().map(str::to_owned).ok_or_else(|| {
                KernelFailure::provenance(
                    "missing_receipt",
                    "receipt index contains a non-string id",
                )
            })
        })
        .collect()
}

fn verify_receipt(
    store: &Path,
    generation_id: &str,
    receipt_id: &str,
) -> Result<Value, KernelFailure> {
    let receipt = read_json(&store.join("receipts").join(format!("{receipt_id}.json")))?;
    let measurement_sha = receipt
        .get("measurement_sha256")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let name = measurement_sha.trim_start_matches("sha256:");
    let bytes = fs::read(store.join("evidence").join(format!("{name}.bin")))
        .map_err(|error| KernelFailure::provenance("missing_evidence", error.to_string()))?;
    let actual = sha256_prefixed(&bytes);
    if actual != measurement_sha {
        return Err(KernelFailure::provenance(
            "evidence_hash_mismatch",
            "kernel recomputed a different measurement hash than the receipt",
        ));
    }
    if receipt.get("minted_by").and_then(|value| value.as_str()) != Some("rust-kernel") {
        return Err(KernelFailure::provenance(
            "forged_receipt",
            "receipt was not minted by the rust kernel",
        ));
    }
    if receipt
        .get("generation_id")
        .and_then(|value| value.as_str())
        != Some(generation_id)
    {
        return Err(KernelFailure::provenance(
            "forged_receipt",
            "receipt generation does not match the pointer",
        ));
    }
    Ok(receipt)
}

pub fn accept_receipt(store: &Path, receipt_id: &str) -> Result<(), KernelFailure> {
    let opened = store::open_store(store)?;
    let allowed = generation_receipt_ids(&opened.generation)?;
    if !allowed.iter().any(|current| current == receipt_id) {
        return Err(KernelFailure::provenance(
            "forged_receipt",
            format!(
                "receipt {receipt_id} is not a receipt of {}",
                opened.generation_id
            ),
        ));
    }
    verify_current(store)?;
    Ok(())
}

pub fn check_solver_image(store: &Path, claimed: &str) -> Result<(), KernelFailure> {
    let generation_id = current_generation(store)?;
    let generation = read_json(
        &store
            .join("generations")
            .join(format!("{generation_id}.json")),
    )?;
    let actual = generation
        .get("solver_image")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if actual != claimed {
        return Err(KernelFailure::provenance(
            "solver_image_mismatch",
            format!("generation pins {actual}, not {claimed}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predicate_trusts_the_flag_without_solving_geometry() {
        let miss = Measurement {
            intersects: false,
            overlap: Rect {
                x0: 0,
                x1: 0,
                y0: 0,
                y1: 0,
            },
            overlap_area: 0,
            raw: Vec::new(),
            sha256: String::new(),
        };
        let hit = Measurement {
            intersects: true,
            overlap: Rect {
                x0: 1,
                x1: 2,
                y0: 3,
                y1: 4,
            },
            overlap_area: 9,
            raw: Vec::new(),
            sha256: String::new(),
        };
        assert!(predicate_passes(&miss));
        assert!(!predicate_passes(&hit));
    }

    #[test]
    fn forged_gate_field_is_rejected() {
        let raw = br#"{"gate_passed":true,"intersects":false,"overlap":{"x0":0,"x1":0,"y0":0,"y1":0},"overlap_area":0,"schema":"wge.lane-overlap/v0"}"#;
        let error = ingest_measurement(raw).expect_err("gate field");
        assert_eq!(error.class(), "OperatorFailure");
        assert_eq!(error.code(), "unknown_field");
    }

    #[test]
    fn kernel_recomputes_measurement_hash() {
        let raw = br#"{"intersects":true,"overlap":{"x0":40,"x1":70,"y0":40,"y1":60},"overlap_area":600,"schema":"wge.lane-overlap/v0"}"#;
        let measurement = ingest_measurement(raw).expect("measurement");
        assert_eq!(measurement.sha256, sha256_prefixed(raw));
        assert!(measurement.intersects);
    }

    #[test]
    fn author_cannot_request_commit_or_solver_invoke() {
        assert_eq!(
            reject_author_effect("generation.commit")
                .unwrap_err()
                .code(),
            "effect_forbidden"
        );
        assert_eq!(
            reject_author_effect("solver.invoke").unwrap_err().code(),
            "effect_forbidden"
        );
        assert_eq!(
            reject_author_effect("filesystem.write").unwrap_err().code(),
            "host_effect_not_semantic"
        );
        assert!(reject_author_effect("generation.propose").is_ok());
    }

    #[test]
    fn out_of_span_edit_is_rejected_and_in_span_edit_is_not() {
        let base = "from wge.world import lane, place\nfrom wge.geometry import rect\n\ncentral_lane = lane(id=\"central\", footprint=rect(x0=0, y0=40, x1=100, y1=60))\nblocked_keep = place(id=\"keep\", footprint=rect(x0=40, y0=30, x1=70, y1=70))\n";
        let repaired = "from wge.world import lane, place\nfrom wge.geometry import rect\n\ncentral_lane = lane(id=\"central\", footprint=rect(x0=0, y0=40, x1=100, y1=60))\nblocked_keep = place(id=\"keep\", footprint=rect(x0=40, y0=0, x1=70, y1=20))\n";
        let tampered = "from wge.world import lane, place\nfrom wge.geometry import rect\n# unrelated note\ncentral_lane = lane(id=\"central\", footprint=rect(x0=0, y0=40, x1=100, y1=60))\nblocked_keep = place(id=\"keep\", footprint=rect(x0=40, y0=0, x1=70, y1=20))\n";
        assert!(authorize_edit(base, repaired, "blocked_keep", "place").is_ok());
        let error = authorize_edit(base, tampered, "blocked_keep", "place").unwrap_err();
        assert_eq!(error.code(), "unauthorized_span");
    }

    #[test]
    fn failed_candidate_leaves_the_pointer_on_g0() {
        let store =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/wge-kernel-unit-store");
        let _ = fs::remove_dir_all(&store);
        store::create_store(&store, "sha256:abc").unwrap();
        let solved = Solved {
            candidate_id: "C-test".into(),
            proposal_id: "P-test".into(),
            measurement: Measurement {
                intersects: true,
                overlap: Rect {
                    x0: 1,
                    x1: 2,
                    y0: 3,
                    y1: 4,
                },
                overlap_area: 1,
                raw: b"{}".to_vec(),
                sha256: "sha256:unit".into(),
            },
            ir: SpecimenIr {
                module_id: "m".into(),
                registry_digest: "sha256:abc".into(),
                lane: Node {
                    binding: "central_lane".into(),
                    constructor: "lane".into(),
                    world_id: "central".into(),
                    footprint: Rect {
                        x0: 0,
                        x1: 1,
                        y0: 0,
                        y1: 1,
                    },
                    span: Span {
                        line: 1,
                        column: 0,
                        end_line: 1,
                        end_column: 1,
                    },
                },
                place: Node {
                    binding: "blocked_keep".into(),
                    constructor: "place".into(),
                    world_id: "keep".into(),
                    footprint: Rect {
                        x0: 0,
                        x1: 1,
                        y0: 0,
                        y1: 1,
                    },
                    span: Span {
                        line: 2,
                        column: 0,
                        end_line: 2,
                        end_column: 1,
                    },
                },
                path: None,
                digest: "sha256:ir".into(),
            },
            authoring_digest: "sha256:src".into(),
        };
        reject_candidate(&store, &solved, "G0", "sha256:solver").unwrap();
        assert_eq!(current_generation(&store).unwrap(), "G0");
        assert!(store.join("generations").join("G0.json").is_file());
        assert!(!store.join("generations").join("G1.json").exists());
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn reopen_does_not_reset_and_corruption_fails_closed() {
        let store = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/wge-store-open");
        let _ = fs::remove_dir_all(&store);
        store::create_store(&store, "sha256:abc").unwrap();
        let genesis = fs::read(store.join("generations").join("G0.json")).unwrap();
        let opened = store::open_store(&store).unwrap();
        assert_eq!(opened.generation_id, "G0");
        assert!(store::create_store(&store, "sha256:abc").is_err());
        assert_eq!(
            fs::read(store.join("generations").join("G0.json")).unwrap(),
            genesis
        );

        let blob = store.join("evidence").join("unit.bin");
        store::put_immutable(&blob, b"same").unwrap();
        store::put_immutable(&blob, b"same").unwrap();
        assert!(store::put_immutable(&blob, b"other").is_err());
        assert_eq!(fs::read(&blob).unwrap(), b"same");

        fs::write(store.join("store.json"), "{\"schema\":\"wge.store/v9\"}").unwrap();
        assert_eq!(
            store::open_store(&store).unwrap_err().code(),
            "unsupported_schema"
        );
        assert_eq!(
            fs::read(store.join("generations").join("G0.json")).unwrap(),
            genesis
        );
        fs::write(
            store.join("pointer.json"),
            "{\"generation_id\":\"G-missing\",\"schema\":\"wge.pointer/v0\"}",
        )
        .unwrap();
        fs::write(
            store.join("store.json"),
            "{\"registry_digest\":\"sha256:abc\",\"schema\":\"wge.store/v0\"}",
        )
        .unwrap();
        assert_eq!(
            store::open_store(&store).unwrap_err().code(),
            "missing_generation"
        );
        assert!(!store.join("generations").join("G-missing.json").exists());
        let _ = fs::remove_dir_all(&store);
    }

    #[test]
    fn solver_image_changes_when_julia_version_changes() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("registry_v0.json");
        let first = solver_image(&path, &path, &path, "julia version 1.0.0").unwrap();
        let second = solver_image(&path, &path, &path, "julia version 1.2.0").unwrap();
        assert_ne!(first, second);
        assert!(!first.contains("pid"));
    }
}
