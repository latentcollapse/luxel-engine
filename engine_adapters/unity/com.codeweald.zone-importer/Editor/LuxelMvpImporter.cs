using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using UnityEditor;
using UnityEngine;

namespace Codeweald.ZoneImporter
{
    /// <summary>
    /// Materializes only Rust-ledger artifacts. It validates the snapshot,
    /// manifest, every source byte and containment before writing any Unity asset.
    /// </summary>
    public static class LuxelMvpImporter
    {
        private const string OutputRoot = "Assets/CodewealdGenerated/LuxelMvp";

        [Serializable]
        private sealed class ImportReport
        {
            public string schema_version = "luxel.unity-mvp-import-report/v1";
            public string project_id;
            public string snapshot_sha256;
            public string status;
            public string snapshot_authority_validation = "indeterminate";
            public string target_runtime_validation = "indeterminate";
            public List<string> imported_artifacts = new List<string>();
            public List<string> verified_sha256 = new List<string>();
            public List<string> failures = new List<string>();
        }

        [MenuItem("Tools/Codeweald/Luxel MVP/Import Certified Snapshot")]
        public static void ChooseAndImport()
        {
            var path = EditorUtility.OpenFilePanel("Luxel MVP Unity Handoff", Application.dataPath, "json");
            if (!string.IsNullOrEmpty(path)) Import(path);
        }

        /// <summary>Compatibility entry point retained for existing menu and automation callers.</summary>
        public static void Import(string manifestPath)
        {
            var handoff = LuxelMvpContract.ValidateHandoff(manifestPath);
            var manifest = handoff.Manifest;
            var projectRoot = Path.Combine(OutputRoot, Sanitize((string)manifest["project_id"]));
            var stagingRoot = Path.Combine(projectRoot, Sanitize((string)handoff.Snapshot["snapshot_id"]));
            Directory.CreateDirectory(stagingRoot);
            var report = new ImportReport
            {
                project_id = (string)manifest["project_id"],
                snapshot_sha256 = (string)manifest["snapshot_sha256"],
                status = "imported_bytes_verified"
            };

            AssetDatabase.StartAssetEditing();
            try
            {
                foreach (var artifact in handoff.Artifacts)
                {
                    var destination = Path.Combine(stagingRoot, Sanitize(artifact.Id) + Path.GetExtension(artifact.SourcePath));
                    File.Copy(artifact.SourcePath, destination, true);
                    if (!string.Equals(LuxelMvpContract.HashFile(destination), artifact.Sha256, StringComparison.Ordinal))
                        throw new InvalidDataException("source changed during Unity import: " + artifact.Id);
                    report.imported_artifacts.Add(artifact.Id);
                    report.verified_sha256.Add(artifact.Id + ":" + artifact.Sha256);
                    AssetDatabase.ImportAsset(destination.Replace('\\', '/'), ImportAssetOptions.ForceUpdate);
                }
                var snapshotDestination = Path.Combine(stagingRoot, "project_snapshot.json");
                File.Copy(Path.Combine(handoff.RootPath, (string)manifest["snapshot_path"]), snapshotDestination, true);
                var copiedSnapshot = LuxelMvpContract.ParseObject(LuxelMvpContract.ReadUtf8(snapshotDestination, "copied snapshot"), "copied snapshot");
                LuxelMvpContract.ValidateSnapshot(copiedSnapshot);
                if (!string.Equals((string)copiedSnapshot["snapshot_sha256"], (string)manifest["snapshot_sha256"], StringComparison.Ordinal))
                    throw new InvalidDataException("snapshot changed during Unity import");
                var manifestDestination = Path.Combine(stagingRoot, "luxel_unity_mvp_import.json");
                File.Copy(handoff.ManifestPath, manifestDestination, true);
                var copiedManifest = LuxelMvpContract.ParseObject(LuxelMvpContract.ReadUtf8(manifestDestination, "copied manifest"), "copied manifest");
                if (!string.Equals(LuxelMvpContract.Canonical(copiedManifest), LuxelMvpContract.Canonical(manifest), StringComparison.Ordinal))
                    throw new InvalidDataException("manifest changed during Unity import");
                var reportPath = Path.Combine(stagingRoot, "luxel_mvp_import_report.json");
                File.WriteAllText(reportPath, JsonUtility.ToJson(report, true));
                AssetDatabase.ImportAsset(snapshotDestination.Replace('\\', '/'), ImportAssetOptions.ForceUpdate);
                AssetDatabase.ImportAsset(manifestDestination.Replace('\\', '/'), ImportAssetOptions.ForceUpdate);
                AssetDatabase.ImportAsset(reportPath.Replace('\\', '/'), ImportAssetOptions.ForceUpdate);
            }
            finally
            {
                AssetDatabase.StopAssetEditing();
                AssetDatabase.Refresh();
            }
            Debug.Log("Luxel MVP snapshot verified and imported: " + manifest["project_id"] + " / " + handoff.Snapshot["snapshot_id"] + ". Runtime remains indeterminate until a built player completes the scripted run.");
        }

        private static string Sanitize(string value)
        {
            var chars = (value ?? string.Empty).Select(c => char.IsLetterOrDigit(c) || c == '_' || c == '-' ? c : '_').ToArray();
            return chars.Length == 0 ? "unnamed" : new string(chars);
        }
    }
}
