use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;
use luxel_gameplay_contract::GameplayReceipt;

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "luxel-gameplay-contract-cli-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create isolated CLI test directory");
        Self(path)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn checked_in_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/fixtures/gameplay_contract/vertical_slice_v1.json")
}

fn run_cli(input: &std::path::Path, output: &std::path::Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_luxel-gameplay-contract"))
        .arg("run")
        .arg(input)
        .arg(output)
        .output()
        .expect("launch Rust gameplay CLI")
}

#[test]
fn cli_writes_the_canonical_receipt_for_the_checked_in_playthrough() {
    let temp = TempDir::new();
    let output_path = temp.path().join("receipt.json");
    let output = run_cli(&checked_in_fixture(), &output_path);
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let bytes = fs::read(&output_path).expect("CLI wrote output receipt");
    let receipt: GameplayReceipt = serde_json::from_slice(&bytes).expect("receipt is typed JSON");
    let expected: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/gameplay_contract/vertical_slice_v1.expected.json"
    ))
    .expect("receipt golden is valid JSON");
    assert_eq!(
        receipt.receipt_sha256,
        expected["receipt_sha256"].as_str().unwrap()
    );
    assert_eq!(receipt.canonical_bytes().unwrap(), bytes);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        receipt.receipt_sha256
    );
}

#[test]
fn cli_rejects_semantic_and_unknown_field_inputs_with_named_failures() {
    let temp = TempDir::new();
    let original: Value = serde_json::from_slice(
        &fs::read(checked_in_fixture()).expect("checked-in fixture is readable"),
    )
    .expect("checked-in fixture is JSON");

    let malformed_path = temp.path().join("malformed.json");
    let malformed_output = temp.path().join("malformed-receipt.json");
    let mut malformed = original.clone();
    malformed["unrecognized"] = Value::Bool(true);
    fs::write(
        &malformed_path,
        serde_json::to_vec(&malformed).expect("serialize malformed test input"),
    )
    .expect("write malformed test input");
    let malformed_result = run_cli(&malformed_path, &malformed_output);
    assert!(!malformed_result.status.success());
    let malformed_stderr = String::from_utf8_lossy(&malformed_result.stderr);
    assert!(malformed_stderr.contains("GameFailure: MalformedInput"));
    assert!(
        !malformed_output.exists(),
        "bad input must not produce a receipt"
    );

    let semantic_path = temp.path().join("silent-ability.json");
    let semantic_output = temp.path().join("silent-receipt.json");
    let mut silent_ability = original;
    silent_ability["snapshot"]["ability"]["effect"]["amount"] = Value::from(0);
    fs::write(
        &semantic_path,
        serde_json::to_vec(&silent_ability).expect("serialize semantic negative control"),
    )
    .expect("write semantic negative control");
    let semantic_result = run_cli(&semantic_path, &semantic_output);
    assert!(!semantic_result.status.success());
    let semantic_stderr = String::from_utf8_lossy(&semantic_result.stderr);
    assert!(semantic_stderr.contains("GameFailure: SilentAbility"));
    assert!(
        !semantic_output.exists(),
        "rejected gameplay must not produce a receipt"
    );
}
