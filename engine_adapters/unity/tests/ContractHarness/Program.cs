using System.Security.Cryptography;
using System.Text;
using Codeweald.ZoneImporter;
using Newtonsoft.Json.Linq;

if (args.Length > 0 && args[0] == "verify")
{
    var options = ParseOptions(args.Skip(1).ToArray());
    var verifiedHandoff = WgeMvpContract.ValidateHandoff(options["manifest"]);
    WgeMvpContract.VerifyRuntimeReceipt(options["receipt"], verifiedHandoff, options["player"], options["nonce"]);
    Console.WriteLine("receipt structure and bindings valid; runtime execution authenticity remains indeterminate");
    return;
}
if (args.Length > 0 && args[0] == "validate")
{
    if (args.Length != 3 || args[1] != "--manifest") throw new ArgumentException("usage: validate --manifest PATH");
    var validated = WgeMvpContract.ValidateHandoff(args[2]);
    Console.WriteLine("valid handoff: " + (string)validated.Snapshot["snapshot_id"]);
    return;
}

var root = Path.Combine(Path.GetTempPath(), "wge-unity-contract-" + Guid.NewGuid().ToString("N"));
Directory.CreateDirectory(root);
try
{
    var handoff = CreateHandoff(root);
    WgeMvpContract.ValidateHandoff(handoff.ManifestPath);
    Console.WriteLine("PASS valid canonical handoff");

    var valid = WgeMvpContract.ValidateHandoff(handoff.ManifestPath);
    var traceArtifact = WgeMvpContract.FindArtifactByIdentity(valid, "wge.input-trace/v1", "trace_id", "gate-run-input");
    if (traceArtifact.Id != "input-artifact") throw new Exception("trace_id was incorrectly treated as the artifact_id");
    ExpectFailure("unknown trace identity", () => WgeMvpContract.FindArtifactByIdentity(valid, "wge.input-trace/v1", "trace_id", "wrong-trace"));
    Console.WriteLine("PASS semantic trace identity differs from ledger artifact id");
    var playerPath = Path.Combine(root, "player.x86_64");
    File.WriteAllBytes(playerPath, Encoding.UTF8.GetBytes("actual built player bytes"));
    var receiptPath = Path.Combine(root, "runtime_receipt.json");
    WriteReceipt(receiptPath, valid, playerPath, "run-001");
    WgeMvpContract.VerifyRuntimeReceipt(receiptPath, valid, playerPath, "run-001");
    Console.WriteLine("PASS detailed runtime receipt binding");
    ExpectFailure("duplicate JSON fields", () => WgeMvpContract.ParseObject("{\"status\":\"pass\",\"status\":\"fail\"}", "duplicate-field control"));
    var invalidUtf8Path = Path.Combine(root, "invalid.json");
    File.WriteAllBytes(invalidUtf8Path, new byte[] { 0x7b, 0xff, 0x7d });
    ExpectFailure("invalid UTF-8", () => WgeMvpContract.ReadUtf8(invalidUtf8Path, "invalid JSON"));
    File.AppendAllText(Path.Combine(root, "input-artifact.json"), " ");
    ExpectFailure("source changed after handoff validation", () => WgeMvpContract.VerifyRuntimeReceipt(receiptPath, valid, playerPath, "run-001"));

    handoff = CreateHandoff(root);
    valid = WgeMvpContract.ValidateHandoff(handoff.ManifestPath);
    ExpectFailure("missing source", () =>
    {
        File.Delete(Path.Combine(root, "gameplay.json"));
        WgeMvpContract.ValidateHandoff(handoff.ManifestPath);
    });
    File.WriteAllText(Path.Combine(root, "gameplay.json"), GameplayJson());
    ExpectFailure("tampered source digest", () =>
    {
        File.AppendAllText(Path.Combine(root, "gameplay.json"), " ");
        WgeMvpContract.ValidateHandoff(handoff.ManifestPath);
    });

    handoff = CreateHandoff(root);
    var manifest = WgeMvpContract.ParseObject(File.ReadAllText(handoff.ManifestPath), "test manifest");
    manifest["artifacts"]![0]!["path"] = "../escape.json";
    File.WriteAllText(handoff.ManifestPath, manifest.ToString());
    ExpectFailure("path escape", () => WgeMvpContract.ValidateHandoff(handoff.ManifestPath));

    if (!OperatingSystem.IsWindows())
    {
        var externalPath = Path.Combine(Path.GetDirectoryName(root)!, "wge-unity-outside-" + Guid.NewGuid().ToString("N"));
        var linkPath = Path.Combine(root, "outside-link.json");
        File.WriteAllText(externalPath, "outside");
        File.CreateSymbolicLink(linkPath, externalPath);
        ExpectFailure("symlink escape", () => WgeMvpContract.ResolveContained(root, "outside-link.json"));
        File.Delete(linkPath);
        File.Delete(externalPath);
    }

    handoff = CreateHandoff(root);
    File.WriteAllText(handoff.SnapshotPath, "{ definitely malformed }");
    ExpectFailure("malformed snapshot", () => WgeMvpContract.ValidateHandoff(handoff.ManifestPath));
    ExpectFailure("JSON comments", () => WgeMvpContract.ParseObject("{\"schema_version\":\"x\",/* comment */\"status\":\"pass\"}", "comment control"));

    handoff = CreateHandoff(root);
    var changedSnapshot = WgeMvpContract.ParseObject(File.ReadAllText(handoff.SnapshotPath), "test snapshot");
    changedSnapshot["project_id"] = "tampered-project";
    File.WriteAllText(handoff.SnapshotPath, changedSnapshot.ToString());
    ExpectFailure("tampered snapshot", () => WgeMvpContract.ValidateHandoff(handoff.ManifestPath));

    handoff = CreateHandoff(root);
    var verified = WgeMvpContract.ValidateHandoff(handoff.ManifestPath);
    WriteReceipt(receiptPath, verified, playerPath, "run-002");
    var receipt = WgeMvpContract.ParseObject(File.ReadAllText(receiptPath), "test receipt");
    receipt["final_state"] = "failed";
    File.WriteAllText(receiptPath, receipt.ToString());
    ExpectFailure("tampered runtime receipt", () => WgeMvpContract.VerifyRuntimeReceipt(receiptPath, verified, playerPath, "run-002"));

    File.WriteAllText(receiptPath, "{\"status\":\"pass\"}");
    ExpectFailure("status-only runtime receipt", () => WgeMvpContract.VerifyRuntimeReceipt(receiptPath, verified, playerPath, "run-002"));

    WriteReceipt(receiptPath, verified, playerPath, "run-stale");
    var staleReceipt = WgeMvpContract.ParseObject(File.ReadAllText(receiptPath), "test receipt");
    staleReceipt["snapshot_sha256"] = HashBytes(Encoding.UTF8.GetBytes("different snapshot"));
    RehashReceipt(staleReceipt);
    File.WriteAllText(receiptPath, staleReceipt.ToString());
    ExpectFailure("stale snapshot runtime receipt", () => WgeMvpContract.VerifyRuntimeReceipt(receiptPath, verified, playerPath, "run-stale"));

    WriteReceipt(receiptPath, verified, playerPath, "run-003");
    ExpectFailure("wrong run nonce", () => WgeMvpContract.VerifyRuntimeReceipt(receiptPath, verified, playerPath, "different-run"));
    Console.WriteLine("PASS fail-closed malformed, missing, stale, tampered and status-only controls");
}
finally
{
    Directory.Delete(root, true);
}

static void ExpectFailure(string name, Action action)
{
    try { action(); }
    catch (InvalidDataException) { Console.WriteLine("PASS reject " + name); return; }
    throw new Exception("Expected rejection: " + name);
}

static void RehashReceipt(JObject receipt)
{
    receipt.Remove("receipt_sha256");
    receipt["receipt_sha256"] = WgeMvpContract.HashCanonical(receipt);
}

static Dictionary<string, string> ParseOptions(string[] args)
{
    var result = new Dictionary<string, string>(StringComparer.Ordinal);
    for (var i = 0; i < args.Length; i += 2)
    {
        if (i + 1 >= args.Length || !args[i].StartsWith("--", StringComparison.Ordinal)) throw new ArgumentException("verify options must be --manifest PATH --receipt PATH --player PATH --nonce VALUE");
        result.Add(args[i].Substring(2), args[i + 1]);
    }
    foreach (var name in new[] { "manifest", "receipt", "player", "nonce" })
        if (!result.ContainsKey(name)) throw new ArgumentException("missing --" + name);
    return result;
}

static string HashBytes(byte[] bytes)
{
    return "sha256:" + Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();
}

static JObject Target() => new()
{
    ["engine"] = "unity", ["engine_version"] = "2022.3.0f1", ["platform"] = "linux-desktop",
    ["coordinate_system"] = "right-handed-xz-up-y", ["build_profile"] = "wge-mvp-debug"
};

static string GameplayJson() => "{\"schema_version\":\"wge.gameplay-runtime/v1\",\"runtime_id\":\"slice\",\"objective_id\":\"goal\",\"start_entity_id\":\"player\",\"ability_id\":\"pulse\",\"input_trace_id\":\"gate-run-input\",\"determinism\":\"fixed_tick_60hz\"}";

static string InputJson() => "{\"schema_version\":\"wge.input-trace/v1\",\"trace_id\":\"gate-run-input\",\"ticks\":[{\"tick\":0,\"input\":\"move_east\"},{\"tick\":1,\"input\":\"ability_pulse\"},{\"tick\":2,\"input\":\"claim_objective\"}]}";

static (string ManifestPath, string SnapshotPath) CreateHandoff(string root)
{
    Directory.CreateDirectory(root);
    var definitions = new[]
    {
        ("terrain", "terrain-manifest", "wge.terrain-manifest/v1", "{\"schema_version\":\"wge.terrain-manifest/v1\",\"world_id\":\"world\"}"),
        ("collision", "collision-plan", "wge.collision-plan/v1", "{\"schema_version\":\"wge.collision-plan/v1\",\"world_id\":\"world\"}"),
        ("navigation", "navigation-plan", "wge.navigation-plan/v1", "{\"schema_version\":\"wge.navigation-plan/v1\",\"world_id\":\"world\",\"start_node\":\"spawn\",\"objective_node\":\"goal\",\"route\":[{\"node\":\"spawn\",\"position_xz_m\":[0.0,0.0],\"neighbors\":[\"goal\"]},{\"node\":\"goal\",\"position_xz_m\":[1.0,0.0],\"neighbors\":[\"spawn\"]}] }"),
        ("gameplay", "gameplay-runtime", "wge.gameplay-runtime/v1", GameplayJson()),
        ("input-artifact", "input-trace", "wge.input-trace/v1", InputJson())
    };
    var artifacts = new JArray();
    foreach (var (id, kind, schema, json) in definitions)
    {
        var path = id + ".json";
        File.WriteAllText(Path.Combine(root, path), json);
        artifacts.Add(new JObject { ["artifact_id"] = id, ["kind"] = kind, ["schema_version"] = schema, ["path"] = path, ["sha256"] = WgeMvpContract.HashFile(Path.Combine(root, path)), ["producer"] = "test-producer" });
    }

    var gates = new JArray(new JObject { ["gate_id"] = "semantic.project_spec", ["evidence_kind"] = "semantic" });
    var evidence = new JArray();
    var evidenceReceipt = new JObject
    {
        ["schema_version"] = WgeMvpContract.EvidenceSchema, ["receipt_id"] = "", ["gate_id"] = "semantic.project_spec",
        ["evidence_kind"] = "semantic", ["status"] = "pass", ["artifact_id"] = "gameplay",
        ["artifact_sha256"] = (string)artifacts[3]!["sha256"]!, ["observed_input_sha256"] = HashBytes(Encoding.UTF8.GetBytes("spec")),
        ["producer"] = "native-validator", ["details"] = new JObject { ["validator"] = "test" }
    };
    evidenceReceipt["receipt_id"] = WgeMvpContract.ReceiptId(evidenceReceipt);
    evidence.Add(evidenceReceipt);
    var snapshot = new JObject
    {
        ["schema_version"] = WgeMvpContract.SnapshotSchema, ["snapshot_id"] = "snapshot_0123456789abcdef", ["project_id"] = "project",
        ["spec_sha256"] = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        ["artifact_graph_sha256"] = HashBytes(Encoding.UTF8.GetBytes("graph")), ["artifacts"] = artifacts,
        ["required_gates"] = gates, ["evidence"] = evidence, ["target"] = Target(), ["status"] = "certified", ["snapshot_sha256"] = ""
    };
    snapshot.Remove("snapshot_sha256");
    snapshot["snapshot_sha256"] = WgeMvpContract.HashCanonical(snapshot);
    var snapshotPath = Path.Combine(root, "project_snapshot.json");
    File.WriteAllText(snapshotPath, snapshot.ToString());
    var manifest = new JObject
    {
        ["schema_version"] = WgeMvpContract.ManifestSchema, ["project_id"] = "project", ["snapshot_sha256"] = (string)snapshot["snapshot_sha256"]!,
        ["snapshot_path"] = "project_snapshot.json", ["target"] = Target(), ["artifacts"] = artifacts.DeepClone(),
        ["world_id"] = "world", ["gameplay_artifact_id"] = "gameplay", ["required_gates"] = gates.DeepClone()
    };
    var manifestPath = Path.Combine(root, "wge_unity_mvp_import.json");
    File.WriteAllText(manifestPath, manifest.ToString());
    return (manifestPath, snapshotPath);
}

static void WriteReceipt(string path, WgeMvpContract.Handoff handoff, string playerPath, string nonce)
{
    var gameplay = handoff.Artifacts.Single(a => a.Id == "gameplay");
    var input = handoff.Artifacts.Single(a => a.Id == "input-artifact");
    var source = WgeMvpContract.ParseObject(File.ReadAllText(input.SourcePath), "input trace");
    var receipt = new JObject
    {
        ["schema_version"] = WgeMvpContract.RuntimeReceiptSchema, ["snapshot_sha256"] = (string)handoff.Snapshot["snapshot_sha256"]!,
        ["player_sha256"] = WgeMvpContract.HashFile(playerPath), ["gameplay_artifact_id"] = gameplay.Id, ["gameplay_artifact_sha256"] = gameplay.Sha256,
        ["input_trace_artifact_id"] = input.Id, ["input_trace_sha256"] = input.Sha256, ["run_nonce"] = nonce,
        ["engine_version"] = "2022.3.0f1", ["platform"] = "linux-desktop", ["fixed_tick_hz"] = 60, ["ticks_executed"] = 4,
        ["input_events"] = source["ticks"]!.DeepClone(), ["waypoint_visits"] = new JArray("spawn", "goal"),
        ["ability_activations"] = new JArray("pulse"), ["objective_claimed"] = true, ["final_state"] = "completed"
    };
    receipt["receipt_sha256"] = WgeMvpContract.HashCanonical(receipt);
    File.WriteAllText(path, receipt.ToString());
}
