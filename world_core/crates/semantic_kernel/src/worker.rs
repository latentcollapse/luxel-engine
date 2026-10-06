//! Rust-supervised Julia workers.
//!
//! The terrain worker remains the numerical owner. Rust owns process lifetime,
//! framing, typed request/receipt checks, and provenance.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::{KernelFailure, canonical_json, sha256_hex, sha256_prefixed};
use serde_json::{Map, Value, json};

pub const MAX_FRAME_BYTES: usize = 1_000_000;
const STDERR_TAIL_BYTES: usize = 8_192;
const EROSION_REQUEST_SCHEMA: &str = "codeweald.erosion-request/v1";
const EROSION_RESULT_SCHEMA: &str = "codeweald.erosion-result/v1";
pub const EROSION_WRAPPER_VERSION: &str = "luxel.erosion-worker/v1";

static TEMP_SCRIPT_COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct Worker {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: Arc<Mutex<Vec<u8>>>,
    temporary_script: Option<TemporaryScript>,
    pub pid: u32,
    pub script_sha256: String,
    pub project_sha256: String,
    pub manifest_sha256: Option<String>,
    pub solver_image: Option<String>,
    pub cold_ms: u128,
}

impl Worker {
    /// Launch a Julia process using its native framed worker script.
    pub fn spawn(julia: &Path, project: &Path, script: &Path) -> Result<Self, KernelFailure> {
        let expected_script = script_digest(script)?;
        let project_file = project.join("Project.toml");
        let project_sha256 = file_digest(&project_file, "worker_unavailable")?;
        Self::spawn_framed(
            julia,
            project,
            script,
            &expected_script,
            &project_sha256,
            None,
            None,
            None,
            false,
        )
    }

    /// Launch the existing one-shot erosion entry point as a warm framed worker.
    /// Its request handler and numerical implementation are reused verbatim;
    /// only its terminal `exit(main())` is omitted from the temporary driver.
    pub fn spawn_erosion(
        julia: &Path,
        project: &Path,
        manifest: &Path,
        script: &Path,
    ) -> Result<Self, KernelFailure> {
        let script_sha256 = script_digest(script)?;
        let project_file = project.join("Project.toml");
        let project_sha256 = project_digest(&project_file, manifest)?;
        let manifest_sha256 = file_digest(manifest, "solver_image_unreadable")?;
        let version = julia_version(julia)?;
        let solver_image = erosion_solver_image(&script_sha256, &project_sha256, &version);
        let temporary_script = make_erosion_driver(&script_sha256, &project_sha256, script)?;
        let command_script = temporary_script.path.clone();
        let worker = Self::spawn_framed(
            julia,
            project,
            &command_script,
            &script_sha256,
            &project_sha256,
            Some(manifest_sha256),
            Some(solver_image),
            Some(temporary_script),
            true,
        )?;
        // spawn_framed validates the original script and project identities
        // announced by the generated driver. Recheck their bytes after startup
        // to fail closed if an input changed during process initialization.
        if script_digest(script)? != worker.script_sha256
            || project_digest(&project_file, manifest)? != worker.project_sha256
        {
            return Err(KernelFailure::provenance(
                "worker_inputs_changed",
                "erosion worker inputs changed while the Julia process was starting",
            ));
        }
        Ok(worker)
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_framed(
        julia: &Path,
        project: &Path,
        command_script: &Path,
        expected_script: &str,
        expected_project: &str,
        manifest_sha256: Option<String>,
        solver_image: Option<String>,
        temporary_script: Option<TemporaryScript>,
        require_project_digest: bool,
    ) -> Result<Self, KernelFailure> {
        let mut command = Command::new(julia);
        command
            .arg(format!("--project={}", project.display()))
            .arg("--startup-file=no")
            .arg(command_script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let started = Instant::now();
        let mut child = command.spawn().map_err(|error| {
            KernelFailure::operator(
                "worker_unavailable",
                format!("failed to spawn {}: {error}", julia.display()),
            )
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            KernelFailure::operator("worker_unavailable", "Julia stdin was not piped")
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            KernelFailure::operator("worker_unavailable", "Julia stdout was not piped")
        })?;
        let stderr_pipe = child.stderr.take().ok_or_else(|| {
            KernelFailure::operator("worker_unavailable", "Julia stderr was not piped")
        })?;
        let stderr = capture_stderr(stderr_pipe);
        let pid = child.id();
        let mut worker = Self {
            child,
            stdin,
            stdout,
            stderr,
            temporary_script,
            pid,
            script_sha256: String::new(),
            project_sha256: String::new(),
            manifest_sha256,
            solver_image,
            cold_ms: 0,
        };
        let ready = worker.read_frame()?;
        worker.cold_ms = started.elapsed().as_millis();
        let (reported_script, reported_project) = parse_ready(&ready)?;
        let project_mismatch = if require_project_digest {
            reported_project != expected_project
        } else {
            !reported_project.is_empty() && reported_project != expected_project
        };
        if reported_script != expected_script || project_mismatch {
            return Err(KernelFailure::provenance(
                "solver_image_mismatch",
                format!(
                    "Julia worker announced script/project {reported_script}/{reported_project}, expected {expected_script}/{expected_project}; stderr: {}",
                    worker.stderr_excerpt()
                ),
            ));
        }
        worker.script_sha256 = reported_script;
        worker.project_sha256 = if reported_project.is_empty() {
            expected_project.to_owned()
        } else {
            reported_project
        };
        Ok(worker)
    }

    /// Send one JSON object and return its JSON object response plus warm time.
    pub fn request(&mut self, payload: &[u8]) -> Result<(Vec<u8>, u128), KernelFailure> {
        validate_json_object(payload, "malformed_job")?;
        self.ensure_running()?;
        let started = Instant::now();
        if let Err(failure) = write_frame(&mut self.stdin, payload) {
            self.stop_child();
            return Err(failure);
        }
        let response = match self.read_frame() {
            Ok(response) => response,
            Err(failure) => {
                self.stop_child();
                return Err(failure);
            }
        };
        if let Err(failure) = validate_json_object(&response, "malformed_protocol") {
            self.stop_child();
            return Err(failure);
        }
        Ok((response, started.elapsed().as_micros()))
    }

    /// Send and validate one typed erosion request and its Julia receipt.
    pub fn request_erosion(&mut self, payload: &[u8]) -> Result<(Value, u128), KernelFailure> {
        let request = validate_json_object(payload, "malformed_request")?;
        validate_erosion_request(&request)?;
        let (raw, elapsed_us) = self.request(payload)?;
        let response = validate_json_object(&raw, "malformed_protocol")?;
        validate_erosion_response(&request, &response)?;
        Ok((response, elapsed_us))
    }

    fn read_frame(&mut self) -> Result<Vec<u8>, KernelFailure> {
        match read_frame(&mut self.stdout) {
            Ok(frame) => Ok(frame),
            Err(failure) if failure.code() == "worker_exited" => Err(KernelFailure::operator(
                "worker_exited",
                format!(
                    "Julia worker {} exited before returning a frame; {}",
                    self.pid,
                    self.stderr_excerpt()
                ),
            )),
            Err(failure) => Err(failure),
        }
    }

    fn ensure_running(&mut self) -> Result<(), KernelFailure> {
        match self.child.try_wait() {
            Ok(Some(status)) => Err(KernelFailure::operator(
                "worker_exited",
                format!(
                    "Julia worker {} exited with {status}; {}",
                    self.pid,
                    self.stderr_excerpt()
                ),
            )),
            Ok(None) => Ok(()),
            Err(error) => Err(KernelFailure::operator("worker_io", error.to_string())),
        }
    }

    fn stop_child(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    pub fn stderr_excerpt(&self) -> String {
        let guard = match self.stderr.lock() {
            Ok(guard) => guard,
            Err(_) => return "stderr capture lock poisoned".to_owned(),
        };
        String::from_utf8_lossy(&guard).into_owned()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop_child();
        // Drop the temporary script only after the Julia process has stopped.
        let _ = self.temporary_script.take();
    }
}

pub struct ErosionSupervisorOptions {
    pub julia: PathBuf,
    pub project: PathBuf,
    pub manifest: PathBuf,
    pub worker_script: PathBuf,
}

/// Process a batch of newline-delimited compatibility requests with one warm
/// Julia process. Rust validates both request and response envelopes and mints
/// the transport receipt; Julia remains the numerical owner.
pub fn run_erosion_supervisor<R: BufRead>(
    options: &ErosionSupervisorOptions,
    input: &mut R,
) -> Result<(), KernelFailure> {
    let mut job_index = 0_u64;
    let mut worker: Option<Worker> = None;
    loop {
        let mut line = Vec::new();
        if !read_bounded_line(input, &mut line)? {
            break;
        }
        if line.last() == Some(&b'\n') {
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
        }
        if line.is_empty() {
            return Err(KernelFailure::operator(
                "malformed_request",
                "empty lines are not erosion requests",
            ));
        }
        let request = validate_json_object(&line, "malformed_request")?;
        validate_erosion_request(&request)?;

        if worker.is_none() {
            let spawned = Worker::spawn_erosion(
                &options.julia,
                &options.project,
                &options.manifest,
                &options.worker_script,
            )?;
            println!(
                "{}",
                canonical_json(&json!({
                    "cold_ms": spawned.cold_ms,
                    "event": "worker_ready",
                    "manifest_sha256": spawned.manifest_sha256,
                    "pid": spawned.pid,
                    "project_sha256": spawned.project_sha256,
                    "script_sha256": spawned.script_sha256,
                    "solver_image": spawned.solver_image,
                    "wrapper_version": EROSION_WRAPPER_VERSION
                }))
            );
            worker = Some(spawned);
        }

        let encoded = canonical_json(&request).into_bytes();
        let (response, job_us) = worker
            .as_mut()
            .expect("validated request starts the worker")
            .request_erosion(&encoded)?;
        job_index += 1;
        let current = worker.as_ref().expect("worker is live after request");
        let request_sha256 = sha256_prefixed(canonical_json(&request).as_bytes());
        let response_sha256 = sha256_prefixed(canonical_json(&response).as_bytes());
        let result_sha256 = response
            .get("result_sha256")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let receipt = json!({
            "job_index": job_index,
            "operation": request["operation"],
            "project_sha256": current.project_sha256,
            "request_sha256": request_sha256,
            "response_sha256": response_sha256,
            "result_sha256": result_sha256,
            "schema": "luxel.erosion-worker-receipt/v1",
            "script_sha256": current.script_sha256,
            "solver_image": current.solver_image,
            "worker_pid": current.pid
        });
        let receipt_sha256 = sha256_prefixed(canonical_json(&receipt).as_bytes());
        println!(
            "{}",
            canonical_json(&json!({
                "event": "worker_result",
                "job_index": job_index,
                "job_us": job_us,
                "phase": if job_index == 1 { "first_job" } else { "warm" },
                "pid": current.pid,
                "receipt": receipt,
                "receipt_sha256": receipt_sha256,
                "response": response
            }))
        );
    }
    if job_index == 0 {
        return Err(KernelFailure::operator(
            "malformed_request",
            "erosion supervisor received no requests",
        ));
    }
    Ok(())
}

fn read_bounded_line(input: &mut impl BufRead, line: &mut Vec<u8>) -> Result<bool, KernelFailure> {
    loop {
        let available = input
            .fill_buf()
            .map_err(|error| KernelFailure::operator("unreadable_input", error.to_string()))?;
        if available.is_empty() {
            return Ok(!line.is_empty());
        }
        let count = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|index| index + 1)
            .unwrap_or(available.len());
        if line.len().saturating_add(count) > MAX_FRAME_BYTES + 1 {
            return Err(KernelFailure::operator(
                "malformed_request",
                format!("request line exceeds {MAX_FRAME_BYTES} bytes"),
            ));
        }
        let complete = available.get(count - 1) == Some(&b'\n');
        line.extend_from_slice(&available[..count]);
        input.consume(count);
        if complete {
            return Ok(true);
        }
    }
}

fn validate_erosion_request(value: &Value) -> Result<(), KernelFailure> {
    let object = as_object(value, "malformed_request", "request")?;
    let operation = required_string(object, "operation", "malformed_request")?;
    let mut expected = BTreeSet::from([
        "schema",
        "operation",
        "shape",
        "height_path",
        "height_sha256",
    ]);
    let operation_fields: &[&str] = match operation {
        "erode" => &[
            "output_path",
            "cell_m",
            "protect_path",
            "protect_sha256",
            "profile",
        ],
        "thermal_erosion" => &["output_path", "cell_m", "talus_degrees", "rate"],
        "flux_field" => &["output_path", "source_path", "source_sha256", "exponent"],
        "valley_cross_section" => &["row_index"],
        _ => {
            return Err(KernelFailure::operator(
                "unsupported_operation",
                format!("erosion operation {operation:?} is not supported"),
            ));
        }
    };
    expected.extend(operation_fields.iter().copied());
    require_exact_keys(object, &expected, "malformed_request", "request")?;
    if required_string(object, "schema", "malformed_request")? != EROSION_REQUEST_SCHEMA {
        return Err(KernelFailure::operator(
            "unsupported_schema",
            "request schema is not codeweald.erosion-request/v1",
        ));
    }
    let shape = object["shape"].as_array().ok_or_else(|| {
        KernelFailure::operator("invalid_shape", "shape must contain exactly two dimensions")
    })?;
    if shape.len() != 2
        || shape
            .iter()
            .any(|dimension| dimension.as_u64().is_none_or(|value| value == 0))
    {
        return Err(KernelFailure::operator(
            "invalid_shape",
            "shape must contain two positive integer dimensions",
        ));
    }
    required_string(object, "height_path", "malformed_request")?;
    require_digest(object, "height_sha256")?;

    match operation {
        "erode" => {
            required_string(object, "output_path", "malformed_request")?;
            require_finite_number(object, "cell_m")?;
            validate_optional_path_digest(object, "protect_path", "protect_sha256")?;
            validate_profile(object.get("profile").ok_or_else(|| {
                KernelFailure::operator("malformed_request", "profile is required")
            })?)?;
        }
        "thermal_erosion" => {
            required_string(object, "output_path", "malformed_request")?;
            require_finite_number(object, "cell_m")?;
            require_finite_number(object, "talus_degrees")?;
            require_finite_number(object, "rate")?;
        }
        "flux_field" => {
            required_string(object, "output_path", "malformed_request")?;
            validate_optional_path_digest(object, "source_path", "source_sha256")?;
            require_finite_number(object, "exponent")?;
        }
        "valley_cross_section" => {
            let row = object["row_index"].as_u64().ok_or_else(|| {
                KernelFailure::operator(
                    "malformed_request",
                    "row_index must be a non-negative integer",
                )
            })?;
            let rows = shape[0].as_u64().expect("shape was validated");
            if row >= rows {
                return Err(KernelFailure::operator(
                    "invalid_row",
                    "row_index is outside the heightfield",
                ));
            }
        }
        _ => unreachable!("operation validated above"),
    }
    Ok(())
}

fn validate_profile(value: &Value) -> Result<(), KernelFailure> {
    let object = as_object(value, "malformed_request", "profile")?;
    let keys = BTreeSet::from([
        "key",
        "iterations",
        "stream_power",
        "glacial_strength",
        "ice_line",
        "lateral_widening",
        "talus_degrees",
        "cirque_strength",
        "planation",
    ]);
    require_exact_keys(object, &keys, "malformed_request", "profile")?;
    let key = required_string(object, "key", "malformed_request")?;
    if key.is_empty() {
        return Err(KernelFailure::operator(
            "malformed_request",
            "profile.key cannot be empty",
        ));
    }
    if object["iterations"].as_u64().is_none() {
        return Err(KernelFailure::operator(
            "malformed_request",
            "profile.iterations must be a non-negative integer",
        ));
    }
    for name in [
        "stream_power",
        "glacial_strength",
        "ice_line",
        "lateral_widening",
        "talus_degrees",
        "cirque_strength",
        "planation",
    ] {
        require_finite_number(object, name)?;
    }
    Ok(())
}

fn validate_optional_path_digest(
    object: &Map<String, Value>,
    path_key: &str,
    digest_key: &str,
) -> Result<(), KernelFailure> {
    let path = object.get(path_key).ok_or_else(|| {
        KernelFailure::operator("malformed_request", format!("{path_key} is required"))
    })?;
    let digest = object.get(digest_key).ok_or_else(|| {
        KernelFailure::operator("malformed_request", format!("{digest_key} is required"))
    })?;
    match (path, digest) {
        (Value::Null, Value::Null) => Ok(()),
        (Value::String(path), Value::String(digest)) if !path.is_empty() && is_sha256(digest) => {
            Ok(())
        }
        _ => Err(KernelFailure::operator(
            "malformed_request",
            format!("{path_key} and {digest_key} must both be null or a path and SHA-256"),
        )),
    }
}

fn validate_erosion_response(request: &Value, response: &Value) -> Result<(), KernelFailure> {
    let object = as_object(response, "malformed_protocol", "worker response")?;
    if object.get("status").and_then(Value::as_str) == Some("error") {
        let keys = BTreeSet::from(["schema", "status", "error"]);
        require_exact_keys(object, &keys, "malformed_protocol", "worker error response")?;
        if object.get("schema").and_then(Value::as_str) != Some(EROSION_RESULT_SCHEMA) {
            return Err(KernelFailure::operator(
                "malformed_protocol",
                "worker error schema is invalid",
            ));
        }
        let error = as_object(&object["error"], "malformed_protocol", "worker error")?;
        require_exact_keys(
            error,
            &BTreeSet::from(["code", "message"]),
            "malformed_protocol",
            "worker error",
        )?;
        return Err(KernelFailure::operator(
            required_string(error, "code", "malformed_protocol")?,
            required_string(error, "message", "malformed_protocol")?,
        ));
    }

    let operation = request["operation"].as_str().expect("request validated");
    let extra: &[&str] = match operation {
        "erode" => &["dtype", "result_sha256", "report"],
        "thermal_erosion" | "flux_field" => &["dtype", "result_sha256"],
        "valley_cross_section" => &["measurement"],
        _ => unreachable!("request operation validated"),
    };
    let mut expected = BTreeSet::from(["schema", "status", "operation", "shape"]);
    expected.extend(extra.iter().copied());
    require_exact_keys(object, &expected, "malformed_protocol", "worker response")?;
    if object.get("schema").and_then(Value::as_str) != Some(EROSION_RESULT_SCHEMA)
        || object.get("status").and_then(Value::as_str) != Some("ok")
        || object.get("operation").and_then(Value::as_str) != Some(operation)
    {
        return Err(KernelFailure::operator(
            "malformed_protocol",
            "worker response schema, status, or operation does not match the request",
        ));
    }
    let request_shape = request["shape"]
        .as_array()
        .expect("request shape validated");
    if object.get("shape") != Some(&Value::Array(request_shape.clone())) {
        return Err(KernelFailure::operator(
            "malformed_protocol",
            "worker response shape does not match the request",
        ));
    }
    if operation == "valley_cross_section" {
        let measurement = as_object(&object["measurement"], "malformed_protocol", "measurement")?;
        require_exact_keys(
            measurement,
            &BTreeSet::from(["form_ratio", "shape", "relief_m"]),
            "malformed_protocol",
            "measurement",
        )?;
        let ratio = finite_number(&measurement["form_ratio"], "form_ratio")?;
        let relief = finite_number(&measurement["relief_m"], "relief_m")?;
        let shape = required_string(measurement, "shape", "malformed_protocol")?;
        if ratio < 0.0 || relief < 0.0 || !["u", "v", "flat"].contains(&shape) {
            return Err(KernelFailure::operator(
                "malformed_protocol",
                "valley measurement is outside its typed domain",
            ));
        }
        return Ok(());
    }

    if object.get("dtype").and_then(Value::as_str) != Some("float64-le") {
        return Err(KernelFailure::operator(
            "malformed_protocol",
            "worker output dtype must be float64-le",
        ));
    }
    let result_sha = required_string(object, "result_sha256", "malformed_protocol")?;
    if !is_sha256(result_sha) {
        return Err(KernelFailure::operator(
            "malformed_protocol",
            "worker result digest is malformed",
        ));
    }
    let output_path = request["output_path"]
        .as_str()
        .expect("request output path validated");
    let bytes = fs::read(output_path).map_err(|error| {
        KernelFailure::operator(
            "worker_result_unreadable",
            format!("worker output {} could not be read: {error}", output_path),
        )
    })?;
    let expected_bytes = request_shape
        .iter()
        .try_fold(8_usize, |product, dimension| {
            let dimension = dimension.as_u64()? as usize;
            product.checked_mul(dimension)
        })
        .ok_or_else(|| KernelFailure::operator("invalid_shape", "output dimensions overflow"))?;
    if bytes.len() != expected_bytes || sha256_hex(&bytes) != result_sha {
        return Err(KernelFailure::provenance(
            "result_digest_mismatch",
            "worker output length or digest does not match its receipt",
        ));
    }
    if operation == "erode" {
        validate_erosion_report(&object["report"])?;
    }
    Ok(())
}

fn validate_erosion_report(value: &Value) -> Result<(), KernelFailure> {
    let report = as_object(value, "malformed_protocol", "report")?;
    let keys = BTreeSet::from([
        "profile",
        "iterations",
        "ice_line_m",
        "lateral_widening_cells",
        "mean_lowering_m",
        "maximum_lowering_m",
        "maximum_raising_m",
        "glacial_share",
        "relief_before_m",
        "relief_after_m",
    ]);
    require_exact_keys(report, &keys, "malformed_protocol", "report")?;
    required_string(report, "profile", "malformed_protocol")?;
    if report["iterations"].as_u64().is_none() {
        return Err(KernelFailure::operator(
            "malformed_protocol",
            "report.iterations must be an integer",
        ));
    }
    for name in [
        "ice_line_m",
        "lateral_widening_cells",
        "mean_lowering_m",
        "maximum_lowering_m",
        "maximum_raising_m",
        "glacial_share",
        "relief_before_m",
        "relief_after_m",
    ] {
        finite_number(&report[name], name)?;
    }
    Ok(())
}

fn validate_json_object(bytes: &[u8], code: &str) -> Result<Value, KernelFailure> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME_BYTES {
        return Err(KernelFailure::operator(
            code,
            "JSON frame is empty or exceeds the 1MB limit",
        ));
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|error| {
        KernelFailure::operator(code, format!("frame is not valid JSON: {error}"))
    })?;
    if !value.is_object() {
        return Err(KernelFailure::operator(
            code,
            "frame must contain a JSON object",
        ));
    }
    Ok(value)
}

fn as_object<'a>(
    value: &'a Value,
    code: &str,
    label: &str,
) -> Result<&'a Map<String, Value>, KernelFailure> {
    value
        .as_object()
        .ok_or_else(|| KernelFailure::operator(code, format!("{label} must be an object")))
}

fn require_exact_keys(
    object: &Map<String, Value>,
    expected: &BTreeSet<&str>,
    code: &str,
    label: &str,
) -> Result<(), KernelFailure> {
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    if &actual != expected {
        return Err(KernelFailure::operator(
            code,
            format!("{label} fields do not match the closed schema"),
        ));
    }
    Ok(())
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    code: &str,
) -> Result<&'a str, KernelFailure> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| KernelFailure::operator(code, format!("{key} must be a string")))
}

fn require_digest(object: &Map<String, Value>, key: &str) -> Result<(), KernelFailure> {
    let digest = required_string(object, key, "malformed_request")?;
    if is_sha256(digest) {
        Ok(())
    } else {
        Err(KernelFailure::operator(
            "malformed_request",
            format!("{key} must be a lowercase SHA-256 digest"),
        ))
    }
}

fn require_finite_number(object: &Map<String, Value>, key: &str) -> Result<f64, KernelFailure> {
    let value = object.get(key).ok_or_else(|| {
        KernelFailure::operator("malformed_request", format!("{key} is required"))
    })?;
    finite_number(value, key)
}

fn finite_number(value: &Value, label: &str) -> Result<f64, KernelFailure> {
    let number = value.as_f64().ok_or_else(|| {
        KernelFailure::operator("malformed_request", format!("{label} must be numeric"))
    })?;
    if number.is_finite() {
        Ok(number)
    } else {
        Err(KernelFailure::operator(
            "malformed_request",
            format!("{label} must be finite"),
        ))
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn write_frame(writer: &mut impl Write, payload: &[u8]) -> Result<(), KernelFailure> {
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err(KernelFailure::operator(
            "malformed_protocol",
            "frame is empty or exceeds the 1MB limit",
        ));
    }
    let length = u32::try_from(payload.len())
        .map_err(|_| KernelFailure::operator("malformed_protocol", "frame length exceeds u32"))?;
    writer
        .write_all(&length.to_be_bytes())
        .and_then(|()| writer.write_all(payload))
        .and_then(|()| writer.flush())
        .map_err(io_failure)
}

fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, KernelFailure> {
    let mut header = [0_u8; 4];
    match reader.read(&mut header[..1]) {
        Ok(0) => {
            return Err(KernelFailure::operator(
                "worker_exited",
                "worker closed stdout",
            ));
        }
        Ok(1) => {}
        Ok(_) => unreachable!("one-byte read cannot exceed one byte"),
        Err(error) => return Err(io_failure(error)),
    }
    reader.read_exact(&mut header[1..]).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            KernelFailure::operator("malformed_protocol", "truncated frame header")
        } else {
            io_failure(error)
        }
    })?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(KernelFailure::operator(
            "malformed_protocol",
            format!("frame length {length} is outside 1..={MAX_FRAME_BYTES}"),
        ));
    }
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            KernelFailure::operator("malformed_protocol", "truncated frame body")
        } else {
            io_failure(error)
        }
    })?;
    Ok(body)
}

fn parse_ready(frame: &[u8]) -> Result<(String, String), KernelFailure> {
    let value = validate_json_object(frame, "malformed_protocol")?;
    let object = value.as_object().expect("JSON object validated");
    let ready: BTreeSet<&str> = if object.contains_key("project_sha256") {
        BTreeSet::from(["op", "script_sha256", "project_sha256"])
    } else {
        BTreeSet::from(["op", "script_sha256"])
    };
    require_exact_keys(object, &ready, "malformed_protocol", "ready frame")?;
    if object.get("op").and_then(Value::as_str) != Some("ready") {
        return Err(KernelFailure::operator(
            "malformed_protocol",
            "expected ready frame",
        ));
    }
    let script = required_string(object, "script_sha256", "malformed_protocol")?.to_owned();
    if !is_sha256(&script) {
        return Err(KernelFailure::operator(
            "malformed_protocol",
            "ready script digest is malformed",
        ));
    }
    let project = match object.get("project_sha256") {
        Some(Value::String(value)) if is_sha256(value) => value.to_owned(),
        Some(_) => {
            return Err(KernelFailure::operator(
                "malformed_protocol",
                "ready project digest is malformed",
            ));
        }
        None => String::new(),
    };
    Ok((script, project))
}

fn capture_stderr(mut reader: impl Read + Send + 'static) -> Arc<Mutex<Vec<u8>>> {
    let stderr = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&stderr);
    std::thread::spawn(move || {
        let mut chunk = [0_u8; 1_024];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(length) => {
                    if let Ok(mut buffer) = captured.lock() {
                        buffer.extend_from_slice(&chunk[..length]);
                        if buffer.len() > STDERR_TAIL_BYTES {
                            let excess = buffer.len() - STDERR_TAIL_BYTES;
                            buffer.drain(..excess);
                        }
                    } else {
                        break;
                    }
                }
            }
        }
    });
    stderr
}

fn io_failure(error: std::io::Error) -> KernelFailure {
    if error.kind() == std::io::ErrorKind::BrokenPipe {
        KernelFailure::operator("worker_exited", error.to_string())
    } else {
        KernelFailure::operator("worker_io", error.to_string())
    }
}

struct TemporaryScript {
    path: PathBuf,
}

impl Drop for TemporaryScript {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn make_erosion_driver(
    script_sha256: &str,
    project_sha256: &str,
    source_path: &Path,
) -> Result<TemporaryScript, KernelFailure> {
    let source = fs::read_to_string(source_path).map_err(|error| {
        KernelFailure::operator(
            "worker_unavailable",
            format!(
                "cannot read Julia erosion worker {}: {error}",
                source_path.display()
            ),
        )
    })?;
    let source = source.trim_end();
    let body = source.strip_suffix("exit(main())").ok_or_else(|| {
        KernelFailure::provenance(
            "unsupported_worker_entrypoint",
            "erosion worker must end with the pinned exit(main()) entry point",
        )
    })?;
    let driver = format!(
        r#"

const _LUXEL_SCRIPT_SHA256 = "{script_sha256}"
const _LUXEL_PROJECT_SHA256 = "{project_sha256}"

function _luxel_read_frame(io::IO)
    header = read(io, 4)
    isempty(header) && return nothing
    length(header) == 4 || error("truncated frame header")
    n = (Int(header[1]) << 24) | (Int(header[2]) << 16) | (Int(header[3]) << 8) | Int(header[4])
    0 < n <= 1_000_000 || error("frame length out of range")
    payload = read(io, n)
    length(payload) == n || error("truncated frame body")
    return payload
end

function _luxel_write_frame(io::IO, payload::AbstractString)
    bytes = Vector{{UInt8}}(codeunits(payload))
    n = length(bytes)
    header = UInt8[UInt8((n >> 24) & 0xff), UInt8((n >> 16) & 0xff), UInt8((n >> 8) & 0xff), UInt8(n & 0xff)]
    write(io, header)
    write(io, bytes)
    flush(io)
    return nothing
end

_luxel_write_frame(stdout, JSON3.write(Dict("op" => "ready", "script_sha256" => _LUXEL_SCRIPT_SHA256, "project_sha256" => _LUXEL_PROJECT_SHA256)))
while true
    payload = try
        _luxel_read_frame(stdin)
    catch error
        println(stderr, "worker frame rejected: ", sprint(showerror, error))
        break
    end
    payload === nothing && break
    response = try
        _handle(JSON3.read(String(payload)))
    catch error
        code = error isa ErosionWorkerError ? error.code : "invalid_request"
        Dict("schema" => EROSION_RESULT_SCHEMA, "status" => "error", "error" => Dict("code" => code, "message" => sprint(showerror, error)))
    end
    _luxel_write_frame(stdout, JSON3.write(response))
end
"#
    );
    let body = body.trim_end();
    let driver_path = std::env::temp_dir().join(format!(
        "luxel-erosion-worker-{}-{}-{}.jl",
        std::process::id(),
        TEMP_SCRIPT_COUNTER.fetch_add(1, Ordering::Relaxed),
        &script_sha256[..16]
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&driver_path)
        .map_err(|error| KernelFailure::operator("worker_unavailable", error.to_string()))?;
    file.write_all(body.as_bytes())
        .and_then(|()| file.write_all(driver.as_bytes()))
        .and_then(|()| file.flush())
        .map_err(|error| {
            let _ = fs::remove_file(&driver_path);
            KernelFailure::operator("worker_unavailable", error.to_string())
        })?;
    Ok(TemporaryScript { path: driver_path })
}

pub fn script_digest(script: &Path) -> Result<String, KernelFailure> {
    let bytes = fs::read(script).map_err(|error| {
        KernelFailure::operator(
            "worker_unavailable",
            format!("cannot read worker script {}: {error}", script.display()),
        )
    })?;
    Ok(sha256_hex(&bytes))
}

fn file_digest(path: &Path, code: &str) -> Result<String, KernelFailure> {
    let bytes = fs::read(path)
        .map_err(|error| KernelFailure::provenance(code, format!("{}: {error}", path.display())))?;
    Ok(sha256_hex(&bytes))
}

pub fn project_digest(project: &Path, manifest: &Path) -> Result<String, KernelFailure> {
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    hasher.update(b"luxel.julia-project/v0\0");
    for path in [project, manifest] {
        let bytes = fs::read(path).map_err(|error| {
            KernelFailure::provenance(
                "solver_image_unreadable",
                format!("{}: {error}", path.display()),
            )
        })?;
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub fn julia_version(julia: &Path) -> Result<String, KernelFailure> {
    let output = Command::new(julia)
        .arg("--version")
        .output()
        .map_err(|error| KernelFailure::operator("worker_unavailable", error.to_string()))?;
    if !output.status.success() {
        return Err(KernelFailure::operator(
            "worker_unavailable",
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return Err(KernelFailure::operator(
            "worker_unavailable",
            "julia --version was empty",
        ));
    }
    Ok(line.to_owned())
}

/// Existing semantic-kernel solver identity, retained for compatibility.
pub fn solver_image(
    script: &Path,
    project: &Path,
    manifest: &Path,
    julia_version: &str,
) -> Result<String, KernelFailure> {
    solver_image_with_protocol(
        script,
        project,
        manifest,
        julia_version,
        b"luxel.worker-protocol/v0",
    )
}

pub fn erosion_solver_image(
    script_sha256: &str,
    project_sha256: &str,
    julia_version: &str,
) -> String {
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    for input in [
        script_sha256.as_bytes(),
        project_sha256.as_bytes(),
        julia_version.as_bytes(),
        EROSION_WRAPPER_VERSION.as_bytes(),
    ] {
        hasher.update((input.len() as u64).to_be_bytes());
        hasher.update(input);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn solver_image_with_protocol(
    script: &Path,
    project: &Path,
    manifest: &Path,
    julia_version: &str,
    protocol: &[u8],
) -> Result<String, KernelFailure> {
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    for path in [script, project, manifest] {
        let bytes = fs::read(path).map_err(|error| {
            KernelFailure::provenance(
                "solver_image_unreadable",
                format!("{}: {error}", path.display()),
            )
        })?;
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    let version = julia_version.as_bytes();
    hasher.update((version.len() as u64).to_be_bytes());
    hasher.update(version);
    hasher.update((protocol.len() as u64).to_be_bytes());
    hasher.update(protocol);
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical_json;
    use std::io::Cursor;

    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, payload).unwrap();
        bytes
    }

    #[test]
    fn framed_json_accepts_good_and_rejects_malformed_controls() {
        let good = frame(br#"{"status":"ok"}"#);
        assert_eq!(
            read_frame(&mut Cursor::new(good)).unwrap(),
            br#"{"status":"ok"}"#
        );

        let oversized = (MAX_FRAME_BYTES as u32 + 1).to_be_bytes();
        assert_eq!(
            read_frame(&mut Cursor::new(oversized)).unwrap_err().code(),
            "malformed_protocol"
        );

        assert_eq!(
            read_frame(&mut Cursor::new([0_u8; 0])).unwrap_err().code(),
            "worker_exited"
        );
        assert_eq!(
            read_frame(&mut Cursor::new([0_u8, 0])).unwrap_err().code(),
            "malformed_protocol"
        );
        assert_eq!(
            read_frame(&mut Cursor::new([0_u8, 0, 0, 3, b'a']))
                .unwrap_err()
                .code(),
            "malformed_protocol"
        );
        assert_eq!(
            read_frame(&mut Cursor::new([0_u8; 4])).unwrap_err().code(),
            "malformed_protocol"
        );
    }

    #[test]
    fn typed_job_gate_accepts_supported_request_and_rejects_bad_request() {
        let good = json!({
            "schema": EROSION_REQUEST_SCHEMA,
            "operation": "flux_field",
            "shape": [2, 3],
            "height_path": "height.f64",
            "height_sha256": "a".repeat(64),
            "output_path": "result.f64",
            "source_path": null,
            "source_sha256": null,
            "exponent": 1.15
        });
        assert!(validate_erosion_request(&good).is_ok());

        let mut bad = good.clone();
        bad["height_sha256"] = Value::String("not-a-digest".to_owned());
        assert_eq!(
            validate_erosion_request(&bad).unwrap_err().code(),
            "malformed_request"
        );
        let mut bad = good;
        bad["unexpected"] = Value::Bool(true);
        assert_eq!(
            validate_erosion_request(&bad).unwrap_err().code(),
            "malformed_request"
        );
    }

    #[test]
    fn ready_receipt_requires_both_expected_identities() {
        let script = "a".repeat(64);
        let project = "b".repeat(64);
        let good = json!({"op":"ready", "script_sha256":script, "project_sha256":project});
        assert_eq!(
            parse_ready(canonical_json(&good).as_bytes()).unwrap(),
            (script.clone(), project.clone())
        );
        let bad = json!({"op":"ready", "script_sha256":script, "project_sha256":"bad"});
        assert_eq!(
            parse_ready(canonical_json(&bad).as_bytes())
                .unwrap_err()
                .code(),
            "malformed_protocol"
        );
    }
}
