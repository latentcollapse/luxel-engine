//! Typed owner for the closed semantic IR v0 wire representation.
//!
//! The Python frontend may preserve the model-facing authoring syntax and
//! produce a draft document. This module validates and normalizes that
//! document before any Rust consumer accepts it.

use serde_json::{Map, Value, json};

use crate::{IR_SCHEMA, KernelFailure};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Declaration {
    Node {
        binding: String,
        constructor: NodeConstructor,
        footprint: Rect,
        id: String,
        span: Span,
    },
    Path {
        binding: String,
        budget: i64,
        id: String,
        span: Span,
        x0: i64,
        x1: i64,
        y0: i64,
        y1: i64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum NodeConstructor {
    Lane,
    Place,
}

impl NodeConstructor {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Lane => "lane",
            Self::Place => "place",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Rect {
    x0: i64,
    x1: i64,
    y0: i64,
    y1: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Span {
    column: usize,
    end_column: usize,
    end_line: usize,
    line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SemanticIrV0 {
    module_id: String,
    registry_digest: String,
    declarations: Vec<Declaration>,
}

impl SemanticIrV0 {
    fn parse(value: &Value) -> Result<Self, KernelFailure> {
        let root = object(value, "ir")?;
        reject_unknown(
            root,
            &["declarations", "module_id", "registry_digest", "schema"],
            "ir",
        )?;
        let schema = string(require(root, "schema", "ir")?, "ir.schema")?;
        if schema != IR_SCHEMA {
            return Err(KernelFailure::operator(
                "unknown_constructor",
                format!("schema {schema} is not {IR_SCHEMA}"),
            ));
        }

        let module_id = string(require(root, "module_id", "ir")?, "ir.module_id")?.to_owned();
        let registry_digest = string(
            require(root, "registry_digest", "ir")?,
            "ir.registry_digest",
        )?
        .to_owned();
        let declarations_value =
            require(root, "declarations", "ir")?
                .as_array()
                .ok_or_else(|| {
                    KernelFailure::operator("malformed_ir", "ir.declarations must be an array")
                })?;
        if !(2..=3).contains(&declarations_value.len()) {
            return Err(KernelFailure::operator(
                "malformed_ir",
                "specimen requires one lane, one place, and at most one path",
            ));
        }

        let mut declarations = Vec::with_capacity(declarations_value.len());
        let mut lane_count = 0;
        let mut place_count = 0;
        let mut path_count = 0;
        for (index, declaration) in declarations_value.iter().enumerate() {
            let path = format!("ir.declarations[{index}]");
            let object = object(declaration, &path)?;
            let constructor = string(
                require(object, "constructor", &path)?,
                &format!("{path}.constructor"),
            )?;
            match constructor {
                "lane" | "place" => {
                    let allowed = ["binding", "constructor", "footprint", "id", "span"];
                    reject_unknown(object, &allowed, &path)?;
                    let constructor = if constructor == "lane" {
                        lane_count += 1;
                        NodeConstructor::Lane
                    } else {
                        place_count += 1;
                        NodeConstructor::Place
                    };
                    let footprint = parse_rect(
                        require(object, "footprint", &path)?,
                        &format!("{path}.footprint"),
                    )?;
                    declarations.push(Declaration::Node {
                        binding: string(
                            require(object, "binding", &path)?,
                            &format!("{path}.binding"),
                        )?
                        .to_owned(),
                        constructor,
                        footprint,
                        id: string(require(object, "id", &path)?, &format!("{path}.id"))?
                            .to_owned(),
                        span: parse_span(require(object, "span", &path)?, &format!("{path}.span"))?,
                    });
                }
                "path" => {
                    path_count += 1;
                    let allowed = [
                        "binding",
                        "budget",
                        "constructor",
                        "id",
                        "span",
                        "x0",
                        "x1",
                        "y0",
                        "y1",
                    ];
                    reject_unknown(object, &allowed, &path)?;
                    declarations.push(Declaration::Path {
                        binding: string(
                            require(object, "binding", &path)?,
                            &format!("{path}.binding"),
                        )?
                        .to_owned(),
                        budget: signed_integer(
                            require(object, "budget", &path)?,
                            &format!("{path}.budget"),
                        )?,
                        id: string(require(object, "id", &path)?, &format!("{path}.id"))?
                            .to_owned(),
                        span: parse_span(require(object, "span", &path)?, &format!("{path}.span"))?,
                        x0: signed_integer(require(object, "x0", &path)?, &format!("{path}.x0"))?,
                        x1: signed_integer(require(object, "x1", &path)?, &format!("{path}.x1"))?,
                        y0: signed_integer(require(object, "y0", &path)?, &format!("{path}.y0"))?,
                        y1: signed_integer(require(object, "y1", &path)?, &format!("{path}.y1"))?,
                    });
                }
                other => {
                    return Err(KernelFailure::operator(
                        "unknown_constructor",
                        format!("{other} is outside the specimen vocabulary"),
                    ));
                }
            }
        }
        if lane_count != 1 || place_count != 1 || path_count > 1 {
            return Err(KernelFailure::operator(
                "malformed_ir",
                "specimen requires exactly one lane, one place, and at most one path",
            ));
        }

        Ok(Self {
            module_id,
            registry_digest,
            declarations,
        })
    }

    fn to_value(&self) -> Value {
        let declarations = self
            .declarations
            .iter()
            .map(|declaration| match declaration {
                Declaration::Node {
                    binding,
                    constructor,
                    footprint,
                    id,
                    span,
                } => json!({
                    "binding": binding,
                    "constructor": constructor.as_str(),
                    "footprint": {
                        "x0": footprint.x0,
                        "x1": footprint.x1,
                        "y0": footprint.y0,
                        "y1": footprint.y1,
                    },
                    "id": id,
                    "span": {
                        "column": span.column,
                        "end_column": span.end_column,
                        "end_line": span.end_line,
                        "line": span.line,
                    },
                }),
                Declaration::Path {
                    binding,
                    budget,
                    id,
                    span,
                    x0,
                    x1,
                    y0,
                    y1,
                } => json!({
                    "binding": binding,
                    "budget": budget,
                    "constructor": "path",
                    "id": id,
                    "span": {
                        "column": span.column,
                        "end_column": span.end_column,
                        "end_line": span.end_line,
                        "line": span.line,
                    },
                    "x0": x0,
                    "x1": x1,
                    "y0": y0,
                    "y1": y1,
                }),
            })
            .collect::<Vec<_>>();
        json!({
            "declarations": declarations,
            "module_id": self.module_id,
            "registry_digest": self.registry_digest,
            "schema": IR_SCHEMA,
        })
    }
}

/// Validate a semantic IR v0 document and return its normalized wire value.
///
/// Declaration array order and source spans are preserved. Object key order
/// is imposed by canonical_json at the serialization boundary. Geometry
/// validity and source-span correspondence remain execution-time checks in
/// load_ir, matching the existing frontend contract.
pub fn normalize_semantic_ir(value: &Value) -> Result<Value, KernelFailure> {
    Ok(SemanticIrV0::parse(value)?.to_value())
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

fn string<'a>(value: &'a Value, path: &str) -> Result<&'a str, KernelFailure> {
    value
        .as_str()
        .ok_or_else(|| KernelFailure::operator("malformed_ir", format!("{path} must be a string")))
}

fn signed_integer(value: &Value, path: &str) -> Result<i64, KernelFailure> {
    if let Some(number) = value.as_i64() {
        return Ok(number);
    }
    if value.as_u64().is_some() {
        return Err(KernelFailure::operator(
            "unsupported_integer_range",
            format!("{path} is outside the signed 64-bit semantic IR range"),
        ));
    }
    Err(KernelFailure::operator(
        "malformed_ir",
        format!("{path} must be an integer"),
    ))
}

fn span_integer(value: &Value, path: &str) -> Result<usize, KernelFailure> {
    let number = value.as_u64().ok_or_else(|| {
        KernelFailure::operator(
            "malformed_ir",
            format!("{path} must be a non-negative integer"),
        )
    })?;
    usize::try_from(number).map_err(|_| {
        KernelFailure::operator(
            "unsupported_integer_range",
            format!("{path} is outside the platform span range"),
        )
    })
}

fn parse_rect(value: &Value, path: &str) -> Result<Rect, KernelFailure> {
    let object = object(value, path)?;
    reject_unknown(object, &["x0", "x1", "y0", "y1"], path)?;
    Ok(Rect {
        x0: signed_integer(require(object, "x0", path)?, &format!("{path}.x0"))?,
        x1: signed_integer(require(object, "x1", path)?, &format!("{path}.x1"))?,
        y0: signed_integer(require(object, "y0", path)?, &format!("{path}.y0"))?,
        y1: signed_integer(require(object, "y1", path)?, &format!("{path}.y1"))?,
    })
}

fn parse_span(value: &Value, path: &str) -> Result<Span, KernelFailure> {
    let object = object(value, path)?;
    reject_unknown(object, &["column", "end_column", "end_line", "line"], path)?;
    Ok(Span {
        column: span_integer(require(object, "column", path)?, &format!("{path}.column"))?,
        end_column: span_integer(
            require(object, "end_column", path)?,
            &format!("{path}.end_column"),
        )?,
        end_line: span_integer(
            require(object, "end_line", path)?,
            &format!("{path}.end_line"),
        )?,
        line: span_integer(require(object, "line", path)?, &format!("{path}.line"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical_json, sha256_prefixed};

    const GOLDEN: &str = include_str!("../../../../tests/fixtures/semantic_ir/v0_golden.json");

    #[test]
    fn shared_python_golden_vectors_have_stable_canonical_bytes() {
        let fixtures: Value = serde_json::from_str(GOLDEN).expect("golden fixture JSON");
        for vector in fixtures["vectors"].as_array().expect("vectors array") {
            let expected = &vector["expected"];
            let normalized = normalize_semantic_ir(expected).expect("valid golden vector");
            assert_eq!(&normalized, expected, "{}", vector["name"]);
            let canonical = canonical_json(&normalized);
            assert_eq!(
                sha256_prefixed(canonical.as_bytes()),
                vector["canonical_sha256"].as_str().expect("canonical hash"),
                "{}",
                vector["name"]
            );
        }
    }

    #[test]
    fn known_bad_ir_is_rejected_without_coercion() {
        let fixtures: Value = serde_json::from_str(GOLDEN).expect("golden fixture JSON");
        let base = &fixtures["vectors"][0]["expected"];

        let mut unknown = base.clone();
        unknown
            .as_object_mut()
            .expect("root object")
            .insert("injected".into(), json!(true));
        assert_eq!(
            normalize_semantic_ir(&unknown).unwrap_err().code(),
            "unknown_field"
        );

        let mut fractional = base.clone();
        fractional["declarations"][0]["footprint"]["x1"] = json!(1.5);
        assert_eq!(
            normalize_semantic_ir(&fractional).unwrap_err().code(),
            "malformed_ir"
        );

        let mut out_of_range = base.clone();
        out_of_range["declarations"][0]["footprint"]["x1"] = json!(i64::MAX as u64 + 1);
        assert_eq!(
            normalize_semantic_ir(&out_of_range).unwrap_err().code(),
            "unsupported_integer_range"
        );
    }

    #[test]
    fn normalization_preserves_legacy_degenerate_geometry_for_execution_validation() {
        let fixtures: Value = serde_json::from_str(GOLDEN).expect("golden fixture JSON");
        let mut degenerate = fixtures["vectors"][0]["expected"].clone();
        degenerate["declarations"][0]["footprint"]["x1"] = json!(0);
        let normalized = normalize_semantic_ir(&degenerate)
            .expect("normalization does not change frontend acceptance");
        assert_eq!(normalized["declarations"][0]["footprint"]["x1"], 0);
    }
}
