use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use luxel_intake_repair_contract::{
    IntakeDraft, RepairEvidenceDelta, RepairProposal, RepairProposalDraft, SemanticIntake,
    SourceBundle, SourceBundleDraft, apply_repair, normalize_intake, normalize_repair_proposal,
    parse_json, prepare_source_bundle, to_pretty_json, validate_delta_identity, validate_intake,
    validate_repair_proposal, validate_source_bundle,
};

fn read(path: &str) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|error| format!("{}: {error}", Path::new(path).display()))
}

fn read_bindings(arguments: &[String]) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let mut bindings = BTreeMap::new();
    for argument in arguments {
        let (key, path) = argument
            .split_once('=')
            .ok_or_else(|| format!("source binding must be REF=PATH: {argument}"))?;
        if key.is_empty() || path.is_empty() {
            return Err(format!("source binding must be REF=PATH: {argument}"));
        }
        if bindings.insert(key.to_owned(), read(path)?).is_some() {
            return Err(format!("duplicate source binding {key}"));
        }
    }
    Ok(bindings)
}

fn write_json<T: serde::Serialize>(path: &str, value: &T) -> Result<(), String> {
    let bytes = to_pretty_json(value).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| format!("{}: {error}", Path::new(path).display()))
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(usage());
    };
    match command {
        "prepare-source-bundle" if arguments.len() >= 3 => {
            let draft: SourceBundleDraft =
                parse_json(&read(&arguments[1])?).map_err(|e| e.to_string())?;
            let source_bytes = read_bindings(&arguments[3..])?;
            let bundle = prepare_source_bundle(draft, &source_bytes).map_err(|e| e.to_string())?;
            write_json(&arguments[2], &bundle)?;
            println!("{}", bundle.source_bundle_id);
            Ok(())
        }
        "validate-source-bundle" if arguments.len() >= 2 => {
            let bundle: SourceBundle =
                parse_json(&read(&arguments[1])?).map_err(|e| e.to_string())?;
            let source_bytes = read_bindings(&arguments[2..])?;
            validate_source_bundle(&bundle, &source_bytes).map_err(|e| e.to_string())?;
            println!("valid {}", bundle.source_bundle_id);
            Ok(())
        }
        "normalize-intake" if arguments.len() >= 6 => {
            let draft: IntakeDraft =
                parse_json(&read(&arguments[1])?).map_err(|e| e.to_string())?;
            let bundle: SourceBundle =
                parse_json(&read(&arguments[2])?).map_err(|e| e.to_string())?;
            let provider_response = read(&arguments[3])?;
            let source_bytes = read_bindings(&arguments[5..])?;
            let intake: SemanticIntake =
                normalize_intake(draft, &bundle, &provider_response, &source_bytes)
                    .map_err(|e| e.to_string())?;
            write_json(&arguments[4], &intake)?;
            println!("{}", intake.intake_id);
            Ok(())
        }
        "validate-intake" if arguments.len() >= 3 => {
            let intake: SemanticIntake =
                parse_json(&read(&arguments[1])?).map_err(|e| e.to_string())?;
            let provider_response = read(&arguments[2])?;
            let source_bytes = read_bindings(&arguments[3..])?;
            validate_intake(&intake, &provider_response, &source_bytes)
                .map_err(|e| e.to_string())?;
            println!("valid {}", intake.intake_id);
            Ok(())
        }
        "normalize-repair-proposal" if arguments.len() == 3 => {
            let draft: RepairProposalDraft =
                parse_json(&read(&arguments[1])?).map_err(|e| e.to_string())?;
            let proposal: RepairProposal =
                normalize_repair_proposal(draft).map_err(|e| e.to_string())?;
            write_json(&arguments[2], &proposal)?;
            println!("{}", proposal.proposal_id);
            Ok(())
        }
        "validate-repair-proposal" if arguments.len() == 2 => {
            let proposal: RepairProposal =
                parse_json(&read(&arguments[1])?).map_err(|e| e.to_string())?;
            validate_repair_proposal(&proposal).map_err(|e| e.to_string())?;
            println!("proposal-valid {}", proposal.proposal_id);
            Ok(())
        }
        "apply-repair" if arguments.len() == 8 => {
            let before = read(&arguments[1])?;
            let proposal: RepairProposal =
                parse_json(&read(&arguments[2])?).map_err(|e| e.to_string())?;
            let proposed = read(&arguments[3])?;
            if arguments[4] != "--output" || arguments[6] != "--receipt" {
                return Err(usage());
            }
            let (applied, application) =
                apply_repair(&proposal, &before, &proposed).map_err(|e| e.to_string())?;
            fs::write(&arguments[5], applied)
                .map_err(|error| format!("{}: {error}", Path::new(&arguments[5]).display()))?;
            write_json(&arguments[7], &application)?;
            println!("{}", application.after_sha256);
            Ok(())
        }
        "validate-delta-identity" if arguments.len() == 2 => {
            let delta: RepairEvidenceDelta =
                parse_json(&read(&arguments[1])?).map_err(|e| e.to_string())?;
            validate_delta_identity(&delta).map_err(|e| e.to_string())?;
            println!("delta-identity-valid {}", delta.delta_id);
            Ok(())
        }
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage:\n  luxel-intake-repair prepare-source-bundle DRAFT.json OUTPUT.json source_ref=PATH...\n  luxel-intake-repair validate-source-bundle BUNDLE.json source_id=PATH...\n  luxel-intake-repair normalize-intake DRAFT.json SOURCE_BUNDLE.json PROVIDER_RESPONSE OUTPUT.json source_id=PATH...\n  luxel-intake-repair validate-intake INTAKE.json PROVIDER_RESPONSE source_id=PATH...\n  luxel-intake-repair normalize-repair-proposal DRAFT.json OUTPUT.json\n  luxel-intake-repair validate-repair-proposal PROPOSAL.json\n  luxel-intake-repair apply-repair BEFORE_LAYOUT.json PROPOSAL.json PROPOSED_LAYOUT.json --output APPLIED_LAYOUT.json --receipt APPLICATION.json\n  luxel-intake-repair validate-delta-identity DELTA.json\n\nFull repair evidence verification is exposed through the Rust API and requires the integrating Rust native-validator registry."
        .into()
}

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
