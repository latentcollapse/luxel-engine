use std::{env, fs, process::ExitCode};

use codeweald_worldspec::{
    compile_render_plan_value, validate_json, validate_placement_plan_value,
    validate_render_plan_value, validate_terrain_analysis_value,
};

type RenderPlanInputs = (
    serde_json::Value,
    serde_json::Value,
    serde_json::Value,
    serde_json::Value,
    Vec<u8>,
    Vec<u8>,
    serde_json::Value,
);

fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let command = arguments.next().unwrap_or_default();
    if matches!(
        command.as_str(),
        "compile-render-plan" | "validate-render-plan"
    ) {
        let paths: Vec<String> = arguments.collect();
        if paths.len() != 8 {
            eprintln!(
                "usage: codeweald-worldspec {command} <zone-spec.json> <asset-plan.json> <asset-preflight.json> <terrain-manifest.json> <heightfield-f32le.bin> <canopy-suitability-u8.bin> <landform-placement-plan.json> <render-plan.json>"
            );
            return ExitCode::from(2);
        }
        return render_plan(&command, &paths);
    }
    if command == "validate-plan" {
        let Some(zone_path) = arguments.next() else {
            eprintln!(
                "usage: codeweald-worldspec validate-plan <zone-spec.json> <asset-plan.json> <placement-plan.json>"
            );
            return ExitCode::from(2);
        };
        let Some(asset_path) = arguments.next() else {
            eprintln!(
                "usage: codeweald-worldspec validate-plan <zone-spec.json> <asset-plan.json> <placement-plan.json>"
            );
            return ExitCode::from(2);
        };
        let Some(plan_path) = arguments.next() else {
            eprintln!(
                "usage: codeweald-worldspec validate-plan <zone-spec.json> <asset-plan.json> <placement-plan.json>"
            );
            return ExitCode::from(2);
        };
        if arguments.next().is_some() {
            eprintln!(
                "usage: codeweald-worldspec validate-plan <zone-spec.json> <asset-plan.json> <placement-plan.json>"
            );
            return ExitCode::from(2);
        }
        return validate_plan(&zone_path, &asset_path, &plan_path);
    }
    if command == "validate-terrain-analysis" {
        let paths: Vec<String> = arguments.collect();
        if paths.len() != 6 {
            eprintln!(
                "usage: codeweald-worldspec validate-terrain-analysis <zone-spec.json> <terrain-manifest.json> <heightfield-f32le.bin> <protected-relief-mask.bin> <semantic-region-mask.bin> <terrain-analysis.json>"
            );
            return ExitCode::from(2);
        }
        return validate_terrain_analysis(&paths);
    }
    let path = arguments.next().unwrap_or_default();
    if !matches!(command.as_str(), "validate" | "fingerprint")
        || path.is_empty()
        || arguments.next().is_some()
    {
        eprintln!("usage: codeweald-worldspec <validate|fingerprint> <zone-spec.json>");
        return ExitCode::from(2);
    }
    let input = match fs::read_to_string(&path) {
        Ok(input) => input,
        Err(error) => {
            eprintln!("cannot read {path}: {error}");
            return ExitCode::from(2);
        }
    };
    match validate_json(&input) {
        Ok(identity) if command == "fingerprint" => {
            println!("{}", identity.canonical_sha256);
            ExitCode::SUCCESS
        }
        Ok(identity) => {
            println!(
                "valid {}: {}x{} m, {} features, sha256:{}",
                identity.zone_id,
                identity.width_m,
                identity.length_m,
                identity.feature_count,
                identity.canonical_sha256
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn render_plan(command: &str, paths: &[String]) -> ExitCode {
    let read_json = |path: &str| -> Result<serde_json::Value, String> {
        let input =
            fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
        serde_json::from_str(&input).map_err(|error| format!("invalid JSON in {path}: {error}"))
    };
    let read_bytes = |path: &str| -> Result<Vec<u8>, String> {
        fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))
    };
    // Read in argument order through one fallible closure. The previous form
    // matched a tuple of Results with one arm per position, which needs a new
    // arm and a new wildcard row for every input added -- and a mis-typed
    // wildcard row binds the wrong error while still compiling. `?` cannot
    // mis-wire an argument.
    let load = || -> Result<RenderPlanInputs, String> {
        Ok((
            read_json(&paths[0])?,
            read_json(&paths[1])?,
            read_json(&paths[2])?,
            read_json(&paths[3])?,
            read_bytes(&paths[4])?,
            read_bytes(&paths[5])?,
            read_json(&paths[6])?,
        ))
    };
    let (zone, assets, preflight, manifest, heightfield, suitability, landforms) = match load() {
        Ok(values) => values,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let plan = if command == "compile-render-plan" {
        match compile_render_plan_value(
            &zone,
            &assets,
            &preflight,
            &manifest,
            &heightfield,
            &suitability,
            &landforms,
        ) {
            Ok(plan) => {
                let output = match serde_json::to_string_pretty(&plan) {
                    Ok(output) => output + "\n",
                    Err(error) => {
                        eprintln!("cannot encode render plan: {error}");
                        return ExitCode::from(2);
                    }
                };
                if let Err(error) = fs::write(&paths[7], output) {
                    eprintln!("cannot write {}: {error}", paths[7]);
                    return ExitCode::from(2);
                }
                plan
            }
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::from(1);
            }
        }
    } else {
        match read_json(&paths[7]) {
            Ok(plan) => plan,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::from(2);
            }
        }
    };
    match validate_render_plan_value(
        &zone,
        &assets,
        &preflight,
        &manifest,
        &heightfield,
        &suitability,
        &landforms,
        &plan,
    ) {
        Ok(count) => {
            println!("valid renderer-independent render plan: {count} instances");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn validate_terrain_analysis(paths: &[String]) -> ExitCode {
    let read_json = |path: &str| -> Result<serde_json::Value, String> {
        let input =
            fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
        serde_json::from_str(&input).map_err(|error| format!("invalid JSON in {path}: {error}"))
    };
    let read_bytes = |path: &str| -> Result<Vec<u8>, String> {
        fs::read(path).map_err(|error| format!("cannot read {path}: {error}"))
    };
    let inputs = (
        read_json(&paths[0]),
        read_json(&paths[1]),
        read_bytes(&paths[2]),
        read_bytes(&paths[3]),
        read_bytes(&paths[4]),
        read_json(&paths[5]),
    );
    match inputs {
        (Ok(zone), Ok(manifest), Ok(heightfield), Ok(protected), Ok(regions), Ok(report)) => {
            match validate_terrain_analysis_value(
                &zone,
                &manifest,
                &heightfield,
                &protected,
                &regions,
                &report,
            ) {
                Ok(identity) => {
                    println!(
                        "valid terrain analysis: {} {}x{}, sha256:{}",
                        identity.zone_id,
                        identity.resolution,
                        identity.resolution,
                        identity.heightfield_sha256
                    );
                    if identity.status == "passed" {
                        ExitCode::SUCCESS
                    } else {
                        eprintln!("terrain quality policy failed");
                        ExitCode::from(1)
                    }
                }
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::from(1)
                }
            }
        }
        (Err(error), _, _, _, _, _)
        | (_, Err(error), _, _, _, _)
        | (_, _, Err(error), _, _, _)
        | (_, _, _, Err(error), _, _)
        | (_, _, _, _, Err(error), _)
        | (_, _, _, _, _, Err(error)) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn validate_plan(zone_path: &str, asset_path: &str, plan_path: &str) -> ExitCode {
    let read_json = |path: &str| -> Result<serde_json::Value, String> {
        let input =
            fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
        serde_json::from_str(&input).map_err(|error| format!("invalid JSON in {path}: {error}"))
    };
    match (
        read_json(zone_path),
        read_json(asset_path),
        read_json(plan_path),
    ) {
        (Ok(zone), Ok(assets), Ok(plan)) => {
            match validate_placement_plan_value(&zone, &assets, &plan) {
                Ok(count) => {
                    println!("valid placement plan: {count} placements");
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::from(1)
                }
            }
        }
        (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}
