using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using UnityEditor;
using UnityEngine;

namespace Codeweald.ZoneImporter
{
    /// <summary>
    /// Imports the certified WGE MVP handoff. This is intentionally separate
    /// from the legacy zone importer: the project ledger is the authority and
    /// this editor transaction only verifies and materializes its artifacts.
    /// There is no primitive fallback for a missing or mismatched source.
    /// </summary>
    public static class WgeMvpImporter
    {
        private const string Schema = "wge.unity-mvp-import/v1";
        private const string OutputRoot = "Assets/CodewealdGenerated/WgeMvp";

        [Serializable]
        private sealed class Manifest
        {
            public string schema_version;
            public string project_id;
            public string snapshot_sha256;
            public string snapshot_path;
            public TargetProfile target;
            public Artifact[] artifacts;
            public string world_id;
            public string gameplay_artifact_id;
            public Gate[] required_gates;
        }

        [Serializable]
        private sealed class TargetProfile
        {
            public string engine;
            public string engine_version;
            public string platform;
            public string coordinate_system;
            public string build_profile;
        }

        [Serializable]
        private sealed class Artifact
        {
            public string artifact_id;
            public string kind;
            public string schema_version;
            public string path;
            public string sha256;
            public string producer;
        }

        [Serializable]
        private sealed class Gate
        {
            public string gate_id;
            public string evidence_kind;
        }

        [Serializable]
        private sealed class ImportReport
        {
            public string schema_version = "wge.unity-mvp-import-report/v1";
            public string project_id;
            public string snapshot_sha256;
            public string status;
            public string target_runtime_validation = "pending";
            public List<string> imported_artifacts = new List<string>();
            public List<string> verified_sha256 = new List<string>();
            public List<string> failures = new List<string>();
        }

        [MenuItem("Tools/Codeweald/WGE MVP/Import Certified Snapshot")]
        public static void ChooseAndImport()
        {
            var path = EditorUtility.OpenFilePanel("WGE MVP Unity Handoff", Application.dataPath, "json");
            if (!string.IsNullOrEmpty(path)) Import(path);
        }

        public static void Import(string manifestPath)
        {
            if (string.IsNullOrWhiteSpace(manifestPath) || !File.Exists(manifestPath))
                throw new InvalidOperationException("WGE MVP manifest does not exist");
            var manifest = JsonUtility.FromJson<Manifest>(File.ReadAllText(manifestPath));
            ValidateManifest(manifest);

            var report = new ImportReport { project_id = manifest.project_id, snapshot_sha256 = manifest.snapshot_sha256 };
            var manifestDirectory = Path.GetDirectoryName(Path.GetFullPath(manifestPath));
            var stagingRoot = Path.Combine(OutputRoot, Sanitize(manifest.project_id));
            Directory.CreateDirectory(stagingRoot);
            foreach (var artifact in manifest.artifacts)
            {
                var sourcePath = ResolveContainedPath(manifestDirectory, artifact.path);
                var sourceDigest = Sha256File(sourcePath);
                if (!string.Equals(sourceDigest, artifact.sha256, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidOperationException("artifact " + artifact.artifact_id + " digest mismatch");
                var destination = Path.Combine(stagingRoot, Sanitize(artifact.artifact_id) + Path.GetExtension(sourcePath));
                File.Copy(sourcePath, destination, true);
                report.imported_artifacts.Add(artifact.artifact_id);
                report.verified_sha256.Add(artifact.artifact_id + ":" + sourceDigest);
                AssetDatabase.ImportAsset(destination.Replace('\\', '/'), ImportAssetOptions.ForceUpdate);
            }

            report.status = "imported";
            var reportPath = Path.Combine(stagingRoot, "wge_mvp_import_report.json");
            File.WriteAllText(reportPath, JsonUtility.ToJson(report, true));
            AssetDatabase.Refresh();
            Debug.Log("WGE MVP imported certified snapshot " + manifest.project_id + "; runtime validation remains pending.");
        }

        private static void ValidateManifest(Manifest manifest)
        {
            if (manifest == null || manifest.schema_version != Schema)
                throw new InvalidOperationException("Expected " + Schema);
            if (manifest.target == null || !string.Equals(manifest.target.engine, "unity", StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("WGE MVP target is not Unity");
            if (string.IsNullOrWhiteSpace(manifest.project_id) || !IsSha256(manifest.snapshot_sha256))
                throw new InvalidOperationException("WGE MVP identity is incomplete");
            if (manifest.artifacts == null || manifest.artifacts.Length == 0)
                throw new InvalidOperationException("WGE MVP handoff has no artifacts");
            if (manifest.required_gates == null || manifest.required_gates.Length == 0)
                throw new InvalidOperationException("WGE MVP handoff has no required gates");
            if (manifest.artifacts.Any(artifact => artifact == null || string.IsNullOrWhiteSpace(artifact.path) || !IsSha256(artifact.sha256)))
                throw new InvalidOperationException("WGE MVP handoff contains an invalid artifact");
            if (manifest.artifacts.Select(artifact => artifact.artifact_id).Distinct().Count() != manifest.artifacts.Length)
                throw new InvalidOperationException("WGE MVP handoff contains duplicate artifact ids");
        }

        private static string ResolveContainedPath(string root, string relativePath)
        {
            if (Path.IsPathRooted(relativePath)) throw new InvalidOperationException("WGE MVP paths must be relative");
            var full = Path.GetFullPath(Path.Combine(root, relativePath));
            var normalizedRoot = Path.GetFullPath(root).TrimEnd(Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
            if (!full.StartsWith(normalizedRoot, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("WGE MVP artifact escapes the handoff root: " + relativePath);
            if (!File.Exists(full)) throw new InvalidOperationException("WGE MVP artifact is missing: " + relativePath);
            return full;
        }

        private static string Sha256File(string path)
        {
            using (var stream = File.OpenRead(path))
            using (var sha = SHA256.Create())
                return "sha256:" + BitConverter.ToString(sha.ComputeHash(stream)).Replace("-", string.Empty).ToLowerInvariant();
        }

        private static bool IsSha256(string value)
        {
            if (string.IsNullOrEmpty(value) || !value.StartsWith("sha256:", StringComparison.OrdinalIgnoreCase)) return false;
            var hex = value.Substring("sha256:".Length);
            return hex.Length == 64 && hex.All(Uri.IsHexDigit);
        }

        private static string Sanitize(string value)
        {
            var builder = new StringBuilder();
            foreach (var character in value ?? string.Empty)
                builder.Append(char.IsLetterOrDigit(character) || character == '_' || character == '-' ? character : '_');
            return builder.Length == 0 ? "unnamed" : builder.ToString();
        }
    }
}
