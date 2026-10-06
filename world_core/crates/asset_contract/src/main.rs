use std::env;
use std::fs;
use std::process::ExitCode;

use serde::Serialize;
use luxel_asset_contract::{
    AcceptanceStatus, AssetPreparationRequest, AssetUse, PhysicalCalibration, PhysicalRole,
    PreparationStatus, RenderConditioningRequest, RenderPreparationStatus, StructuralAcceptance,
    condition_render_asset, evaluate_physical_acceptance, evaluate_structural_acceptance,
    inspect_file, prepare_asset,
};

#[derive(Serialize)]
struct Output<'a> {
    inspection: &'a luxel_asset_contract::AssetReport,
    structural_acceptance: StructuralAcceptance,
    physical_acceptance: Option<luxel_asset_contract::PhysicalAcceptance>,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut args = env::args().skip(1);
    let Some(first) = args.next() else {
        return Err(usage().into());
    };
    if first == "prepare" {
        return run_prepare(args);
    }
    if first == "prepare-render" {
        return run_prepare_render(args);
    }
    let path = first;
    let mut asset_use = AssetUse::Unspecified;
    let mut role: Option<PhysicalRole> = None;
    let mut meters_per_unit = None;
    let mut placed_scale = None;
    let mut vertical_axis = None;

    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}\n{}", usage()))?;
        match flag.as_str() {
            "--kind" => {
                asset_use = match value.as_str() {
                    "character" => AssetUse::Character,
                    "static_mesh" => AssetUse::StaticMesh,
                    "unspecified" => AssetUse::Unspecified,
                    _ => return Err(format!("unsupported kind {value:?}\n{}", usage())),
                };
            }
            "--role" => {
                role =
                    Some(parse_role(&value).ok_or_else(|| {
                        format!("unsupported physical role {value:?}\n{}", usage())
                    })?);
            }
            "--meters-per-unit" => meters_per_unit = Some(parse_positive(&flag, &value)?),
            "--placed-scale" => placed_scale = Some(parse_positive(&flag, &value)?),
            "--vertical-axis" => {
                vertical_axis = Some(match value.as_str() {
                    "x" => luxel_asset_contract::Axis::X,
                    "y" => luxel_asset_contract::Axis::Y,
                    "z" => luxel_asset_contract::Axis::Z,
                    _ => return Err(format!("unsupported vertical axis {value:?}\n{}", usage())),
                });
            }
            _ => return Err(format!("unknown option {flag:?}\n{}", usage())),
        }
    }

    let report = inspect_file(&path).map_err(|error| error.to_string())?;
    let structural_acceptance = evaluate_structural_acceptance(&report, asset_use);
    let physical_acceptance = if let Some(role) = role {
        let any_calibration =
            meters_per_unit.is_some() || placed_scale.is_some() || vertical_axis.is_some();
        let calibration = if any_calibration {
            Some(PhysicalCalibration {
                meters_per_unit: meters_per_unit
                    .ok_or("--meters-per-unit is required when providing physical calibration")?,
                vertical_axis: vertical_axis
                    .ok_or("--vertical-axis is required when providing physical calibration")?,
                placed_scale: placed_scale
                    .ok_or("--placed-scale is required when providing physical calibration")?,
            })
        } else {
            None
        };
        Some(
            evaluate_physical_acceptance(&report, role, calibration)
                .map_err(|error| error.to_string())?,
        )
    } else {
        None
    };
    let output = Output {
        inspection: &report,
        structural_acceptance,
        physical_acceptance,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&output).map_err(|error| error.to_string())?
    );
    let status = output.structural_acceptance.status;
    Ok(if status == AcceptanceStatus::Rejected {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn run_prepare(args: impl Iterator<Item = String>) -> Result<ExitCode, String> {
    let values = args.collect::<Vec<_>>();
    if values.len() != 2 {
        return Err("usage: luxel-asset-contract prepare ASSET.glb REQUEST.json".into());
    }
    let asset_path = &values[0];
    let request_path = &values[1];
    let bytes =
        fs::read(asset_path).map_err(|error| format!("cannot read {asset_path}: {error}"))?;
    let request_bytes =
        fs::read(request_path).map_err(|error| format!("cannot read {request_path}: {error}"))?;
    let request: AssetPreparationRequest = serde_json::from_slice(&request_bytes)
        .map_err(|error| format!("invalid typed asset preparation request: {error}"))?;
    let receipt = prepare_asset(&bytes, &request).map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(match receipt.status {
        PreparationStatus::Ready => ExitCode::SUCCESS,
        PreparationStatus::Rejected => ExitCode::from(3),
    })
}

fn run_prepare_render(args: impl Iterator<Item = String>) -> Result<ExitCode, String> {
    let values = args.collect::<Vec<_>>();
    if values.len() != 2 {
        return Err("usage: luxel-asset-contract prepare-render ASSET.glb REQUEST.json".into());
    }
    let asset_path = &values[0];
    let request_path = &values[1];
    let bytes =
        fs::read(asset_path).map_err(|error| format!("cannot read {asset_path}: {error}"))?;
    let request_bytes =
        fs::read(request_path).map_err(|error| format!("cannot read {request_path}: {error}"))?;
    let request: RenderConditioningRequest = serde_json::from_slice(&request_bytes)
        .map_err(|error| format!("invalid typed render conditioning request: {error}"))?;
    let receipt = condition_render_asset(&bytes, &request).map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string_pretty(&receipt).map_err(|error| error.to_string())?
    );
    Ok(match receipt.status {
        RenderPreparationStatus::Ready => ExitCode::SUCCESS,
        RenderPreparationStatus::Rejected => ExitCode::from(3),
    })
}

fn parse_positive(flag: &str, value: &str) -> Result<f64, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|error| format!("{flag} must be a number: {error}"))?;
    if !parsed.is_finite() || parsed <= 0.0 {
        return Err(format!("{flag} must be finite and positive"));
    }
    Ok(parsed)
}

fn parse_role(value: &str) -> Option<PhysicalRole> {
    Some(match value {
        "conifer_canopy" => PhysicalRole::ConiferCanopy,
        "highland_understory" => PhysicalRole::HighlandUnderstory,
        "highland_groundcover" => PhysicalRole::HighlandGroundcover,
        "forest_floor_rock" => PhysicalRole::ForestFloorRock,
        "faction_fortification" => PhysicalRole::FactionFortification,
        "objective_landmark" => PhysicalRole::ObjectiveLandmark,
        "lane_guardian" => PhysicalRole::LaneGuardian,
        "settlement_landmark" => PhysicalRole::SettlementLandmark,
        "lane_crossing_structure" => PhysicalRole::LaneCrossingStructure,
        "landform_dressing" => PhysicalRole::LandformDressing,
        "other" => PhysicalRole::Other,
        _ => return None,
    })
}

fn usage() -> &'static str {
    "usage: luxel-asset-contract ASSET.glb [--kind character|static_mesh|unspecified] \
     [--role ROLE] [--meters-per-unit N --vertical-axis x|y|z --placed-scale N]\n\
     or: luxel-asset-contract prepare ASSET.glb REQUEST.json\n\
     or: luxel-asset-contract prepare-render ASSET.glb REQUEST.json"
}
