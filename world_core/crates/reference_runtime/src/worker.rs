use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::fields::{JuliaFieldRequest, fields_from_response, prefixed_sha256};
use crate::{
    AuthoredLayout, JuliaFieldProvenance, JuliaFieldResponse, NumericalFields,
    ReferenceRuntimeError, WorldBuild, build_world, render_reference_capture, run_playthrough,
    validate_layout,
};

const WORLD_FIELDS_WORKER_ID: &str = "terrain_lab/bin/luxel_reference_world_fields.jl";
const MAX_PROTOCOL_BYTES: usize = 16 * 1024 * 1024;

pub fn julia_field_request(
    layout: &AuthoredLayout,
    layout_sha256: &str,
) -> Result<Vec<u8>, ReferenceRuntimeError> {
    validate_layout(layout)?;
    if !crate::fields::is_sha256(layout_sha256) {
        return Err(ReferenceRuntimeError::provenance(
            "layout identity must be a prefixed SHA-256".into(),
        ));
    }
    let request = JuliaFieldRequest::new(layout, layout_sha256);
    serde_json::to_vec(&request).map_err(|error| {
        ReferenceRuntimeError::contract(format!("request encoding failed: {error}"))
    })
}

pub fn run_julia_field_worker(
    layout: &AuthoredLayout,
    layout_sha256: &str,
    julia_executable: &Path,
    terrain_lab: &Path,
) -> Result<(NumericalFields, JuliaFieldProvenance), ReferenceRuntimeError> {
    let worker_path = terrain_lab.join("bin/luxel_reference_world_fields.jl");
    let project_path = terrain_lab.join("Project.toml");
    let manifest_path = terrain_lab.join("Manifest.toml");
    let script_bytes = fs::read(&worker_path).map_err(|error| {
        ReferenceRuntimeError::worker(format!(
            "could not read Julia worker {}: {error}",
            worker_path.display()
        ))
    })?;
    let script_sha256 = prefixed_sha256(&script_bytes);
    let project_sha256 = terrain_project_digest(&project_path, &manifest_path)?;
    let julia_version = julia_version(julia_executable)?;
    let request_json = julia_field_request(layout, layout_sha256)?;
    if request_json.len() > MAX_PROTOCOL_BYTES {
        return Err(ReferenceRuntimeError::contract(
            "Julia request exceeds the protocol size limit".into(),
        ));
    }
    let request_sha256 = prefixed_sha256(&request_json);

    let mut command = Command::new(julia_executable);
    command
        .arg(format!("--project={}", terrain_lab.display()))
        .arg("--startup-file=no")
        .arg(&worker_path)
        .current_dir(terrain_lab)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| {
        ReferenceRuntimeError::worker(format!(
            "could not launch Julia worker {}: {error}",
            julia_executable.display()
        ))
    })?;
    {
        let stdin = child.stdin.as_mut().ok_or_else(|| {
            ReferenceRuntimeError::worker("Julia worker stdin was not piped".into())
        })?;
        stdin.write_all(&request_json).map_err(|error| {
            ReferenceRuntimeError::worker(format!("could not send Julia request: {error}"))
        })?;
        stdin.write_all(b"\n").map_err(|error| {
            ReferenceRuntimeError::worker(format!(
                "could not terminate Julia request frame: {error}"
            ))
        })?;
    }
    let output = child.wait_with_output().map_err(|error| {
        ReferenceRuntimeError::worker(format!("could not collect Julia worker output: {error}"))
    })?;
    if output.stdout.len() > MAX_PROTOCOL_BYTES || output.stderr.len() > MAX_PROTOCOL_BYTES {
        return Err(ReferenceRuntimeError::worker(
            "Julia worker exceeded the protocol output size limit".into(),
        ));
    }
    if !output.status.success() {
        return Err(ReferenceRuntimeError::worker(format!(
            "Julia worker exited with {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let response_bytes = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
    if response_bytes.is_empty()
        || response_bytes.contains(&b'\n')
        || response_bytes.contains(&b'\r')
    {
        return Err(ReferenceRuntimeError::worker(
            "Julia worker must return exactly one JSON frame".into(),
        ));
    }
    let response_json = std::str::from_utf8(response_bytes)
        .map_err(|error| {
            ReferenceRuntimeError::worker(format!("worker response is not UTF-8: {error}"))
        })?
        .to_owned();
    let response: JuliaFieldResponse = serde_json::from_slice(response_bytes).map_err(|error| {
        ReferenceRuntimeError::worker(format!(
            "worker response violates its typed schema: {error}"
        ))
    })?;
    let response_sha256 = prefixed_sha256(response_bytes);
    let request: JuliaFieldRequest = serde_json::from_slice(&request_json).map_err(|error| {
        ReferenceRuntimeError::contract(format!("internally generated request is invalid: {error}"))
    })?;
    let fields = fields_from_response(&request, &request_sha256, &response)?;

    // Pin the executable inputs across process startup and execution.
    if prefixed_sha256(&fs::read(&worker_path).map_err(|error| {
        ReferenceRuntimeError::provenance(format!("Julia worker changed or disappeared: {error}"))
    })?) != script_sha256
        || terrain_project_digest(&project_path, &manifest_path)? != project_sha256
    {
        return Err(ReferenceRuntimeError::provenance(
            "Julia worker or terrain project changed during field generation".into(),
        ));
    }

    let provenance = JuliaFieldProvenance {
        schema_version: "luxel.julia-world-fields-provenance/v1".into(),
        worker_id: WORLD_FIELDS_WORKER_ID.into(),
        request_sha256,
        response_sha256,
        worker_script_sha256: script_sha256,
        terrain_project_sha256: project_sha256,
        julia_version,
        request_json: String::from_utf8(request_json).map_err(|error| {
            ReferenceRuntimeError::contract(format!("request encoding was not UTF-8: {error}"))
        })?,
        response_json,
    };
    Ok((fields, provenance))
}

pub fn build_from_layout_path(
    layout_path: &Path,
    julia_executable: &Path,
    terrain_lab: &Path,
) -> Result<WorldBuild, ReferenceRuntimeError> {
    let input = fs::read(layout_path).map_err(|error| {
        ReferenceRuntimeError::contract(format!(
            "could not read authored layout {}: {error}",
            layout_path.display()
        ))
    })?;
    let layout: AuthoredLayout = serde_json::from_slice(&input).map_err(|error| {
        ReferenceRuntimeError::contract(format!(
            "authored layout violates its typed schema: {error}"
        ))
    })?;
    validate_layout(&layout)?;
    let canonical_layout = serde_json::to_vec(&layout).map_err(|error| {
        ReferenceRuntimeError::contract(format!("layout canonicalization failed: {error}"))
    })?;
    let layout_sha256 = prefixed_sha256(&canonical_layout);
    let (fields, provenance) =
        run_julia_field_worker(&layout, &layout_sha256, julia_executable, terrain_lab)?;
    let world = build_world(layout, layout_sha256, fields, provenance)?;
    let traversal = run_playthrough(&world)?;
    let (capture_bytes, visual) = render_reference_capture(&world)?;
    let gameplay_kit =
        luxel_gameplay_contract::reference_vertical_slice_kit().map_err(|diagnostics| {
            ReferenceRuntimeError::contract(format!(
                "reference gameplay kit resolution failed: {diagnostics:?}"
            ))
        })?;
    crate::validate_gameplay_kit(&gameplay_kit)?;
    let gameplay =
        crate::build_gameplay_world_binding(&world, &traversal, &capture_bytes, &visual)?;
    crate::validate_gameplay_world_binding(&world, &traversal, &capture_bytes, &visual, &gameplay)?;
    Ok(WorldBuild {
        world,
        traversal,
        capture_bytes,
        visual,
        gameplay,
        gameplay_kit,
    })
}

fn julia_version(executable: &Path) -> Result<String, ReferenceRuntimeError> {
    let output = Command::new(executable)
        .arg("--version")
        .output()
        .map_err(|error| {
            ReferenceRuntimeError::worker(format!("could not query Julia version: {error}"))
        })?;
    if !output.status.success() {
        return Err(ReferenceRuntimeError::worker(format!(
            "Julia --version exited with {}; stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !version.starts_with("julia version ") {
        return Err(ReferenceRuntimeError::provenance(
            "Julia executable returned an unrecognized version string".into(),
        ));
    }
    Ok(version)
}

fn terrain_project_digest(
    project_path: &Path,
    manifest_path: &Path,
) -> Result<String, ReferenceRuntimeError> {
    let mut bytes = fs::read(project_path).map_err(|error| {
        ReferenceRuntimeError::provenance(format!(
            "could not read terrain project {}: {error}",
            project_path.display()
        ))
    })?;
    if manifest_path.exists() {
        bytes.extend_from_slice(b"\0Manifest.toml\0");
        bytes.extend(fs::read(manifest_path).map_err(|error| {
            ReferenceRuntimeError::provenance(format!(
                "could not read terrain manifest {}: {error}",
                manifest_path.display()
            ))
        })?);
    }
    Ok(prefixed_sha256(&bytes))
}
