//! Native compiler from a source-bound semantic intake and an explicit,
//! typed project template into the canonical project-spec envelope.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use luxel_intake_repair_contract as intake;

use crate::{
    ArtifactNode, AssetBinding, BriefSpec, ClaimEvidence, ClaimKind, DesignConstraint,
    GameplayBinding, GateRequirement, IntakeProvenance, IntakeProviderProvenance, LedgerError,
    PROJECT_SPEC_SCHEMA, ProjectSpec, SemanticClaim, SourceKind, SourceProvenance, SourceReference,
    SpecAssumption, SpecConflict, StyleTarget, TargetProfile, WorkOrder, WorldSpec, canonical_json,
    sha256_prefixed, validate_relative_path, validate_spec,
};

pub const PROJECT_TEMPLATE_SCHEMA: &str = "luxel.project-template/v1";

/// The template supplies design and build decisions that source interpretation
/// cannot choose. Source paths are keyed by the intake's stable source IDs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectTemplate {
    pub schema_version: String,
    pub expected_intake_id: String,
    pub project_id: String,
    pub title: String,
    pub brief_source_id: String,
    /// Exact UTF-8 bytes of the designated brief source.
    pub brief_text: String,
    pub source_paths: BTreeMap<String, String>,
    pub style_target: StyleTarget,
    pub design_constraints: Vec<DesignConstraint>,
    pub target: TargetProfile,
    pub world: WorldSpec,
    pub assets: Vec<AssetBinding>,
    pub gameplay: GameplayBinding,
    pub artifact_graph: Vec<ArtifactNode>,
    pub work_orders: Vec<WorkOrder>,
    pub required_gates: Vec<GateRequirement>,
}

/// Compile without inventing a design choice. Every intake source, claim,
/// evidence region, conflict, assumption and provider identity is represented
/// in the result or compilation fails.
pub fn compile_project_spec(
    source_intake: &intake::SemanticIntake,
    template: ProjectTemplate,
) -> Result<ProjectSpec, LedgerError> {
    validate_intake_identity(source_intake)?;
    if template.schema_version != PROJECT_TEMPLATE_SCHEMA {
        return Err(LedgerError::Contract(format!(
            "unsupported project template schema {:?}",
            template.schema_version
        )));
    }
    if template.expected_intake_id != source_intake.intake_id {
        return Err(LedgerError::Provenance(format!(
            "template pins intake {}, but received {}",
            template.expected_intake_id, source_intake.intake_id
        )));
    }
    nonempty("template.project_id", &template.project_id)?;
    nonempty("template.title", &template.title)?;

    let source_ids: BTreeSet<&str> = source_intake
        .sources
        .iter()
        .map(|source| source.source_id.as_str())
        .collect();
    if source_ids.len() != source_intake.sources.len() {
        return Err(LedgerError::Provenance(
            "intake contains duplicate source IDs".into(),
        ));
    }
    let template_path_ids: BTreeSet<&str> =
        template.source_paths.keys().map(String::as_str).collect();
    if template_path_ids != source_ids {
        return Err(LedgerError::Contract(
            "template source_paths must map every intake source exactly once".into(),
        ));
    }
    for (source_id, path) in &template.source_paths {
        nonempty("template.source_paths.path", path)?;
        validate_relative_path(path, &format!("source path for {source_id}"))?;
    }
    let brief_source = source_intake
        .sources
        .iter()
        .find(|source| source.source_id == template.brief_source_id)
        .ok_or_else(|| LedgerError::Contract("brief_source_id is not in the intake".into()))?;
    if brief_source.kind != intake::SourceKind::Brief {
        return Err(LedgerError::Contract(
            "brief_source_id must identify a brief source".into(),
        ));
    }
    if sha256_prefixed(template.brief_text.as_bytes()) != brief_source.content_sha256 {
        return Err(LedgerError::Provenance(
            "template brief_text is stale or does not match the pinned brief source".into(),
        ));
    }

    let sources = source_intake
        .sources
        .iter()
        .map(|source| {
            Ok(SourceReference {
                source_id: source.source_id.clone(),
                kind: source_kind(source.kind),
                path: template.source_paths[&source.source_id].clone(),
                sha256: source.content_sha256.clone(),
                region_normalized: None,
                byte_length: Some(source.byte_length),
                media_type: Some(source.media_type.clone()),
                provenance: Some(SourceProvenance {
                    origin: source.provenance.origin,
                    origin_ref: source.provenance.origin_ref.clone(),
                    provider_id: source.provenance.provider_id.clone(),
                    provider_version: source.provenance.provider_version.clone(),
                }),
            })
        })
        .collect::<Result<Vec<_>, LedgerError>>()?;

    let claims = source_intake
        .observations
        .iter()
        .map(|claim| (ClaimKind::Observed, claim))
        .chain(
            source_intake
                .inferences
                .iter()
                .map(|claim| (ClaimKind::Inferred, claim)),
        )
        .map(|(kind, claim)| {
            let first = claim.evidence.first().ok_or_else(|| {
                LedgerError::Provenance(format!(
                    "intake claim {} has no evidence to represent",
                    claim.claim_id
                ))
            })?;
            Ok(SemanticClaim {
                claim_id: claim.claim_id.clone(),
                source_id: first.source_id.clone(),
                kind,
                statement: claim.statement.clone(),
                confidence: claim.confidence,
                domain: Some(claim.domain),
                evidence: claim
                    .evidence
                    .iter()
                    .map(|evidence| ClaimEvidence {
                        source_id: evidence.source_id.clone(),
                        region: evidence.region.clone(),
                    })
                    .collect(),
            })
        })
        .collect::<Result<Vec<_>, LedgerError>>()?;
    let conflicts = source_intake
        .conflicts
        .iter()
        .map(|conflict| SpecConflict {
            conflict_id: conflict.conflict_id.clone(),
            left_claim_id: conflict.left_claim_id.clone(),
            right_claim_id: conflict.right_claim_id.clone(),
            explanation: conflict.explanation.clone(),
            resolution: None,
        })
        .collect();
    let assumptions = source_intake
        .assumptions
        .iter()
        .map(|assumption| SpecAssumption {
            assumption_id: assumption.assumption_id.clone(),
            statement: assumption.statement.clone(),
            reason: assumption.rationale.clone(),
            confidence: Some(assumption.confidence),
            related_claim_ids: assumption.related_claim_ids.clone(),
        })
        .collect();

    let spec = ProjectSpec {
        schema_version: PROJECT_SPEC_SCHEMA.into(),
        project_id: template.project_id,
        title: template.title,
        brief: BriefSpec {
            text: template.brief_text,
            sources,
            claims,
            conflicts,
            style_target: template.style_target,
            assumptions,
            design_constraints: template.design_constraints,
            intake_provenance: Some(IntakeProvenance {
                intake_id: source_intake.intake_id.clone(),
                request_id: source_intake.request_id.clone(),
                source_bundle_id: source_intake.source_bundle_id.clone(),
                provider: IntakeProviderProvenance {
                    provider_id: source_intake.provider.provider_id.clone(),
                    provider_version: source_intake.provider.provider_version.clone(),
                    protocol: source_intake.provider.protocol.clone(),
                    response_sha256: source_intake.provider.response_sha256.clone(),
                },
            }),
        },
        target: template.target,
        world: template.world,
        assets: template.assets,
        gameplay: template.gameplay,
        artifact_graph: template.artifact_graph,
        work_orders: template.work_orders,
        required_gates: template.required_gates,
    };
    validate_spec(&spec)?;
    Ok(spec)
}

fn validate_intake_identity(intake: &intake::SemanticIntake) -> Result<(), LedgerError> {
    if intake.schema_version != intake::INTAKE_SCHEMA {
        return Err(LedgerError::Contract(format!(
            "expected {}, found {}",
            intake::INTAKE_SCHEMA,
            intake.schema_version
        )));
    }
    for (label, value) in [
        ("intake_id", intake.intake_id.as_str()),
        ("request_id", intake.request_id.as_str()),
        ("source_bundle_id", intake.source_bundle_id.as_str()),
        ("provider_id", intake.provider.provider_id.as_str()),
        (
            "provider_version",
            intake.provider.provider_version.as_str(),
        ),
        ("provider.protocol", intake.provider.protocol.as_str()),
    ] {
        nonempty(label, value)?;
    }
    if intake.provider.request_source_bundle_id != intake.source_bundle_id {
        return Err(LedgerError::Provenance(
            "provider request is bound to a different source bundle".into(),
        ));
    }
    validate_intake_digest(&intake.provider.response_sha256, "intake provider response")?;

    let mut source_ids = BTreeSet::new();
    for source in &intake.sources {
        nonempty("intake source_id", &source.source_id)?;
        nonempty("intake source media_type", &source.media_type)?;
        nonempty(
            "intake source provenance origin_ref",
            &source.provenance.origin_ref,
        )?;
        validate_intake_digest(&source.content_sha256, "intake source")?;
        validate_source_provenance(&source.provenance)?;
        validate_media_type(&source.media_type)?;
        if source_identity(source)? != source.source_id
            || !source_ids.insert(source.source_id.as_str())
        {
            return Err(LedgerError::Provenance(format!(
                "forged or duplicate intake source {}",
                source.source_id
            )));
        }
    }
    if intake.sources.is_empty() {
        return Err(LedgerError::Contract("intake has no sources".into()));
    }
    if source_bundle_identity(intake)? != intake.source_bundle_id {
        return Err(LedgerError::Provenance(
            "forged or stale source_bundle_id".into(),
        ));
    }

    let mut claims = BTreeMap::<&str, ()>::new();
    for (epistemic, items) in [
        (intake::EpistemicKind::Observation, &intake.observations),
        (intake::EpistemicKind::Inference, &intake.inferences),
    ] {
        for claim in items {
            nonempty("intake claim_id", &claim.claim_id)?;
            nonempty("intake claim statement", &claim.statement)?;
            if !claim.confidence.is_finite() || !(0.0..=1.0).contains(&claim.confidence) {
                return Err(LedgerError::Contract(format!(
                    "claim {} confidence is outside [0,1]",
                    claim.claim_id
                )));
            }
            if claim.evidence.is_empty() {
                return Err(LedgerError::Provenance(format!(
                    "claim {} has no source evidence",
                    claim.claim_id
                )));
            }
            for evidence in &claim.evidence {
                if !source_ids.contains(evidence.source_id.as_str()) {
                    return Err(LedgerError::Provenance(format!(
                        "claim {} refers to missing source {}",
                        claim.claim_id, evidence.source_id
                    )));
                }
                let source = intake
                    .sources
                    .iter()
                    .find(|source| source.source_id == evidence.source_id)
                    .expect("source ID was checked above");
                if let Some(region) = &evidence.region {
                    validate_region(region, source.kind, source.byte_length)?;
                }
            }
            let expected = claim_identity(epistemic, claim)?;
            if expected != claim.claim_id || claims.insert(claim.claim_id.as_str(), ()).is_some() {
                return Err(LedgerError::Provenance(format!(
                    "forged or duplicate intake claim {}",
                    claim.claim_id
                )));
            }
        }
    }
    let mut conflict_ids = BTreeSet::new();
    for conflict in &intake.conflicts {
        if !claims.contains_key(conflict.left_claim_id.as_str())
            || !claims.contains_key(conflict.right_claim_id.as_str())
            || conflict.left_claim_id == conflict.right_claim_id
            || conflict_identity(conflict)? != conflict.conflict_id
            || !conflict_ids.insert(conflict.conflict_id.as_str())
        {
            return Err(LedgerError::Provenance(format!(
                "malformed, forged, or duplicate conflict {}",
                conflict.conflict_id
            )));
        }
    }
    let mut assumption_ids = BTreeSet::new();
    for assumption in &intake.assumptions {
        if !assumption.confidence.is_finite() || !(0.0..=1.0).contains(&assumption.confidence) {
            return Err(LedgerError::Contract(format!(
                "assumption {} confidence is outside [0,1]",
                assumption.assumption_id
            )));
        }
        if assumption
            .related_claim_ids
            .iter()
            .any(|id| !claims.contains_key(id.as_str()))
            || assumption_identity(assumption)? != assumption.assumption_id
            || !assumption_ids.insert(assumption.assumption_id.as_str())
        {
            return Err(LedgerError::Provenance(format!(
                "malformed, forged, or duplicate assumption {}",
                assumption.assumption_id
            )));
        }
    }
    if intake_identity(intake)? != intake.intake_id {
        return Err(LedgerError::Provenance("forged or stale intake_id".into()));
    }
    Ok(())
}

fn source_identity(source: &intake::SourceRecord) -> Result<String, LedgerError> {
    let body = json!({
        "kind": source.kind,
        "content_sha256": source.content_sha256,
        "byte_length": source.byte_length,
        "media_type": source.media_type,
        "provenance": source.provenance,
    });
    Ok(identity("source", &canonical_digest(&body)?))
}

fn source_bundle_identity(value: &intake::SemanticIntake) -> Result<String, LedgerError> {
    let mut sources = value.sources.clone();
    sources.sort_by(|left, right| left.source_id.cmp(&right.source_id));
    let body = json!({
        "schema_version": intake::SOURCE_BUNDLE_SCHEMA,
        "request_id": value.request_id,
        "sources": sources,
    });
    Ok(identity("source-bundle", &canonical_digest(&body)?))
}

fn claim_identity(
    epistemic: intake::EpistemicKind,
    claim: &intake::SemanticClaim,
) -> Result<String, LedgerError> {
    let body = json!({
        "epistemic_kind": epistemic,
        "domain": claim.domain,
        "statement": claim.statement,
        "confidence": claim.confidence,
        "evidence": claim.evidence,
    });
    let raw = identity("claim", &canonical_digest(&body)?);
    Ok(format!(
        "{}:{raw}",
        match epistemic {
            intake::EpistemicKind::Observation => "observation",
            intake::EpistemicKind::Inference => "inference",
        }
    ))
}

fn conflict_identity(value: &intake::SemanticConflict) -> Result<String, LedgerError> {
    Ok(identity(
        "conflict",
        &canonical_digest(&json!({
            "left_claim_id": value.left_claim_id,
            "right_claim_id": value.right_claim_id,
            "explanation": value.explanation,
        }))?,
    ))
}

fn assumption_identity(value: &intake::SemanticAssumption) -> Result<String, LedgerError> {
    Ok(identity(
        "assumption",
        &canonical_digest(&json!({
            "statement": value.statement,
            "rationale": value.rationale,
            "confidence": value.confidence,
            "related_claim_ids": value.related_claim_ids,
        }))?,
    ))
}

fn intake_identity(value: &intake::SemanticIntake) -> Result<String, LedgerError> {
    let mut sources = value.sources.clone();
    let mut observations = value.observations.clone();
    let mut inferences = value.inferences.clone();
    let mut conflicts = value.conflicts.clone();
    let mut assumptions = value.assumptions.clone();
    sources.sort_by(|a, b| a.source_id.cmp(&b.source_id));
    observations.sort_by(|a, b| a.claim_id.cmp(&b.claim_id));
    inferences.sort_by(|a, b| a.claim_id.cmp(&b.claim_id));
    conflicts.sort_by(|a, b| a.conflict_id.cmp(&b.conflict_id));
    assumptions.sort_by(|a, b| a.assumption_id.cmp(&b.assumption_id));
    Ok(identity(
        "intake",
        &canonical_digest(&json!({
            "schema_version": value.schema_version,
            "request_id": value.request_id,
            "source_bundle_id": value.source_bundle_id,
            "provider": value.provider,
            "sources": sources,
            "observations": observations,
            "inferences": inferences,
            "conflicts": conflicts,
            "assumptions": assumptions,
        }))?,
    ))
}

fn canonical_digest(value: &Value) -> Result<String, LedgerError> {
    Ok(sha256_prefixed(canonical_json(value).as_bytes()))
}

fn validate_intake_digest(value: &str, label: &str) -> Result<(), LedgerError> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(LedgerError::Provenance(format!(
            "{label} digest must use sha256:<64 lowercase hex>"
        )));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(LedgerError::Provenance(format!(
            "{label} digest is malformed"
        )));
    }
    Ok(())
}

fn validate_source_provenance(provenance: &intake::SourceProvenance) -> Result<(), LedgerError> {
    nonempty("source.provenance.origin_ref", &provenance.origin_ref)?;
    match (
        provenance.origin,
        &provenance.provider_id,
        &provenance.provider_version,
    ) {
        (intake::ProvenanceOrigin::UserSupplied, None, None) => Ok(()),
        (intake::ProvenanceOrigin::UserSupplied, _, _) => Err(LedgerError::Provenance(
            "user_supplied source cannot claim provider identity".into(),
        )),
        (_, Some(provider_id), Some(provider_version)) => {
            nonempty("source.provenance.provider_id", provider_id)?;
            nonempty("source.provenance.provider_version", provider_version)
        }
        _ => Err(LedgerError::Provenance(
            "non-user source requires provider id and version".into(),
        )),
    }
}

fn validate_media_type(value: &str) -> Result<(), LedgerError> {
    let mut parts = value.split('/');
    let (Some(top), Some(sub), None) = (parts.next(), parts.next(), parts.next()) else {
        return Err(LedgerError::Contract(
            "source media_type is malformed".into(),
        ));
    };
    if top.is_empty()
        || sub.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&^_.+-/".contains(&byte))
    {
        return Err(LedgerError::Contract(
            "source media_type is malformed".into(),
        ));
    }
    Ok(())
}

fn validate_region(
    region: &intake::SourceRegion,
    source_kind: intake::SourceKind,
    byte_length: u64,
) -> Result<(), LedgerError> {
    let valid_rect = |x_min: f64, y_min: f64, x_max: f64, y_max: f64| {
        [x_min, y_min, x_max, y_max]
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
            && x_min < x_max
            && y_min < y_max
    };
    let valid = match region {
        intake::SourceRegion::TextSpan {
            start_byte,
            end_byte,
        } => {
            start_byte < end_byte
                && *end_byte <= byte_length
                && matches!(
                    source_kind,
                    intake::SourceKind::Brief | intake::SourceKind::DesignDocument
                )
        }
        intake::SourceRegion::ImageRect {
            x_min,
            y_min,
            x_max,
            y_max,
        } => {
            source_kind == intake::SourceKind::ConceptArt
                && valid_rect(*x_min, *y_min, *x_max, *y_max)
        }
        intake::SourceRegion::PageRect {
            page,
            x_min,
            y_min,
            x_max,
            y_max,
        } => {
            *page > 0
                && source_kind == intake::SourceKind::DesignDocument
                && valid_rect(*x_min, *y_min, *x_max, *y_max)
        }
    };
    if valid {
        Ok(())
    } else {
        Err(LedgerError::Provenance(
            "intake evidence region is invalid for its source".into(),
        ))
    }
}

fn identity(prefix: &str, digest: &str) -> String {
    format!("{prefix}:{digest}")
}

fn source_kind(value: intake::SourceKind) -> SourceKind {
    match value {
        intake::SourceKind::Brief => SourceKind::Brief,
        intake::SourceKind::ConceptArt => SourceKind::ConceptArt,
        intake::SourceKind::DesignDocument => SourceKind::DesignDocument,
    }
}

fn nonempty(label: &str, value: &str) -> Result<(), LedgerError> {
    if value.trim().is_empty() {
        return Err(LedgerError::Contract(format!("{label} is required")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::PathBuf};

    use luxel_intake_repair_contract::{
        AssumptionDraft, ClaimDraft, ConflictDraft, EpistemicKind, EvidenceLinkDraft, IntakeDraft,
        ProviderInterpretation, ProviderProvenance, SemanticIntake, SourceBundleDraft, SourceDraft,
        SourceProvenance as IntakeSourceProvenance, normalize_intake, parse_json,
        prepare_source_bundle, sha256_prefixed,
    };

    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../intake_repair_contract/fixtures/intake")
                .join(name),
        )
        .unwrap()
    }

    fn normalized_intake(with_conflict: bool) -> SemanticIntake {
        let source_draft: SourceBundleDraft =
            parse_json(&fixture("source-bundle-draft.json")).unwrap();
        let source_bytes = BTreeMap::from([
            ("brief".to_owned(), fixture("brief.txt")),
            ("concept".to_owned(), fixture("concept.svg")),
        ]);
        let bundle = prepare_source_bundle(source_draft, &source_bytes).unwrap();
        let bytes_by_id = bundle
            .sources
            .iter()
            .map(|source| {
                let bytes = if source.media_type == "text/plain" {
                    fixture("brief.txt")
                } else {
                    fixture("concept.svg")
                };
                (source.source_id.clone(), bytes)
            })
            .collect();
        let mut response: ProviderInterpretation =
            parse_json(&fixture("provider-response.json")).unwrap();
        if with_conflict {
            response.conflicts.push(ConflictDraft {
                conflict_ref: "marker-objective-disagreement".into(),
                left_claim_ref: "marker".into(),
                right_claim_ref: "objective".into(),
                explanation: "The visual marker and written objective may refer to different targets.".into(),
            });
        }
        let response_bytes = serde_json::to_vec(&response).unwrap();
        let mut draft: IntakeDraft = parse_json(&fixture("intake-draft.json")).unwrap();
        draft.provider.response_sha256 = sha256_prefixed(&response_bytes);
        draft.interpretation = response;
        normalize_intake(draft, &bundle, &response_bytes, &bytes_by_id).unwrap()
    }

    fn template(intake: &SemanticIntake) -> ProjectTemplate {
        template_for(intake, String::from_utf8(fixture("brief.txt")).unwrap())
    }

    fn template_for(intake: &SemanticIntake, brief_text: String) -> ProjectTemplate {
        let base = crate::tests::spec();
        let brief_source = intake
            .sources
            .iter()
            .find(|source| source.kind == intake::SourceKind::Brief)
            .unwrap();
        let source_paths = intake
            .sources
            .iter()
            .map(|source| {
                (
                    source.source_id.clone(),
                    format!("sources/{}.dat", source.source_id),
                )
            })
            .collect();
        let mut style_target = base.brief.style_target;
        style_target.reference_source_ids = vec![brief_source.source_id.clone()];
        ProjectTemplate {
            schema_version: PROJECT_TEMPLATE_SCHEMA.into(),
            expected_intake_id: intake.intake_id.clone(),
            project_id: base.project_id,
            title: base.title,
            brief_source_id: brief_source.source_id.clone(),
            brief_text,
            source_paths,
            style_target,
            design_constraints: base.brief.design_constraints,
            target: base.target,
            world: base.world,
            assets: base.assets,
            gameplay: base.gameplay,
            artifact_graph: base.artifact_graph,
            work_orders: base.work_orders,
            required_gates: base.required_gates,
        }
    }

    #[test]
    fn compiles_without_losing_source_regions_epistemics_or_assumptions() {
        let intake = normalized_intake(true);
        let spec = compile_project_spec(&intake, template(&intake)).unwrap();
        assert_eq!(
            spec.brief.claims.len(),
            intake.observations.len() + intake.inferences.len()
        );
        assert_eq!(
            spec.brief
                .claims
                .iter()
                .filter(|claim| claim.kind == ClaimKind::Observed)
                .count(),
            intake.observations.len()
        );
        assert_eq!(
            spec.brief
                .claims
                .iter()
                .filter(|claim| claim.kind == ClaimKind::Inferred)
                .count(),
            intake.inferences.len()
        );
        let inferred = spec
            .brief
            .claims
            .iter()
            .find(|claim| claim.kind == ClaimKind::Inferred)
            .unwrap();
        assert_eq!(inferred.evidence.len(), 2);
        assert!(
            inferred
                .evidence
                .iter()
                .all(|evidence| evidence.region.is_some())
        );
        assert_eq!(spec.brief.conflicts.len(), 1);
        assert_eq!(spec.brief.conflicts[0].resolution, None);
        assert_eq!(
            spec.brief.conflicts[0].explanation,
            intake.conflicts[0].explanation
        );
        assert_eq!(
            spec.brief.assumptions[0].confidence,
            Some(intake.assumptions[0].confidence)
        );
        assert_eq!(
            spec.brief.assumptions[0].related_claim_ids,
            intake.assumptions[0].related_claim_ids
        );
        assert_eq!(
            spec.brief.intake_provenance.as_ref().unwrap().intake_id,
            intake.intake_id
        );
        assert!(
            spec.brief
                .sources
                .iter()
                .all(|source| source.byte_length.is_some() && source.provenance.is_some())
        );
    }

    #[test]
    fn rejects_stale_or_self_inconsistent_intake_and_template() {
        let intake = normalized_intake(false);
        let mut stale = intake.clone();
        stale.observations[0].statement.push_str(" changed");
        assert!(compile_project_spec(&stale, template(&intake)).is_err());

        let mut wrong_pin = template(&intake);
        wrong_pin.expected_intake_id.push_str("stale");
        assert!(compile_project_spec(&intake, wrong_pin).is_err());

        let mut incomplete = template(&intake);
        incomplete.source_paths.pop_first();
        assert!(compile_project_spec(&intake, incomplete).is_err());
    }

    #[test]
    fn rejects_unknown_template_fields_and_brief_bytes_that_do_not_match_source() {
        let intake = normalized_intake(false);
        let mut bad_brief = template(&intake);
        bad_brief.brief_text.push_str(" stale");
        assert!(compile_project_spec(&intake, bad_brief).is_err());

        let value = serde_json::to_value(template(&intake)).unwrap();
        let mut object = value.as_object().unwrap().clone();
        object.insert("silent_default".into(), Value::Bool(true));
        assert!(serde_json::from_value::<ProjectTemplate>(Value::Object(object)).is_err());
    }

    #[test]
    fn fresh_non_fixture_intake_compiles_and_semantic_changes_change_spec_digest() {
        fn compile_fresh(brief_text: &[u8]) -> ProjectSpec {
            let concept_bytes = b"fresh concept source, generated inside this test".to_vec();
            let raw_sources = BTreeMap::from([
                ("brief".to_owned(), brief_text.to_vec()),
                ("concept".to_owned(), concept_bytes.clone()),
            ]);
            let source_draft = SourceBundleDraft {
                schema_version: luxel_intake_repair_contract::SOURCE_BUNDLE_DRAFT_SCHEMA.into(),
                request_id: format!("fresh-luxel6-{}", brief_text.len()),
                sources: vec![
                    SourceDraft {
                        source_ref: "brief".into(),
                        kind: intake::SourceKind::Brief,
                        content_sha256: sha256_prefixed(brief_text),
                        media_type: "text/plain".into(),
                        provenance: IntakeSourceProvenance {
                            origin: intake::ProvenanceOrigin::UserSupplied,
                            origin_ref: "fresh-test:brief".into(),
                            provider_id: None,
                            provider_version: None,
                        },
                    },
                    SourceDraft {
                        source_ref: "concept".into(),
                        kind: intake::SourceKind::ConceptArt,
                        content_sha256: sha256_prefixed(&concept_bytes),
                        media_type: "image/svg+xml".into(),
                        provenance: IntakeSourceProvenance {
                            origin: intake::ProvenanceOrigin::UserSupplied,
                            origin_ref: "fresh-test:concept".into(),
                            provider_id: None,
                            provider_version: None,
                        },
                    },
                ],
            };
            let bundle = prepare_source_bundle(source_draft, &raw_sources).unwrap();
            let source_bytes_by_id = bundle
                .sources
                .iter()
                .map(|source| {
                    let bytes = if source.kind == intake::SourceKind::Brief {
                        brief_text.to_vec()
                    } else {
                        concept_bytes.clone()
                    };
                    (source.source_id.clone(), bytes)
                })
                .collect();
            let brief_source_id = bundle
                .sources
                .iter()
                .find(|source| source.kind == intake::SourceKind::Brief)
                .unwrap()
                .source_id
                .clone();
            let concept_source_id = bundle
                .sources
                .iter()
                .find(|source| source.kind == intake::SourceKind::ConceptArt)
                .unwrap()
                .source_id
                .clone();
            let interpretation = ProviderInterpretation {
                schema_version: luxel_intake_repair_contract::PROVIDER_INTERPRETATION_SCHEMA.into(),
                source_bundle_id: bundle.source_bundle_id.clone(),
                claims: vec![ClaimDraft {
                    claim_ref: "route".into(),
                    epistemic_kind: EpistemicKind::Observation,
                    domain: intake::ClaimDomain::Gameplay,
                    statement: "The brief requires a route to the objective.".into(),
                    confidence: 1.0,
                    evidence: vec![EvidenceLinkDraft {
                        source_id: brief_source_id,
                        region: Some(intake::SourceRegion::TextSpan {
                            start_byte: 0,
                            end_byte: brief_text.len() as u64,
                        }),
                    }],
                }],
                conflicts: Vec::new(),
                assumptions: vec![AssumptionDraft {
                    assumption_ref: "camera".into(),
                    statement: "Use a third-person camera.".into(),
                    rationale: "The brief omits a camera mode.".into(),
                    confidence: 0.61,
                    related_claim_refs: vec!["route".into()],
                }],
            };
            let response_bytes = serde_json::to_vec(&interpretation).unwrap();
            let draft = IntakeDraft {
                schema_version: luxel_intake_repair_contract::INTAKE_DRAFT_SCHEMA.into(),
                source_bundle_id: bundle.source_bundle_id.clone(),
                provider: ProviderProvenance {
                    provider_id: "fresh-test.interpreter".into(),
                    provider_version: "1".into(),
                    protocol: luxel_intake_repair_contract::PROVIDER_INTERPRETATION_SCHEMA.into(),
                    request_source_bundle_id: bundle.source_bundle_id.clone(),
                    response_sha256: sha256_prefixed(&response_bytes),
                },
                interpretation,
            };
            let semantic_intake =
                normalize_intake(draft, &bundle, &response_bytes, &source_bytes_by_id).unwrap();
            let mut template = template_for(
                &semantic_intake,
                String::from_utf8(brief_text.to_vec()).unwrap(),
            );
            template.style_target.reference_source_ids = vec![concept_source_id];
            compile_project_spec(&semantic_intake, template).unwrap()
        }

        let first = compile_fresh(b"A short fresh brief with an objective route.");
        let changed = compile_fresh(b"A changed fresh brief with an objective route.");
        assert_ne!(
            crate::spec_digest(&first).unwrap(),
            crate::spec_digest(&changed).unwrap()
        );

        let intake = normalized_intake(false);
        let mut revised_template = template(&intake);
        revised_template.title.push_str(" revised");
        let revised = compile_project_spec(&intake, revised_template).unwrap();
        assert_ne!(
            crate::spec_digest(&compile_project_spec(&intake, template(&intake)).unwrap()).unwrap(),
            crate::spec_digest(&revised).unwrap()
        );
    }
}
