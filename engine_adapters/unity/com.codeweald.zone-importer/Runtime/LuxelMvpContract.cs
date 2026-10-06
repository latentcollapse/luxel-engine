using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

namespace Codeweald.ZoneImporter
{
    /// <summary>Strict reader for Rust project-ledger handoffs and target-runtime evidence.</summary>
    public static class LuxelMvpContract
    {
        public const string ManifestSchema = "luxel.unity-mvp-import/v1";
        public const string SnapshotSchema = "luxel.project-snapshot/v1";
        public const string EvidenceSchema = "luxel.evidence/v1";
        public const string RuntimeReceiptSchema = "luxel.unity-runtime-receipt/v1";

        public sealed class Handoff
        {
            public string ManifestPath { get; internal set; }
            public string RootPath { get; internal set; }
            public JObject Manifest { get; internal set; }
            public JObject Snapshot { get; internal set; }
            public IReadOnlyList<VerifiedArtifact> Artifacts { get; internal set; }
        }

        public sealed class VerifiedArtifact
        {
            public string Id { get; internal set; }
            public string Kind { get; internal set; }
            public string SchemaVersion { get; internal set; }
            public string SourcePath { get; internal set; }
            public string Sha256 { get; internal set; }
        }

        public static Handoff ValidateHandoff(string manifestPath)
        {
            if (string.IsNullOrWhiteSpace(manifestPath) || !File.Exists(manifestPath))
                throw new InvalidDataException("Luxel import manifest is missing");

            var fullManifestPath = Path.GetFullPath(manifestPath);
            var manifest = ParseObject(ReadUtf8(fullManifestPath, "manifest"), "manifest");
            RequireFields(manifest, "manifest", "schema_version", "project_id", "snapshot_sha256", "snapshot_path", "target", "artifacts", "world_id", "gameplay_artifact_id", "required_gates");
            Require(String(manifest, "schema_version") == ManifestSchema, "unsupported Luxel manifest schema");
            Require(NonEmpty(String(manifest, "project_id")), "manifest project_id is empty");
            Require(NonEmpty(String(manifest, "world_id")), "manifest world_id is empty");
            Require(IsDigest(String(manifest, "snapshot_sha256")), "manifest snapshot digest is malformed");

            var root = Path.GetDirectoryName(fullManifestPath);
            var snapshotPath = ResolveContained(root, String(manifest, "snapshot_path"));
            var snapshot = ParseObject(ReadUtf8(snapshotPath, "snapshot"), "snapshot");
            ValidateSnapshot(snapshot);
            Require(String(snapshot, "snapshot_sha256") == String(manifest, "snapshot_sha256"), "manifest does not bind the referenced snapshot digest");
            Require(String(snapshot, "project_id") == String(manifest, "project_id"), "manifest project identity does not match snapshot");
            Require(Canonical(snapshot["target"]) == Canonical(manifest["target"]), "manifest target does not match snapshot target");
            Require(String(manifest["target"] as JObject, "engine") == "unity", "handoff target engine is not Unity");
            Require(Canonical(snapshot["artifacts"]) == Canonical(manifest["artifacts"]), "manifest artifact list differs from the certified snapshot");
            Require(Canonical(snapshot["required_gates"]) == Canonical(manifest["required_gates"]), "manifest gate list differs from the certified snapshot");

            var artifactArray = manifest["artifacts"] as JArray;
            Require(artifactArray != null && artifactArray.Count > 0, "handoff has no artifacts");
            var artifacts = new List<VerifiedArtifact>();
            var ids = new HashSet<string>(StringComparer.Ordinal);
            foreach (var token in artifactArray)
            {
                var artifact = token as JObject;
                Require(artifact != null, "manifest artifact must be an object");
                RequireFields(artifact, "artifact", "artifact_id", "kind", "schema_version", "path", "sha256", "producer");
                var id = String(artifact, "artifact_id");
                var path = String(artifact, "path");
                var digest = String(artifact, "sha256");
                Require(NonEmpty(id) && ids.Add(id), "artifact identity is empty or duplicated");
                Require(NonEmpty(String(artifact, "kind")) && NonEmpty(String(artifact, "schema_version")) && NonEmpty(String(artifact, "producer")), "artifact metadata is incomplete");
                Require(IsDigest(digest), "artifact digest is malformed: " + id);
                var source = ResolveContained(root, path);
                Require(string.Equals(HashFile(source), digest, StringComparison.Ordinal), "artifact digest mismatch: " + id);
                artifacts.Add(new VerifiedArtifact { Id = id, Kind = String(artifact, "kind"), SchemaVersion = String(artifact, "schema_version"), SourcePath = source, Sha256 = digest });
            }

            var gameplayId = String(manifest, "gameplay_artifact_id");
            Require(ids.Contains(gameplayId), "manifest gameplay artifact is absent from the snapshot");
            return new Handoff { ManifestPath = fullManifestPath, RootPath = root, Manifest = manifest, Snapshot = snapshot, Artifacts = artifacts.AsReadOnly() };
        }

        public static void ValidateSnapshot(JObject snapshot)
        {
            RequireFields(snapshot, "snapshot", "schema_version", "snapshot_id", "project_id", "spec_sha256", "artifact_graph_sha256", "artifacts", "required_gates", "evidence", "target", "status", "snapshot_sha256");
            Require(String(snapshot, "schema_version") == SnapshotSchema, "unsupported snapshot schema");
            Require(String(snapshot, "status") == "certified", "snapshot is not certified");
            Require(NonEmpty(String(snapshot, "project_id")), "snapshot project_id is empty");
            var specDigest = String(snapshot, "spec_sha256");
            var graphDigest = String(snapshot, "artifact_graph_sha256");
            var snapshotDigest = String(snapshot, "snapshot_sha256");
            Require(IsDigest(specDigest) && IsDigest(graphDigest) && IsDigest(snapshotDigest), "snapshot digest field is malformed");
            Require(String(snapshot, "snapshot_id") == "snapshot_" + specDigest.Substring(7, 16), "snapshot_id does not derive from spec_sha256");
            var target = snapshot["target"] as JObject;
            RequireFields(target, "target profile", "engine", "engine_version", "platform", "coordinate_system", "build_profile");
            Require(String(target, "engine") == "unity" && NonEmpty(String(target, "engine_version")) && NonEmpty(String(target, "platform")) && NonEmpty(String(target, "coordinate_system")) && NonEmpty(String(target, "build_profile")), "snapshot target profile is malformed or not Unity");
            var withoutDigest = (JObject)snapshot.DeepClone();
            withoutDigest.Remove("snapshot_sha256");
            Require(HashCanonical(withoutDigest) == snapshotDigest, "snapshot content digest mismatch");

            var artifacts = snapshot["artifacts"] as JArray;
            var gates = snapshot["required_gates"] as JArray;
            var receipts = snapshot["evidence"] as JArray;
            Require(artifacts != null && artifacts.Count > 0 && gates != null && gates.Count > 0 && receipts != null, "snapshot arrays are absent or empty");
            var artifactIds = new HashSet<string>(StringComparer.Ordinal);
            foreach (var token in artifacts)
            {
                var item = token as JObject;
                Require(item != null, "snapshot artifact must be an object");
                RequireFields(item, "snapshot artifact", "artifact_id", "kind", "schema_version", "path", "sha256", "producer");
                Require(NonEmpty(String(item, "artifact_id")) && artifactIds.Add(String(item, "artifact_id")), "snapshot artifact id is empty or duplicated");
                Require(NonEmpty(String(item, "kind")) && NonEmpty(String(item, "schema_version")) && NonEmpty(String(item, "path")) && NonEmpty(String(item, "producer")) && IsDigest(String(item, "sha256")), "snapshot artifact is malformed");
            }
            var required = new Dictionary<string, string>(StringComparer.Ordinal);
            foreach (var token in gates)
            {
                var gate = token as JObject;
                Require(gate != null, "required gate must be an object");
                RequireFields(gate, "required gate", "gate_id", "evidence_kind");
                var id = String(gate, "gate_id");
                Require(NonEmpty(id) && NonEmpty(String(gate, "evidence_kind")) && !required.ContainsKey(id), "required gate is malformed or duplicated");
                required.Add(id, String(gate, "evidence_kind"));
            }

            var seen = new HashSet<string>(StringComparer.Ordinal);
            foreach (var token in receipts)
            {
                var receipt = token as JObject;
                Require(receipt != null, "snapshot receipt must be an object");
                RequireFields(receipt, "snapshot receipt", "schema_version", "receipt_id", "gate_id", "evidence_kind", "status", "artifact_id", "artifact_sha256", "observed_input_sha256", "producer", "details");
                Require(String(receipt, "schema_version") == EvidenceSchema, "unsupported evidence receipt schema");
                var receiptId = String(receipt, "receipt_id");
                var gateId = String(receipt, "gate_id");
                Require(receiptId == ReceiptId(receipt), "snapshot receipt identity is forged or stale");
                Require(required.TryGetValue(gateId, out var kind) && kind == String(receipt, "evidence_kind"), "receipt covers an undeclared gate or wrong evidence kind");
                Require(String(receipt, "status") == "pass", "required snapshot gate is not passing");
                Require(seen.Add(gateId), "snapshot has duplicate gate evidence");
                var artifact = artifacts.OfType<JObject>().SingleOrDefault(x => String(x, "artifact_id") == String(receipt, "artifact_id"));
                Require(artifact != null && String(artifact, "sha256") == String(receipt, "artifact_sha256"), "receipt artifact binding is invalid");
                Require(IsDigest(String(receipt, "observed_input_sha256")), "receipt input digest is malformed");
                Require(NonEmpty(String(receipt, "producer")) && receipt["details"] is JObject, "receipt producer/details are malformed");
            }
            Require(seen.SetEquals(required.Keys), "snapshot evidence does not exactly cover required gates");
        }

        public static JObject ParseObject(string text, string label)
        {
            try
            {
                if (text == null) throw new InvalidDataException(label + " JSON is absent");
                Require(!ContainsJsonComment(text), label + " JSON must not contain comments");
                var settings = new JsonLoadSettings { DuplicatePropertyNameHandling = DuplicatePropertyNameHandling.Error, CommentHandling = CommentHandling.Load, LineInfoHandling = LineInfoHandling.Load };
                var token = JToken.Parse(text, settings);
                var obj = token as JObject;
                Require(obj != null, label + " root must be an object");
                return obj;
            }
            catch (JsonException error) { throw new InvalidDataException(label + " JSON is malformed: " + error.Message, error); }
        }

        public static string ReadUtf8(string path, string label)
        {
            try
            {
                var text = new UTF8Encoding(false, true).GetString(File.ReadAllBytes(path));
                return text.Length > 0 && text[0] == '\uFEFF' ? text.Substring(1) : text;
            }
            catch (DecoderFallbackException error) { throw new InvalidDataException(label + " is not valid UTF-8", error); }
        }

        private static bool ContainsJsonComment(string text)
        {
            var inString = false;
            var escaped = false;
            for (var i = 0; i < (text ?? string.Empty).Length; i++)
            {
                var c = text[i];
                if (inString)
                {
                    if (escaped) escaped = false;
                    else if (c == '\\') escaped = true;
                    else if (c == '"') inString = false;
                }
                else if (c == '"') inString = true;
                else if (c == '/' && i + 1 < text.Length && (text[i + 1] == '/' || text[i + 1] == '*')) return true;
            }
            return false;
        }

        public static string ReceiptId(JObject receipt)
        {
            var unsigned = (JObject)receipt.DeepClone();
            unsigned.Remove("receipt_id");
            return "receipt_" + HashCanonical(unsigned).Substring(7, 32);
        }

        public static string HashCanonical(JToken token)
        {
            using (var sha = SHA256.Create())
            {
                var bytes = Encoding.UTF8.GetBytes(Canonical(token));
                return "sha256:" + BitConverter.ToString(sha.ComputeHash(bytes)).Replace("-", string.Empty).ToLowerInvariant();
            }
        }

        public static string Canonical(JToken token)
        {
            if (token == null) return "null";
            if (token is JObject obj)
                return "{" + string.Join(",", obj.Properties().OrderBy(p => p.Name, StringComparer.Ordinal).Select(p => JsonConvert.ToString(p.Name) + ":" + Canonical(p.Value))) + "}";
            if (token is JArray array) return "[" + string.Join(",", array.Select(Canonical)) + "]";
            if (token is JValue value)
            {
                if (value.Type == JTokenType.Float)
                {
                    var number = Convert.ToDouble(value.Value, CultureInfo.InvariantCulture).ToString("R", CultureInfo.InvariantCulture);
                    var exponent = number.IndexOfAny(new[] { 'E', 'e' });
                    if (exponent >= 0)
                    {
                        var mantissa = number.Substring(0, exponent);
                        var power = int.Parse(number.Substring(exponent + 1), NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture);
                        return mantissa + "e" + (power >= 0 ? "+" : string.Empty) + power.ToString(CultureInfo.InvariantCulture);
                    }
                    return number.IndexOf('.') < 0 ? number + ".0" : number;
                }
                if (value.Type == JTokenType.Integer)
                    return Convert.ToString(value.Value, CultureInfo.InvariantCulture);
                return value.ToString(Formatting.None);
            }
            throw new InvalidDataException("unsupported JSON token");
        }

        public static string HashFile(string path)
        {
            using (var stream = File.OpenRead(path))
            using (var sha = SHA256.Create())
                return "sha256:" + BitConverter.ToString(sha.ComputeHash(stream)).Replace("-", string.Empty).ToLowerInvariant();
        }

        public static JObject VerifyRuntimeReceipt(string receiptPath, Handoff handoff, string playerPath, string expectedNonce)
        {
            Require(handoff != null, "validated handoff is required");
            handoff = ValidateHandoff(handoff.ManifestPath);
            Require(File.Exists(receiptPath), "runtime receipt is missing");
            Require(File.Exists(playerPath), "built Unity player is missing");
            var receipt = ParseObject(ReadUtf8(receiptPath, "runtime receipt"), "runtime receipt");
            RequireFields(receipt, "runtime receipt", "schema_version", "snapshot_sha256", "player_sha256", "gameplay_artifact_id", "gameplay_artifact_sha256", "input_trace_artifact_id", "input_trace_sha256", "run_nonce", "engine_version", "platform", "fixed_tick_hz", "ticks_executed", "input_events", "waypoint_visits", "ability_activations", "objective_claimed", "final_state", "receipt_sha256");
            Require(String(receipt, "schema_version") == RuntimeReceiptSchema, "unsupported Unity runtime receipt schema");
            var receiptHash = String(receipt, "receipt_sha256");
            Require(IsDigest(receiptHash), "runtime receipt digest is malformed");
            var unsigned = (JObject)receipt.DeepClone();
            unsigned.Remove("receipt_sha256");
            Require(HashCanonical(unsigned) == receiptHash, "runtime receipt was tampered with");
            Require(String(receipt, "snapshot_sha256") == String(handoff.Snapshot, "snapshot_sha256"), "runtime receipt belongs to a different snapshot");
            Require(NonEmpty(expectedNonce) && String(receipt, "run_nonce") == expectedNonce, "runtime receipt nonce does not match this run");
            Require(String(receipt, "player_sha256") == HashFile(playerPath), "runtime receipt does not bind this player build");
            Require(VersionMatches(String(handoff.Manifest["target"] as JObject, "engine_version"), String(receipt, "engine_version")), "runtime receipt engine version differs from target profile");
            Require(String(receipt, "platform") == String(handoff.Manifest["target"] as JObject, "platform"), "runtime receipt platform differs from target profile");
            Require(receipt["fixed_tick_hz"]?.Type == JTokenType.Integer && (int)receipt["fixed_tick_hz"] == 60, "runtime did not use the required 60 Hz fixed tick");
            Require(String(receipt, "final_state") == "completed" && receipt["objective_claimed"]?.Type == JTokenType.Boolean && (bool)receipt["objective_claimed"], "runtime playthrough did not complete the objective");

            var gameplayId = String(handoff.Manifest, "gameplay_artifact_id");
            var gameplay = handoff.Artifacts.Single(x => x.Id == gameplayId);
            Require(String(receipt, "gameplay_artifact_id") == gameplay.Id && String(receipt, "gameplay_artifact_sha256") == gameplay.Sha256, "runtime receipt gameplay artifact binding is invalid");
            var gameplayJson = ParseObject(ReadUtf8(gameplay.SourcePath, "gameplay artifact"), "gameplay artifact");
            var inputTraceId = String(gameplayJson, "input_trace_id");
            var input = FindArtifactByIdentity(handoff, "luxel.input-trace/v1", "trace_id", inputTraceId);
            Require(String(receipt, "input_trace_artifact_id") == input.Id && String(receipt, "input_trace_sha256") == input.Sha256, "runtime receipt input trace binding is invalid");
            var inputJson = ParseObject(ReadUtf8(input.SourcePath, "input trace artifact"), "input trace artifact");
            Require(String(inputJson, "schema_version") == "luxel.input-trace/v1" && String(inputJson, "trace_id") == inputTraceId, "input trace schema or identity is invalid");
            RequireFields(inputJson, "input trace", "schema_version", "trace_id", "ticks");
            var inputTicks = inputJson["ticks"] as JArray;
            Require(inputTicks != null && inputTicks.Count > 0, "input trace has no events");
            var previousTick = -1;
            foreach (var item in inputTicks)
            {
                var inputEvent = item as JObject;
                RequireFields(inputEvent, "input event", "tick", "input");
                Require(inputEvent["tick"]?.Type == JTokenType.Integer && (int)inputEvent["tick"] > previousTick && NonEmpty(String(inputEvent, "input")), "input trace ticks must be non-negative, unique, ordered integers with named inputs");
                previousTick = (int)inputEvent["tick"];
            }
            Require(Canonical(receipt["input_events"]) == Canonical(inputTicks), "runtime receipt does not contain the exact scripted input trace");
            var expectedTicks = inputTicks.Select(x => (int)x["tick"]).ToArray();
            Require(receipt["ticks_executed"]?.Type == JTokenType.Integer && (int)receipt["ticks_executed"] > expectedTicks.Max(), "runtime receipt ended before the scripted input trace");

            var navigation = handoff.Artifacts.SingleOrDefault(x => x.Kind == "navigation-plan" || x.SchemaVersion == "luxel.navigation-plan/v1");
            Require(navigation != null, "navigation artifact is missing");
            var navJson = ParseObject(ReadUtf8(navigation.SourcePath, "navigation artifact"), "navigation artifact");
            var route = navJson["route"] as JArray;
            Require(route != null && route.Count >= 2, "navigation route is malformed");
            var expectedWaypoints = new JArray(route.Select(x => String(x as JObject, "node")).Select(x => (JToken)new JValue(x)));
            Require(Canonical(receipt["waypoint_visits"]) == Canonical(expectedWaypoints), "runtime did not visit the authored route in order");
            var abilityId = String(gameplayJson, "ability_id");
            var activations = receipt["ability_activations"] as JArray;
            var expectedActivations = new JArray(inputTicks
                .Where(x => (string)x["input"] == "ability_" + abilityId)
                .Select(_ => (JToken)new JValue(abilityId)));
            Require(expectedActivations.Count > 0 && Canonical(activations) == Canonical(expectedActivations), "runtime receipt ability activations do not match the authored input trace");
            return receipt;
        }

        public static VerifiedArtifact FindArtifactByIdentity(Handoff handoff, string schemaVersion, string identityField, string identity)
        {
            Require(handoff != null && NonEmpty(schemaVersion) && NonEmpty(identityField) && NonEmpty(identity), "typed artifact identity query is incomplete");
            var matches = handoff.Artifacts.Where(artifact =>
            {
                if (!string.Equals(artifact.SchemaVersion, schemaVersion, StringComparison.Ordinal)) return false;
                var payload = ParseObject(ReadUtf8(artifact.SourcePath, "typed artifact"), "typed artifact");
                return string.Equals(String(payload, identityField), identity, StringComparison.Ordinal);
            }).ToArray();
            Require(matches.Length == 1, "typed artifact identity must resolve to exactly one verified artifact: " + schemaVersion + "/" + identityField + "=" + identity);
            return matches[0];
        }

        public static string ResolveContained(string root, string relativePath)
        {
            Require(!string.IsNullOrWhiteSpace(relativePath) && !Path.IsPathRooted(relativePath), "handoff paths must be non-empty and relative");
            var normalizedRoot = Path.GetFullPath(root);
            var full = Path.GetFullPath(Path.Combine(normalizedRoot, relativePath));
            var prefix = normalizedRoot.TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar) + Path.DirectorySeparatorChar;
            var pathComparison = Path.DirectorySeparatorChar == '\\' ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal;
            Require(full.StartsWith(prefix, pathComparison), "handoff path escapes its root: " + relativePath);
            var cursor = normalizedRoot;
            foreach (var part in full.Substring(prefix.Length).Split(new[] { Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar }, StringSplitOptions.RemoveEmptyEntries))
            {
                cursor = Path.Combine(cursor, part);
                if (File.Exists(cursor) || Directory.Exists(cursor))
                    Require((File.GetAttributes(cursor) & FileAttributes.ReparsePoint) == 0, "handoff path traverses a symlink/reparse point: " + relativePath);
            }
            Require(File.Exists(full), "handoff source is missing: " + relativePath);
            return full;
        }

        public static bool IsDigest(string value)
        {
            if (value == null || !value.StartsWith("sha256:", StringComparison.Ordinal) || value.Length != 71) return false;
            return value.Substring(7).All(Uri.IsHexDigit) && value == value.ToLowerInvariant();
        }

        public static bool VersionMatches(string required, string observed)
        {
            return NonEmpty(required) && NonEmpty(observed) &&
                (string.Equals(required, observed, StringComparison.Ordinal) || observed.StartsWith(required + ".", StringComparison.Ordinal));
        }

        internal static string String(JObject obj, string name)
        {
            var value = obj?[name];
            return value != null && value.Type == JTokenType.String ? (string)value : null;
        }

        internal static bool NonEmpty(string value) => !string.IsNullOrWhiteSpace(value);

        internal static void RequireFields(JObject obj, string label, params string[] fields)
        {
            Require(obj != null, label + " must be an object");
            var expected = new HashSet<string>(fields, StringComparer.Ordinal);
            Require(obj.Properties().All(p => expected.Contains(p.Name)) && expected.All(obj.ContainsKey), label + " has missing or unknown fields");
        }

        internal static void Require(bool condition, string message)
        {
            if (!condition) throw new InvalidDataException(message);
        }
    }
}
