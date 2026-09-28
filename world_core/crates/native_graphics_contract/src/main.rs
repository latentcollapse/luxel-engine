use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use wge_native_graphics_contract::lower_reference_world;
use wge_reference_runtime::build_from_layout_path;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("lower-layout") => {
            let layout = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "lower-layout requires LAYOUT PATH".to_owned())?,
                ),
                "layout",
            )?;
            let julia = PathBuf::from(
                arguments
                    .next()
                    .ok_or_else(|| "lower-layout requires JULIA PATH".to_owned())?,
            );
            let terrain_lab = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "lower-layout requires TERRAIN_LAB PATH".to_owned())?,
                ),
                "terrain lab",
            )?;
            if arguments.next().is_some() {
                return Err("lower-layout accepts exactly LAYOUT JULIA TERRAIN_LAB".into());
            }
            let world = build_from_layout_path(&layout, &julia, &terrain_lab)
                .map_err(|error| error.to_string())?;
            let packet = lower_reference_world(&world.world).map_err(|error| error.to_string())?;
            let bytes = serde_json::to_vec(&packet).map_err(|error| error.to_string())?;
            println!(
                "{}",
                String::from_utf8(bytes).map_err(|error| error.to_string())?
            );
            Ok(())
        }
        _ => {
            Err("usage: wge-native-graphics-contract lower-layout LAYOUT JULIA TERRAIN_LAB".into())
        }
    }
}

fn canonical_path(path: PathBuf, label: &str) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|error| format!("cannot resolve {label} {}: {error}", path.display()))
}
