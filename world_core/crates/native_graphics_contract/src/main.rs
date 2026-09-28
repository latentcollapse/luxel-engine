use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use serde::Serialize;
use wge_native_graphics_contract::{
    GraphicsWorkerSupervisor, lower_dense_benchmark_packet, lower_objective_close_packet,
    lower_reference_world, lower_showcase_packet,
};
use wge_reference_runtime::build_from_layout_path;

const BENCHMARK_SCHEMA: &str = "wge.native-graphics-benchmark/v2";
const DEFAULT_WARM_FRAMES: usize = 30;
const MAX_WARM_FRAMES: usize = 256;

#[derive(Clone, Debug, Serialize)]
struct BenchmarkSample {
    wall_time_us: u64,
    renderer_frame_time_us: u64,
    gpu_frame_time_us: Option<u64>,
    capture_sha256: String,
    draw_calls: usize,
    pipeline_compilations: usize,
    upload_bytes: usize,
    readback_bytes: usize,
    visible_instance_count: usize,
    culled_instance_count: usize,
    background_visible_instance_count: usize,
    background_culled_instance_count: usize,
    mesh_vertex_count: usize,
    pass_timings: BenchmarkPassTimings,
}

#[derive(Clone, Debug, Serialize)]
struct BenchmarkPassTimings {
    prepare_us: u64,
    scene_raster_us: u64,
    resolve_us: u64,
    overlay_us: u64,
    flush_readback_us: u64,
    gpu_prepare_us: Option<u64>,
    gpu_scene_raster_us: Option<u64>,
    gpu_resolve_us: Option<u64>,
    gpu_overlay_us: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
struct TimingSummary {
    sample_count: usize,
    min_us: u64,
    p50_us: u64,
    p95_us: u64,
    p99_us: u64,
    max_us: u64,
    mean_us: u64,
}

#[derive(Clone, Debug, Serialize)]
struct BenchmarkPassSummary {
    prepare_us: TimingSummary,
    scene_raster_us: TimingSummary,
    resolve_us: TimingSummary,
    overlay_us: TimingSummary,
    flush_readback_us: TimingSummary,
    gpu_prepare_us: Option<TimingSummary>,
    gpu_scene_raster_us: Option<TimingSummary>,
    gpu_resolve_us: Option<TimingSummary>,
    gpu_overlay_us: Option<TimingSummary>,
}

#[derive(Clone, Debug, Serialize)]
struct BenchmarkReport {
    schema: &'static str,
    profile: &'static str,
    instance_count: usize,
    packet_sha256: String,
    cold: BenchmarkSample,
    warm_wall_time: TimingSummary,
    warm_renderer_frame_time: TimingSummary,
    warm_gpu_frame_time: Option<TimingSummary>,
    warm_pass_timings: BenchmarkPassSummary,
    warm_capture_sha256: String,
    deterministic_capture: bool,
}

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
    let command = arguments.next();
    match command.as_deref() {
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
        Some("render-layout") | Some("render-close-layout") | Some("render-showcase-layout") => {
            let close_view = command.as_deref() == Some("render-close-layout");
            let showcase_view = command.as_deref() == Some("render-showcase-layout");
            let layout = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "render command requires LAYOUT PATH".to_owned())?,
                ),
                "layout",
            )?;
            let julia = PathBuf::from(
                arguments
                    .next()
                    .ok_or_else(|| "render command requires JULIA PATH".to_owned())?,
            );
            let terrain_lab = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "render command requires TERRAIN_LAB PATH".to_owned())?,
                ),
                "terrain lab",
            )?;
            let graphics_project = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "render command requires GRAPHICS PROJECT PATH".to_owned())?,
                ),
                "graphics project",
            )?;
            let worker = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "render command requires WORKER PATH".to_owned())?,
                ),
                "graphics worker",
            )?;
            let output = PathBuf::from(
                arguments
                    .next()
                    .ok_or_else(|| "render command requires OUTPUT PPM PATH".to_owned())?,
            );
            if arguments.next().is_some() {
                return Err(
                    "render command accepts exactly LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT"
                        .into(),
                );
            }
            let world = build_from_layout_path(&layout, &julia, &terrain_lab)
                .map_err(|error| error.to_string())?;
            let packet = lower_reference_world(&world.world).map_err(|error| error.to_string())?;
            let packet = if showcase_view {
                lower_showcase_packet(&packet).map_err(|error| error.to_string())?
            } else if close_view {
                lower_objective_close_packet(&packet).map_err(|error| error.to_string())?
            } else {
                packet
            };
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
        Some("benchmark-layout") | Some("benchmark-dense-layout") => {
            let dense_profile = command.as_deref() == Some("benchmark-dense-layout");
            let layout = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "benchmark-layout requires LAYOUT PATH".to_owned())?,
                ),
                "layout",
            )?;
            let julia = PathBuf::from(
                arguments
                    .next()
                    .ok_or_else(|| "benchmark-layout requires JULIA PATH".to_owned())?,
            );
            let terrain_lab = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "benchmark-layout requires TERRAIN_LAB PATH".to_owned())?,
                ),
                "terrain lab",
            )?;
            let graphics_project = canonical_path(
                PathBuf::from(
                    arguments.next().ok_or_else(|| {
                        "benchmark-layout requires GRAPHICS PROJECT PATH".to_owned()
                    })?,
                ),
                "graphics project",
            )?;
            let worker = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "benchmark-layout requires WORKER PATH".to_owned())?,
                ),
                "graphics worker",
            )?;
            let warm_frames = arguments
                .next()
                .map(|raw| {
                    raw.parse::<usize>()
                        .map_err(|error| format!("warm frame count is not an integer: {error}"))
                })
                .transpose()?
                .unwrap_or(DEFAULT_WARM_FRAMES);
            if !(1..=MAX_WARM_FRAMES).contains(&warm_frames) {
                return Err(format!(
                    "warm frame count must be between 1 and {MAX_WARM_FRAMES}"
                ));
            }
            let dense_background_instances = if dense_profile {
                Some(
                    arguments
                        .next()
                        .ok_or_else(|| {
                            "benchmark-dense-layout requires DENSE_BACKGROUND_INSTANCES".to_owned()
                        })?
                        .parse::<usize>()
                        .map_err(|error| {
                            format!("dense background instance count is not an integer: {error}")
                        })?,
                )
            } else {
                None
            };
            if arguments.next().is_some() {
                return Err(
                    "benchmark command received unexpected trailing arguments"
                        .into(),
                );
            }

            let world = build_from_layout_path(&layout, &julia, &terrain_lab)
                .map_err(|error| error.to_string())?;
            let packet = lower_reference_world(&world.world).map_err(|error| error.to_string())?;
            let packet = if let Some(background_instance_count) = dense_background_instances {
                lower_dense_benchmark_packet(&packet, background_instance_count)
                    .map_err(|error| error.to_string())?
            } else {
                packet
            };
            let mut supervisor = GraphicsWorkerSupervisor::start(&julia, &graphics_project, &worker)
                .map_err(|error| error.to_string())?;
            supervisor.capabilities().map_err(|error| error.to_string())?;
            let (cold, cold_capture) = measure_frame(&mut supervisor, &packet)?;
            let mut warm_samples = Vec::with_capacity(warm_frames);
            let mut warm_captures = Vec::with_capacity(warm_frames);
            for _ in 0..warm_frames {
                let (sample, capture) = measure_frame(&mut supervisor, &packet)?;
                warm_samples.push(sample);
                warm_captures.push(capture);
            }
            let warm_capture_sha256 = warm_captures
                .first()
                .cloned()
                .ok_or_else(|| "benchmark produced no warm capture".to_owned())?;
            let deterministic_capture = cold_capture == warm_capture_sha256
                && warm_captures.iter().all(|capture| capture == &warm_capture_sha256);
            let report = BenchmarkReport {
                schema: BENCHMARK_SCHEMA,
                profile: if dense_profile {
                    "synthetic-dense-foliage"
                } else {
                    "certified-overview"
                },
                instance_count: packet.body.instances.len(),
                packet_sha256: packet.packet_sha256.clone(),
                cold,
                warm_wall_time: summarize(
                    &warm_samples
                        .iter()
                        .map(|sample| sample.wall_time_us)
                        .collect::<Vec<_>>(),
                )?,
                warm_renderer_frame_time: summarize(
                    &warm_samples
                        .iter()
                        .map(|sample| sample.renderer_frame_time_us)
                        .collect::<Vec<_>>(),
                )?,
                warm_gpu_frame_time: summarize_optional_gpu_time(&warm_samples)?,
                warm_pass_timings: summarize_pass_timings(&warm_samples)?,
                warm_capture_sha256,
                deterministic_capture,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?
            );
            if !report.deterministic_capture {
                return Err("benchmark captures were not deterministic".into());
            }
            Ok(())
        }
        _ => {
            Err("usage: wge-native-graphics-contract lower-layout LAYOUT JULIA TERRAIN_LAB\n       wge-native-graphics-contract render-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT\n       wge-native-graphics-contract render-close-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT\n       wge-native-graphics-contract render-showcase-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT\n       wge-native-graphics-contract benchmark-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER [WARM_FRAMES]\n       wge-native-graphics-contract benchmark-dense-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER [WARM_FRAMES] DENSE_BACKGROUND_INSTANCES".into())
        }
    }
}

fn measure_frame(
    supervisor: &mut GraphicsWorkerSupervisor,
    packet: &wge_native_graphics_contract::GraphicsScenePacket,
) -> Result<(BenchmarkSample, String), String> {
    let started = Instant::now();
    let promoted = supervisor
        .render_and_promote(packet)
        .map_err(|error| error.to_string())?;
    let telemetry = &promoted.receipt.body.telemetry;
    let sample = BenchmarkSample {
        wall_time_us: started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64,
        renderer_frame_time_us: telemetry.frame_time_us,
        gpu_frame_time_us: telemetry.gpu_frame_time_us,
        capture_sha256: promoted.frame.capture_sha256.clone(),
        draw_calls: telemetry.draw_calls,
        pipeline_compilations: telemetry.pipeline_compilations,
        upload_bytes: telemetry.upload_bytes,
        readback_bytes: telemetry.readback_bytes,
        visible_instance_count: telemetry.visible_instance_count,
        culled_instance_count: telemetry.culled_instance_count,
        background_visible_instance_count: telemetry.background_visible_instance_count,
        background_culled_instance_count: telemetry.background_culled_instance_count,
        mesh_vertex_count: telemetry.mesh_vertex_count,
        pass_timings: BenchmarkPassTimings {
            prepare_us: telemetry.pass_timings.prepare_us,
            scene_raster_us: telemetry.pass_timings.scene_raster_us,
            resolve_us: telemetry.pass_timings.resolve_us,
            overlay_us: telemetry.pass_timings.overlay_us,
            flush_readback_us: telemetry.pass_timings.flush_readback_us,
            gpu_prepare_us: telemetry.pass_timings.gpu_prepare_us,
            gpu_scene_raster_us: telemetry.pass_timings.gpu_scene_raster_us,
            gpu_resolve_us: telemetry.pass_timings.gpu_resolve_us,
            gpu_overlay_us: telemetry.pass_timings.gpu_overlay_us,
        },
    };
    Ok((sample, promoted.frame.capture_sha256))
}

fn summarize(values: &[u64]) -> Result<TimingSummary, String> {
    if values.is_empty() {
        return Err("benchmark timing samples must not be empty".into());
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let sum = sorted.iter().map(|value| u128::from(*value)).sum::<u128>();
    Ok(TimingSummary {
        sample_count: sorted.len(),
        min_us: sorted[0],
        p50_us: nearest_rank(&sorted, 1, 2),
        p95_us: nearest_rank(&sorted, 19, 20),
        p99_us: nearest_rank(&sorted, 99, 100),
        max_us: *sorted.last().expect("non-empty timing samples"),
        mean_us: (sum / sorted.len() as u128).min(u128::from(u64::MAX)) as u64,
    })
}

fn summarize_optional_gpu_time(
    samples: &[BenchmarkSample],
) -> Result<Option<TimingSummary>, String> {
    let values = samples
        .iter()
        .filter_map(|sample| sample.gpu_frame_time_us)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Ok(None);
    }
    if values.len() != samples.len() {
        return Err("benchmark GPU timing was present for only some warm frames".into());
    }
    summarize(&values).map(Some)
}

fn summarize_pass_timings(samples: &[BenchmarkSample]) -> Result<BenchmarkPassSummary, String> {
    let summarize_field = |field: fn(&BenchmarkPassTimings) -> u64| {
        summarize(
            &samples
                .iter()
                .map(|sample| field(&sample.pass_timings))
                .collect::<Vec<_>>(),
        )
    };
    let summarize_optional_field = |field: fn(&BenchmarkPassTimings) -> Option<u64>| {
        summarize_optional_values(
            &samples
                .iter()
                .map(|sample| field(&sample.pass_timings))
                .collect::<Vec<_>>(),
        )
    };
    Ok(BenchmarkPassSummary {
        prepare_us: summarize_field(|timings| timings.prepare_us)?,
        scene_raster_us: summarize_field(|timings| timings.scene_raster_us)?,
        resolve_us: summarize_field(|timings| timings.resolve_us)?,
        overlay_us: summarize_field(|timings| timings.overlay_us)?,
        flush_readback_us: summarize_field(|timings| timings.flush_readback_us)?,
        gpu_prepare_us: summarize_optional_field(|timings| timings.gpu_prepare_us)?,
        gpu_scene_raster_us: summarize_optional_field(|timings| timings.gpu_scene_raster_us)?,
        gpu_resolve_us: summarize_optional_field(|timings| timings.gpu_resolve_us)?,
        gpu_overlay_us: summarize_optional_field(|timings| timings.gpu_overlay_us)?,
    })
}

fn summarize_optional_values(values: &[Option<u64>]) -> Result<Option<TimingSummary>, String> {
    let present = values.iter().flatten().copied().collect::<Vec<_>>();
    if present.is_empty() {
        return Ok(None);
    }
    if present.len() != values.len() {
        return Err("benchmark pass GPU timing was present for only some warm frames".to_owned());
    }
    summarize(&present).map(Some)
}

fn nearest_rank(sorted: &[u64], numerator: usize, denominator: usize) -> u64 {
    let rank = (sorted.len() * numerator).div_ceil(denominator).max(1);
    sorted[rank - 1]
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

#[cfg(test)]
mod tests {
    use super::{nearest_rank, summarize};

    #[test]
    fn benchmark_percentiles_use_nearest_rank() {
        let values = [10, 20, 30, 40, 50];
        assert_eq!(nearest_rank(&values, 1, 2), 30);
        assert_eq!(nearest_rank(&values, 19, 20), 50);
        assert_eq!(nearest_rank(&values, 99, 100), 50);
    }

    #[test]
    fn benchmark_summary_is_sorted_and_integer_stable() {
        let summary = summarize(&[40, 10, 20, 30]).expect("samples are non-empty");
        assert_eq!(summary.sample_count, 4);
        assert_eq!(summary.min_us, 10);
        assert_eq!(summary.p50_us, 20);
        assert_eq!(summary.p95_us, 40);
        assert_eq!(summary.mean_us, 25);
    }
}
