use std::env;
use std::ffi::OsStr;
use std::path::Path;
use std::process::ExitCode;

use serde::Deserialize;
use luxel_gameplay_contract::{FailureCode, GameFailure, GameSnapshot, ReplayTrace, run_replay};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    snapshot: GameSnapshot,
    trace: ReplayTrace,
}

fn main() -> ExitCode {
    let arguments: Vec<_> = env::args_os().skip(1).collect();
    let [command, input, output] = arguments.as_slice() else {
        print_usage();
        return ExitCode::from(2);
    };
    if command != OsStr::new("run") {
        print_usage();
        return ExitCode::from(2);
    }

    match execute(Path::new(input), Path::new(output)) {
        Ok(receipt_sha256) => {
            println!("{receipt_sha256}");
            ExitCode::SUCCESS
        }
        Err(failure) => {
            eprintln!("GameFailure: {failure}");
            ExitCode::FAILURE
        }
    }
}

fn print_usage() {
    eprintln!("usage: luxel-gameplay-contract run FIXTURE.json OUTPUT.json");
}

fn execute(input_path: &Path, output_path: &Path) -> Result<String, GameFailure> {
    let input_bytes = std::fs::read(input_path).map_err(|error| {
        GameFailure::subject(
            FailureCode::InputReadFailed,
            input_path.display().to_string(),
            format!("could not read fixture: {error}"),
        )
    })?;
    let fixture: Fixture = serde_json::from_slice(&input_bytes).map_err(|error| {
        GameFailure::subject(
            FailureCode::MalformedInput,
            input_path.display().to_string(),
            format!("fixture must be a closed JSON object with snapshot and trace: {error}"),
        )
    })?;

    let receipt = run_replay(&fixture.snapshot, &fixture.trace)?;
    let receipt_bytes = receipt.canonical_bytes()?;
    std::fs::write(output_path, receipt_bytes).map_err(|error| {
        GameFailure::subject(
            FailureCode::OutputWriteFailed,
            output_path.display().to_string(),
            format!("could not write canonical receipt: {error}"),
        )
    })?;
    Ok(receipt.receipt_sha256)
}
