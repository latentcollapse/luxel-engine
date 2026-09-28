use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use wge_native_graphics_contract::{GraphicsWorkerSupervisor, lower_reference_world};
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
        Some("render-layout") => {
            let layout = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "render-layout requires LAYOUT PATH".to_owned())?,
                ),
                "layout",
            )?;
            let julia = PathBuf::from(
                arguments
                    .next()
                    .ok_or_else(|| "render-layout requires JULIA PATH".to_owned())?,
            );
            let terrain_lab = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "render-layout requires TERRAIN_LAB PATH".to_owned())?,
                ),
                "terrain lab",
            )?;
            let graphics_project = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "render-layout requires GRAPHICS PROJECT PATH".to_owned())?,
                ),
                "graphics project",
            )?;
            let worker = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "render-layout requires WORKER PATH".to_owned())?,
                ),
                "graphics worker",
            )?;
            let output = PathBuf::from(
                arguments
                    .next()
                    .ok_or_else(|| "render-layout requires OUTPUT PPM PATH".to_owned())?,
            );
            if arguments.next().is_some() {
                return Err(
                    "render-layout accepts exactly LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT"
                        .into(),
                );
            }
            let world = build_from_layout_path(&layout, &julia, &terrain_lab)
                .map_err(|error| error.to_string())?;
            let packet = lower_reference_world(&world.world).map_err(|error| error.to_string())?;
            let mut supervisor = GraphicsWorkerSupervisor::start(&julia, &graphics_project, &worker)
                .map_err(|error| error.to_string())?;
            supervisor.capabilities().map_err(|error| error.to_string())?;
            let promoted = supervisor
                .render_and_promote(&packet)
                .map_err(|error| error.to_string())?;
            fs::write(
                &output,
                rgba8_to_ppm(
                    &promoted.capture_bytes,
                    promoted.frame.width_px,
                    promoted.frame.height_px,
                )?,
            )
                .map_err(|error| format!("cannot write capture {}: {error}", output.display()))?;
            println!(
                "{}",
                serde_json::to_string_pretty(&promoted.receipt).map_err(|error| error.to_string())?
            );
            Ok(())
        }
        _ => {
            Err("usage: wge-native-graphics-contract lower-layout LAYOUT JULIA TERRAIN_LAB\n       wge-native-graphics-contract render-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT".into())
        }
    }
}

fn canonical_path(path: PathBuf, label: &str) -> Result<PathBuf, String> {
    path.canonicalize()
        .map_err(|error| format!("cannot resolve {label} {}: {error}", path.display()))
}

fn rgba8_to_ppm(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let expected = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "capture dimensions overflow PPM size".to_owned())?;
    if bytes.len() != expected {
        return Err(format!(
            "capture has {} bytes, expected {expected}",
            bytes.len()
        ));
    }
    let header = format!("P6\n{width} {height}\n255\n");
    let mut ppm = Vec::with_capacity(header.len() + expected / 4 * 3);
    ppm.extend_from_slice(header.as_bytes());
    for pixel in bytes.chunks_exact(4) {
        ppm.extend_from_slice(&pixel[..3]);
    }
    Ok(ppm)
}
