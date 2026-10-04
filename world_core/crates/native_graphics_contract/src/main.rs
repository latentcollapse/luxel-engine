use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use serde::Serialize;
use wge_native_graphics_contract::{
    Campaign2View, GraphicsWorkerSupervisor, QualityOutcome, VisualQualityProfile,
    assess_campaign2_visual_evidence, assess_visual_quality,
    deterministic_certification_frame_receipt, load_terrain_layer_set,
    lower_campaign2_packet_with, ParityContent, ParityPolicyCandidate,
    lower_dense_benchmark_packet, lower_objective_close_packet, lower_reference_world,
    lower_showcase_packet, lower_world_showcase_packet, sha256_prefixed,
    validate_campaign2_visual_evidence,
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

#[derive(Clone, Debug, Serialize)]
struct CaptureManifest {
    schema: &'static str,
    exporter: &'static str,
    capture_path: String,
    ppm_sha256: String,
    raw_rgba_sha256: String,
    receipt_sha256: String,
    packet_sha256: String,
    capture_id: String,
    camera_id: String,
    width_px: u32,
    height_px: u32,
    format: &'static str,
}

const CAPTURE_MANIFEST_SCHEMA: &str = "wge.native-graphics-capture-manifest/v1";
const CAPTURE_EXPORTER: &str = "wge-native-graphics-contract-cli/v1";

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
        Some("render-layout")
        | Some("render-close-layout")
        | Some("render-showcase-layout")
        | Some("render-world-showcase-layout") => {
            let close_view = command.as_deref() == Some("render-close-layout");
            let showcase_view = command.as_deref() == Some("render-showcase-layout");
            let world_showcase_view = command.as_deref() == Some("render-world-showcase-layout");
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
            } else if world_showcase_view {
                lower_world_showcase_packet(&packet).map_err(|error| error.to_string())?
            } else if close_view {
                lower_objective_close_packet(&packet).map_err(|error| error.to_string())?
            } else {
                packet
            };
            let mut supervisor = GraphicsWorkerSupervisor::start(&julia, &graphics_project, &worker)
                .map_err(|error| error.to_string())?;
            supervisor.capabilities().map_err(|error| error.to_string())?;
            let promoted = supervisor
                .render_and_promote(&packet, &world.world)
                .map_err(|error| error.to_string())?;
            if let Some(parent) = output.parent()
                && !parent.as_os_str().is_empty()
            {
                fs::create_dir_all(parent).map_err(|error| {
                    format!("cannot create capture directory {}: {error}", parent.display())
                })?;
            }
            let ppm = rgba8_to_ppm(
                &promoted.capture_bytes,
                promoted.frame.width_px,
                promoted.frame.height_px,
            )?;
            fs::write(&output, &ppm)
                .map_err(|error| format!("cannot write capture {}: {error}", output.display()))?;
            let manifest = CaptureManifest {
                schema: CAPTURE_MANIFEST_SCHEMA,
                exporter: CAPTURE_EXPORTER,
                capture_path: output.display().to_string(),
                ppm_sha256: sha256_prefixed(&ppm),
                raw_rgba_sha256: promoted.frame.capture_sha256.clone(),
                receipt_sha256: promoted.receipt.receipt_sha256.clone(),
                packet_sha256: promoted.frame.packet_sha256.clone(),
                capture_id: promoted.frame.capture_id.clone(),
                camera_id: packet.body.capture.camera_id.clone(),
                width_px: promoted.frame.width_px,
                height_px: promoted.frame.height_px,
                format: "image/x-portable-pixmap; magic=P6",
            };
            let manifest_path = output.with_extension("manifest.json");
            let manifest_bytes = serde_json::to_vec_pretty(&manifest)
                .map_err(|error| format!("cannot serialize capture manifest: {error}"))?;
            fs::write(&manifest_path, manifest_bytes).map_err(|error| {
                format!("cannot write capture manifest {}: {error}", manifest_path.display())
            })?;
            println!(
                "{}",
                serde_json::to_string_pretty(&promoted.receipt).map_err(|error| error.to_string())?
            );
            Ok(())
        }
        Some("render-quality-layout") => {
            let layout = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "render-quality-layout requires LAYOUT PATH".to_owned())?,
                ),
                "layout",
            )?;
            let julia = PathBuf::from(
                arguments
                    .next()
                    .ok_or_else(|| "render-quality-layout requires JULIA PATH".to_owned())?,
            );
            let terrain_lab = canonical_path(
                PathBuf::from(arguments.next().ok_or_else(|| {
                    "render-quality-layout requires TERRAIN LAB PATH".to_owned()
                })?),
                "terrain lab",
            )?;
            let graphics_project = canonical_path(
                PathBuf::from(arguments.next().ok_or_else(|| {
                    "render-quality-layout requires GRAPHICS PROJECT PATH".to_owned()
                })?),
                "graphics project",
            )?;
            let worker = canonical_path(
                PathBuf::from(arguments.next().ok_or_else(|| {
                    "render-quality-layout requires GRAPHICS WORKER PATH".to_owned()
                })?),
                "graphics worker",
            )?;
            let output_dir = PathBuf::from(arguments.next().ok_or_else(|| {
                "render-quality-layout requires OUTPUT DIRECTORY".to_owned()
            })?);
            if arguments.next().is_some() {
                return Err("render-quality-layout accepts exactly LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT_DIR".into());
            }

            let world = build_from_layout_path(&layout, &julia, &terrain_lab)
                .map_err(|error| error.to_string())?;
            // The quality command deliberately renders the canonical WGE
            // world-showcase composition. It remains bound to the authored
            // world artifact and spatial fields, while providing enough
            // authored geometry/material structure for the technical visual
            // floor to measure a real scene rather than a diagnostic flat
            // overview. This is WGE machinery, not a comparison fixture.
            let reference_packet =
                lower_reference_world(&world.world).map_err(|error| error.to_string())?;
            let packet = lower_world_showcase_packet(&reference_packet)
                .map_err(|error| error.to_string())?;
            let mut supervisor = GraphicsWorkerSupervisor::start(&julia, &graphics_project, &worker)
                .map_err(|error| error.to_string())?;
            supervisor.capabilities().map_err(|error| error.to_string())?;
            let promoted = supervisor
                .render_and_promote(&packet, &world.world)
                .map_err(|error| error.to_string())?;
            let certification_receipt = deterministic_certification_frame_receipt(
                &packet,
                &promoted.receipt,
                &promoted.capture_bytes,
            )
            .map_err(|error| error.to_string())?;
            let profile = VisualQualityProfile::terrain_reference_v1(
                packet.body.capture.width_px,
                packet.body.capture.height_px,
            );
            let evidence = assess_visual_quality(
                &packet,
                &certification_receipt,
                &promoted.capture_bytes,
                &profile,
            );

            fs::create_dir_all(&output_dir).map_err(|error| {
                format!(
                    "cannot create visual-quality artifact directory {}: {error}",
                    output_dir.display()
                )
            })?;
            write_json_artifact(&output_dir.join("world_artifact.json"), &world.world)?;
            write_json_artifact(&output_dir.join("graphics_scene_packet.json"), &packet)?;
            write_json_artifact(
                &output_dir.join("graphics_frame_receipt.json"),
                &certification_receipt,
            )?;
            write_json_artifact(
                &output_dir.join("graphics_renderer_attestation.json"),
                &promoted.renderer_attestation,
            )?;
            fs::write(
                output_dir.join("native_capture.rgba"),
                &promoted.capture_bytes,
            )
            .map_err(|error| format!("cannot write native RGBA capture: {error}"))?;
            let ppm = rgba8_to_ppm(
                &promoted.capture_bytes,
                promoted.frame.width_px,
                promoted.frame.height_px,
            )?;
            fs::write(output_dir.join("native_capture.ppm"), &ppm)
                .map_err(|error| format!("cannot write native PPM capture: {error}"))?;
            write_json_artifact(
                &output_dir.join("visual_quality_evidence.json"),
                &evidence,
            )?;
            let outcome = serde_json::to_value(evidence.body.outcome)
                .map_err(|error| format!("cannot serialize visual-quality outcome: {error}"))?;
            let summary = serde_json::json!({
                "schema_version": "wge.native-visual-quality-artifacts/v1",
                "outcome": outcome,
                "packet_sha256": packet.packet_sha256,
                "frame_receipt_sha256": certification_receipt.receipt_sha256,
                "renderer_attestation_sha256": promoted
                    .renderer_attestation
                    .attestation_sha256,
                "capture_sha256": sha256_prefixed(&promoted.capture_bytes),
                "capture_ppm_sha256": sha256_prefixed(&ppm),
                "evidence_sha256": evidence.evidence_sha256,
                "world_artifact_id": world.world.artifact_id,
                "world_artifact_sha256": world.world.artifact_sha256,
                "render_profile": "native-world-showcase-v1",
                "packet_path": output_dir.join("graphics_scene_packet.json"),
                "frame_receipt_path": output_dir.join("graphics_frame_receipt.json"),
                "renderer_attestation_path":
                    output_dir.join("graphics_renderer_attestation.json"),
                "capture_path": output_dir.join("native_capture.rgba"),
                "capture_ppm_path": output_dir.join("native_capture.ppm"),
                "evidence_path": output_dir.join("visual_quality_evidence.json"),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&summary)
                    .map_err(|error| format!("cannot serialize visual-quality summary: {error}"))?
            );
            Ok(())
        }
        Some("render-campaign2-layout") => {
            let layout = canonical_path(
                PathBuf::from(
                    arguments
                        .next()
                        .ok_or_else(|| "render-campaign2-layout requires LAYOUT PATH".to_owned())?,
                ),
                "layout",
            )?;
            let julia = PathBuf::from(arguments.next().ok_or_else(|| {
                "render-campaign2-layout requires JULIA PATH".to_owned()
            })?);
            let terrain_lab = canonical_path(
                PathBuf::from(arguments.next().ok_or_else(|| {
                    "render-campaign2-layout requires TERRAIN LAB PATH".to_owned()
                })?),
                "terrain lab",
            )?;
            let graphics_project = canonical_path(
                PathBuf::from(arguments.next().ok_or_else(|| {
                    "render-campaign2-layout requires GRAPHICS PROJECT PATH".to_owned()
                })?),
                "graphics project",
            )?;
            let worker = canonical_path(
                PathBuf::from(arguments.next().ok_or_else(|| {
                    "render-campaign2-layout requires WORKER PATH".to_owned()
                })?),
                "graphics worker",
            )?;
            let output_dir = PathBuf::from(arguments.next().ok_or_else(|| {
                "render-campaign2-layout requires OUTPUT DIRECTORY".to_owned()
            })?);
            if arguments.next().is_some() {
                return Err("render-campaign2-layout accepts exactly LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT_DIR".into());
            }

            let world = build_from_layout_path(&layout, &julia, &terrain_lab)
                .map_err(|error| error.to_string())?;
            let reference_packet =
                lower_reference_world(&world.world).map_err(|error| error.to_string())?;
            let mut supervisor = GraphicsWorkerSupervisor::start(&julia, &graphics_project, &worker)
                .map_err(|error| error.to_string())?;
            supervisor.capabilities().map_err(|error| error.to_string())?;
            fs::create_dir_all(&output_dir).map_err(|error| {
                format!(
                    "cannot create Campaign 2 artifact directory {}: {error}",
                    output_dir.display()
                )
            })?;

            // N-4: converge0 surfaces its terrain with a scanned layer set. The
            // manifest path is explicit (env), the files are digest-verified on
            // load, and the same set is handed to the supervisor so it can
            // re-derive the authorized packet.
            let parity_candidate = ParityPolicyCandidate::from_env();
            let parity_content = ParityContent::from_env(parity_candidate).map_err(|error| error.to_string())?;
            let terrain_layers = if parity_content.converge0 {
                let manifest = env::var("WGE_TERRAIN_LAYER_SET").map_err(|_| {
                    "converge0 needs WGE_TERRAIN_LAYER_SET=<manifest>, e.g. tools/terrain_layers/converge0.json \
                     (fetch the files first with tools/fetch_terrain_layers.py)"
                        .to_owned()
                })?;
                let root = env::current_dir().map_err(|error| error.to_string())?;
                Some(
                    load_terrain_layer_set(&PathBuf::from(manifest), &root)
                        .map_err(|error| format!("terrain layer set failed to load: {error}"))?,
                )
            } else {
                None
            };
            let view_specs = [
                ("close", Campaign2View::Close),
                ("medium", Campaign2View::Medium),
                ("wide", Campaign2View::Wide),
            ];
            let mut view_summaries = Vec::with_capacity(view_specs.len());
            let mut failed_views = Vec::new();
            for (view_name, view) in view_specs {
                let packet = lower_campaign2_packet_with(
                    &reference_packet,
                    view,
                    parity_candidate,
                    parity_content,
                    terrain_layers.as_ref(),
                )
                .map_err(|error| format!("Campaign 2 {view_name} lowering failed: {error}"))?;
                let promoted = match &terrain_layers {
                    Some(set) => supervisor.render_and_promote_with_terrain_layers(&packet, &world.world, set),
                    None => supervisor.render_and_promote(&packet, &world.world),
                }
                .map_err(|error| format!("Campaign 2 {view_name} render failed: {error}"))?;
                let certification_receipt = deterministic_certification_frame_receipt(
                    &packet,
                    &promoted.receipt,
                    &promoted.capture_bytes,
                )
                .map_err(|error| {
                    format!("Campaign 2 {view_name} certification projection failed: {error}")
                })?;
                let profile = VisualQualityProfile::campaign2_authored_frame_v1(
                    packet.body.capture.width_px,
                    packet.body.capture.height_px,
                );
                let evidence = assess_visual_quality(
                    &packet,
                    &certification_receipt,
                    &promoted.capture_bytes,
                    &profile,
                );
                let view_role = match view {
                    Campaign2View::Close => "close-material-hero",
                    Campaign2View::Medium => "medium-terrain-environment",
                    Campaign2View::Wide => "wide-world-composition",
                };
                let vector_evidence = assess_campaign2_visual_evidence(
                    &packet,
                    &certification_receipt,
                    &promoted.receipt,
                    &evidence,
                    &promoted.capture_bytes,
                    view_role,
                );
                validate_campaign2_visual_evidence(
                    &vector_evidence,
                    &packet,
                    &certification_receipt,
                    &promoted.receipt,
                    &evidence,
                    &promoted.capture_bytes,
                )
                .map_err(|error| {
                    format!("Campaign 2 {view_name} vector evidence failed revalidation: {error}")
                })?;
                let view_dir = output_dir.join(view_name);
                fs::create_dir_all(&view_dir).map_err(|error| {
                    format!("cannot create Campaign 2 {view_name} directory: {error}")
                })?;
                write_json_artifact(&view_dir.join("world_artifact.json"), &world.world)?;
                write_json_artifact(&view_dir.join("graphics_scene_packet.json"), &packet)?;
                write_json_artifact(
                    &view_dir.join("graphics_frame_receipt.json"),
                    &certification_receipt,
                )?;
                write_json_artifact(
                    &view_dir.join("graphics_renderer_attestation.json"),
                    &promoted.renderer_attestation,
                )?;
                fs::write(view_dir.join("native_capture.rgba"), &promoted.capture_bytes)
                    .map_err(|error| format!("cannot write Campaign 2 {view_name} RGBA: {error}"))?;
                let ppm = rgba8_to_ppm(
                    &promoted.capture_bytes,
                    promoted.frame.width_px,
                    promoted.frame.height_px,
                )?;
                fs::write(view_dir.join("native_capture.ppm"), &ppm)
                    .map_err(|error| format!("cannot write Campaign 2 {view_name} PPM: {error}"))?;
                write_json_artifact(&view_dir.join("visual_quality_evidence.json"), &evidence)?;
                write_json_artifact(
                    &view_dir.join("campaign2_visual_evidence.json"),
                    &vector_evidence,
                )?;
                let passed = evidence.body.outcome == QualityOutcome::Good;
                if !passed {
                    failed_views.push(format!(
                        "{view_name}: {:?}",
                        evidence.body.reasons
                    ));
                }
                view_summaries.push(serde_json::json!({
                    "view": view_name,
                    "camera_id": packet.body.camera.camera_id,
                    "packet_sha256": packet.packet_sha256,
                    "frame_receipt_sha256": certification_receipt.receipt_sha256,
                    "capture_sha256": sha256_prefixed(&promoted.capture_bytes),
                    "capture_ppm_sha256": sha256_prefixed(&ppm),
                    "evidence_sha256": evidence.evidence_sha256,
                    "campaign2_visual_evidence_sha256": vector_evidence.evidence_sha256,
                    "quality_outcome": evidence.body.outcome,
                    "measurements": evidence.body.measurements,
                    "visual_vector": vector_evidence.body.measurements,
                    "output_dir": view_dir,
                }));
            }
            let summary = serde_json::json!({
                "schema_version": "wge.native-graphics-campaign2/v1",
                "campaign": "WGE GRAPHICS CAMPAIGN 2 — THE AUTHORED FRAME",
                "world_artifact_id": world.world.artifact_id,
                "world_artifact_sha256": world.world.artifact_sha256,
                "views": view_summaries,
                "outcome": if failed_views.is_empty() { "good" } else { "bad" },
                "failed_views": failed_views,
            });
            write_json_artifact(&output_dir.join("campaign2_summary.json"), &summary)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&summary)
                    .map_err(|error| format!("cannot serialize Campaign 2 summary: {error}"))?
            );
            if summary["outcome"] != "good" {
                return Err("Campaign 2 visual-quality gate failed for one or more views".into());
            }
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
            let (cold, cold_capture) = measure_frame(&mut supervisor, &packet, &world.world)?;
            let mut warm_samples = Vec::with_capacity(warm_frames);
            let mut warm_captures = Vec::with_capacity(warm_frames);
            for _ in 0..warm_frames {
                let (sample, capture) = measure_frame(&mut supervisor, &packet, &world.world)?;
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
        Some("render-calibration") => run_render_calibration(arguments.collect()),
        _ => {
            Err("usage: wge-native-graphics-contract lower-layout LAYOUT JULIA TERRAIN_LAB\n       wge-native-graphics-contract render-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT\n       wge-native-graphics-contract render-close-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT\n       wge-native-graphics-contract render-showcase-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT\n       wge-native-graphics-contract render-world-showcase-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT\n       wge-native-graphics-contract render-quality-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT_DIR\n       wge-native-graphics-contract render-campaign2-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER OUTPUT_DIR\n       wge-native-graphics-contract benchmark-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER [WARM_FRAMES]\n       wge-native-graphics-contract benchmark-dense-layout LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER [WARM_FRAMES] DENSE_BACKGROUND_INSTANCES\n       wge-native-graphics-contract render-calibration LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER CALIBRATION_GLB OUTPUT_DIR [--rigs all|sun,overcast,grazing,sun-albedo-grey] [--views all|row|close|grazing|VIEW,...]".into())
        }
    }
}

/// CALIBRATION-1 (WGE_CONVERGE1_CONTRACTS.md §3): render the material
/// calibration scene through the authorized bound-scene route, for each
/// requested enumerated rig and derived view.
fn run_render_calibration(arguments: Vec<String>) -> Result<(), String> {
    use wge_asset_contract::{PreparationStatus, RenderPreparationStatus, condition_render_asset, prepare_asset};
    use wge_native_graphics_contract::calibration::{
        GREY_CARD_TARGET_SRGB8, grey_card_local_center, local_sphere, project_local_point, project_local_sphere,
        render_request, runtime_request,
    };
    use wge_native_graphics_contract::{
        BoundSceneRenderAuthorization, CalibrationPlacement, CalibrationRig, CalibrationView,
        compose_bound_scene_with_view, project_render_asset, validate_graphics_asset_projection,
    };

    const USAGE: &str = "render-calibration LAYOUT JULIA TERRAIN_LAB GRAPHICS_PROJECT WORKER CALIBRATION_GLB OUTPUT_DIR [--rigs ...] [--views ...]";
    let mut positional = Vec::new();
    let mut rig_filter = "all".to_owned();
    let mut view_filter = "row,close-calibration".to_owned();
    let mut rest = arguments.into_iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--rigs" => rig_filter = rest.next().ok_or_else(|| format!("--rigs needs a value; {USAGE}"))?,
            "--views" => view_filter = rest.next().ok_or_else(|| format!("--views needs a value; {USAGE}"))?,
            _ => positional.push(argument),
        }
    }
    let [layout, julia, terrain_lab, graphics_project, worker, glb_path, output_dir]: [String; 7] =
        positional.try_into().map_err(|_| USAGE.to_owned())?;
    let layout = canonical_path(PathBuf::from(layout), "layout")?;
    let julia = PathBuf::from(julia);
    let terrain_lab = canonical_path(PathBuf::from(terrain_lab), "terrain lab")?;
    let graphics_project = canonical_path(PathBuf::from(graphics_project), "graphics project")?;
    let worker = canonical_path(PathBuf::from(worker), "graphics worker")?;
    let output_dir = PathBuf::from(output_dir);

    let rigs: Vec<CalibrationRig> = if rig_filter == "all" {
        CalibrationRig::ALL.to_vec()
    } else {
        rig_filter
            .split(',')
            .map(|name| CalibrationRig::parse(name.trim()).ok_or_else(|| format!("unknown calibration rig `{name}`")))
            .collect::<Result<_, _>>()?
    };
    let all_views = CalibrationView::all();
    let views: Vec<CalibrationView> = if view_filter == "all" {
        all_views
    } else {
        let tokens: Vec<&str> = view_filter.split(',').map(str::trim).collect();
        let selected: Vec<CalibrationView> = all_views
            .into_iter()
            .filter(|view| {
                let name = view.name();
                tokens.iter().any(|token| {
                    *token == name || name.split('-').next() == Some(*token) && !token.contains('-')
                })
            })
            .collect();
        if selected.is_empty() {
            return Err(format!("--views `{view_filter}` selects no calibration view"));
        }
        selected
    };

    let glb = fs::read(&glb_path).map_err(|error| format!("cannot read {glb_path}: {error}"))?;
    let render_receipt = condition_render_asset(&glb, &render_request()).map_err(|error| error.to_string())?;
    if render_receipt.status != RenderPreparationStatus::Ready || !render_receipt.findings.is_empty() {
        return Err(format!("calibration GLB failed render conditioning: {:?}", render_receipt.findings));
    }
    let package = render_receipt.package.as_ref().expect("a ready receipt has a package");
    let runtime_receipt = prepare_asset(&glb, &runtime_request(&glb, package).map_err(|e| e.to_string())?)
        .map_err(|error| error.to_string())?;
    if runtime_receipt.status != PreparationStatus::Ready || !runtime_receipt.findings.is_empty() {
        return Err(format!("calibration GLB failed prepare: {:?}", runtime_receipt.findings));
    }
    let projection = project_render_asset(package).map_err(|error| error.to_string())?;
    validate_graphics_asset_projection(&projection).map_err(|error| error.to_string())?;

    let world = build_from_layout_path(&layout, &julia, &terrain_lab).map_err(|error| error.to_string())?;
    let reference = lower_reference_world(&world.world).map_err(|error| error.to_string())?;
    let base = lower_objective_close_packet(&reference).map_err(|error| error.to_string())?;
    let placement = CalibrationPlacement::for_world(&world.world).map_err(|error| error.to_string())?;
    let scene = wge_native_graphics_contract::calibration::calibration_scene(
        &world.world,
        &runtime_receipt,
        package,
        placement,
    )
    .map_err(|error| error.to_string())?;
    let card_local = grey_card_local_center(package).map_err(|error| error.to_string())?;

    let mut supervisor = GraphicsWorkerSupervisor::start(&julia, &graphics_project, &worker)
        .map_err(|error| error.to_string())?;
    supervisor.capabilities().map_err(|error| error.to_string())?;
    fs::create_dir_all(&output_dir)
        .map_err(|error| format!("cannot create {}: {error}", output_dir.display()))?;
    write_json_artifact(&output_dir.join("scene_artifact.json"), &scene)?;

    let mut renders = Vec::new();
    let mut gate_failures = Vec::new();
    for rig in &rigs {
        for view in &views {
            let started = Instant::now();
            let camera = view.camera(package, placement).map_err(|error| error.to_string())?;
            let mut body = base.body.clone();
            let packet = compose_bound_scene_with_view(
                &mut body,
                &scene,
                std::slice::from_ref(&projection),
                Some(&camera),
                Some(*rig),
            )
            .map_err(|error| format!("{} {} composition failed: {error}", rig.name(), view.name()))?;
            let promoted = supervisor
                .render_bound_scene_and_promote(
                    &packet,
                    &world.world,
                    BoundSceneRenderAuthorization {
                        base_packet: &base,
                        scene: &scene,
                        asset_receipts: std::slice::from_ref(&runtime_receipt),
                        render_packages: std::slice::from_ref(package),
                        assets: std::slice::from_ref(&projection),
                        camera: Some(&camera),
                        rig: Some(*rig),
                    },
                )
                .map_err(|error| format!("{} {} render failed: {error}", rig.name(), view.name()))?;
            let certification = deterministic_certification_frame_receipt(&packet, &promoted.receipt, &promoted.capture_bytes)
                .map_err(|error| error.to_string())?;
            let dir = output_dir.join(rig.name()).join(view.name());
            fs::create_dir_all(&dir).map_err(|error| format!("cannot create {}: {error}", dir.display()))?;
            let (width, height) = (promoted.frame.width_px, promoted.frame.height_px);
            let ppm = rgba8_to_ppm(&promoted.capture_bytes, width, height)?;
            fs::write(dir.join("native_capture.ppm"), &ppm).map_err(|error| error.to_string())?;
            fs::write(dir.join("native_capture.rgba"), &promoted.capture_bytes).map_err(|error| error.to_string())?;
            write_json_artifact(&dir.join("graphics_frame_receipt.json"), &certification)?;
            write_json_artifact(&dir.join("graphics_renderer_attestation.json"), &promoted.renderer_attestation)?;
            // The 18% card, 5x5 px mean of sRGB, in the two views composed to
            // frame it. Other views may have the distant card inside the
            // frustum but small or occluded, which is not a measurement.
            let frames_card = matches!(view, CalibrationView::Row) || *view == CalibrationView::Close("calibration".into());
            let grey_card = frames_card.then(|| project_local_point(&camera, placement, card_local)).flatten().map(|(px, py)| {
                let mut sum = 0.0f64;
                let mut count = 0.0f64;
                for y in py.saturating_sub(2)..=(py + 2).min(height - 1) {
                    for x in px.saturating_sub(2)..=(px + 2).min(width - 1) {
                        let i = ((y * width + x) * 4) as usize;
                        sum += promoted.capture_bytes[i..i + 3].iter().map(|v| f64::from(*v)).sum::<f64>() / 3.0;
                        count += 1.0;
                    }
                }
                let mean = sum / count;
                serde_json::json!({
                    "pixel": [px, py],
                    "srgb8_mean": mean,
                    "target_srgb8": GREY_CARD_TARGET_SRGB8,
                    "relative_error": (mean - f64::from(GREY_CARD_TARGET_SRGB8)) / f64::from(GREY_CARD_TARGET_SRGB8),
                })
            });
            // N-2 acceptance probes: the specimen spheres a close view frames.
            let probe_ids: Vec<String> = match view {
                CalibrationView::Close(column) if column == "calibration" => {
                    vec!["ball_chrome".into(), "ball_grey_018".into()]
                }
                CalibrationView::Close(column) => vec![format!("{column}_sphere")],
                _ => Vec::new(),
            };
            let mut spheres = serde_json::Map::new();
            for id in probe_ids {
                let Ok((center, radius)) = local_sphere(package, &id) else { continue };
                if let Some((cx, cy, r)) = project_local_sphere(&camera, placement, center, radius)
                    && let Some(stats) = sphere_probe(&promoted.capture_bytes, width, height, cx, cy, r * 0.85)
                {
                    spheres.insert(id, stats);
                }
            }
            let entry = serde_json::json!({
                "rig": rig.name(),
                "view": view.name(),
                "spheres": spheres,
                "packet_sha256": packet.packet_sha256,
                "packet_bytes": serde_json::to_vec(&packet).map(|bytes| bytes.len()).unwrap_or(0),
                "capture_sha256": sha256_prefixed(&promoted.capture_bytes),
                "frame_receipt_sha256": certification.receipt_sha256,
                "grey_card": grey_card,
                "wall_time_ms": started.elapsed().as_millis() as u64,
            });
            eprintln!("calibration {} {} {}", rig.name(), view.name(), entry["capture_sha256"]);
            // Contract §3 gate: the 18% card within ±5% of middle grey under
            // the rigs whose exposure is calibrated on it (sun and overcast,
            // with and without IBL).
            if rig.exposure_calibrated()
                && let Some(error) = entry["grey_card"]["relative_error"].as_f64()
                && error.abs() > 0.05
            {
                gate_failures.push(format!("{}: grey card {:+.1}% from target", view.name(), error * 100.0));
            }
            renders.push(entry);
        }
    }
    let summary = serde_json::json!({
        "schema_version": "wge.native-graphics-calibration/v1",
        "contract": "WGE_CONVERGE1_CONTRACTS.md §3 CALIBRATION-1",
        "world_artifact_id": world.world.artifact_id,
        "glb_sha256": sha256_prefixed(&glb),
        "runtime_receipt_sha256": runtime_receipt.receipt_sha256,
        "render_receipt_sha256": render_receipt.receipt_sha256,
        "scene_artifact_sha256": scene.artifact_sha256,
        "placement_anchor_xyz_m": placement.anchor_xyz_m,
        "renders": renders,
        "gate_failures": gate_failures,
    });
    write_json_artifact(&output_dir.join("calibration_summary.json"), &summary)?;
    println!("{}", serde_json::to_string_pretty(&summary).map_err(|error| error.to_string())?);
    if !gate_failures.is_empty() {
        return Err(format!("calibration grey-card gate failed: {gate_failures:?}"));
    }
    Ok(())
}

fn write_json_artifact<T: Serialize>(path: &PathBuf, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("cannot serialize {}: {error}", path.display()))?;
    fs::write(path, bytes).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn measure_frame(
    supervisor: &mut GraphicsWorkerSupervisor,
    packet: &wge_native_graphics_contract::GraphicsScenePacket,
    world: &wge_reference_runtime::WorldArtifact,
) -> Result<(BenchmarkSample, String), String> {
    let started = Instant::now();
    let promoted = supervisor
        .render_and_promote(packet, world)
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

/// Linear-light statistics of the capture pixels inside a screen circle:
/// mean RGB and its chromaticity, median and 99th-percentile luminance, and
/// the upper / lower half means (a chrome ball reflects sky above, ground below).
fn sphere_probe(capture: &[u8], width: u32, height: u32, cx: f32, cy: f32, r: f32) -> Option<serde_json::Value> {
    let decode = |v: u8| {
        let c = f64::from(v) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let (mut sum, mut upper, mut lower) = ([0.0f64; 3], [0.0f64; 4], [0.0f64; 4]);
    let mut lums = Vec::new();
    let (x0, x1) = ((cx - r).floor().max(0.0) as u32, (cx + r).ceil().min(width as f32 - 1.0) as u32);
    let (y0, y1) = ((cy - r).floor().max(0.0) as u32, (cy + r).ceil().min(height as f32 - 1.0) as u32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            if dx * dx + dy * dy > r * r {
                continue;
            }
            let i = ((y * width + x) * 4) as usize;
            let rgb = [decode(capture[i]), decode(capture[i + 1]), decode(capture[i + 2])];
            for k in 0..3 {
                sum[k] += rgb[k];
            }
            let half = if dy < 0.0 { &mut upper } else { &mut lower };
            for k in 0..3 {
                half[k] += rgb[k];
            }
            half[3] += 1.0;
            lums.push(0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]);
        }
    }
    if lums.len() < 16 {
        return None;
    }
    let n = lums.len() as f64;
    let mean = sum.map(|v| v / n);
    let total = (mean[0] + mean[1] + mean[2]).max(1e-12);
    lums.sort_by(|a, b| a.total_cmp(b));
    let pct = |p: f64| lums[((lums.len() - 1) as f64 * p).round() as usize];
    let half_mean = |h: [f64; 4]| [0, 1, 2].map(|k| h[k] / h[3].max(1.0));
    Some(serde_json::json!({
        "pixels": lums.len(),
        "mean_linear_rgb": mean,
        "chromaticity_rg": [mean[0] / total, mean[1] / total],
        "luminance_median": pct(0.5),
        "luminance_p99": pct(0.99),
        "upper_mean_linear_rgb": half_mean(upper),
        "lower_mean_linear_rgb": half_mean(lower),
    }))
}
