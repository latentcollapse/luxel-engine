// Install this folder as a local UPM package, then use Tools/Codeweald/Import Zone Manifest.
// The importer intentionally refuses to fabricate unavailable props: external GLB/FBX paths
// are copied only when they can be found and each decision appears in the import report.

using System;
using System.Collections.Generic;
using System.IO;
using System.Security.Cryptography;
using Codeweald.ZoneImporter;
using UnityEditor;
using UnityEngine;

namespace Codeweald.ZoneImporter.Editor
{
    public static class CodewealdZoneImporter
    {
        private const string Schema = "codeweald.unity-zone-import/v1";

        [Serializable] private sealed class Manifest { public string schema_version; public string zone_id; public TerrainInfo terrain; public Feature[] features; public RuntimeEffects runtime_effects; }
        [Serializable] private sealed class TerrainInfo { public string heightmap_16; public int resolution; public HeightNormalization height_normalization_m; public Size size_m; public string splatmap; }
        [Serializable] private sealed class HeightNormalization { public float min; public float max; public float range; }
        [Serializable] private sealed class Size { public float x; public float y; public float z; }
        [Serializable] private sealed class Feature { public string id; public string category; public string semantic; public Geometry geometry; public Placement placement; }
        [Serializable] private sealed class Geometry { public string type; public Point[] points_m_xz; }
        [Serializable] private sealed class Point { public float x; public float y; }
        [Serializable] private sealed class Placement { public string profile_id; public int instance_count; public SourceAsset[] source_assets; public EcologicalLayer[] layers; }
        [Serializable] private sealed class EcologicalLayer { public string id; public string role; public int instance_count; public float[] scale_m; public float minimum_spacing_m; public SourceAsset[] source_assets; }
        [Serializable] private sealed class SourceAsset { public string path; public string format; public string sha256; }
        [Serializable] private sealed class RuntimeEffects { public string schema_version; public RuntimeEffect[] effects; }
        [Serializable] private sealed class RuntimeEffect { public string feature_id; public string kind; public RuntimeParameters parameters; }
        [Serializable] private sealed class RuntimeParameters { public float flow_speed_mps; public float wave_amplitude_m; public float period_s; public float light_energy_min; public float light_energy_max; public float gust_period_s; public float sway_degrees; public float amplitude_m; public float banner_height_m; public float[] color_srgb; }
        [Serializable] private sealed class ImportReport { public string zone_id; public int placed_instances; public List<string> imported_assets = new List<string>(); public List<string> unavailable_assets = new List<string>(); }

        [MenuItem("Tools/Codeweald/Import Zone Manifest")]
        public static void ChooseAndImport()
        {
            var path = EditorUtility.OpenFilePanel("Codeweald Unity Zone Manifest", Application.dataPath, "json");
            if (!string.IsNullOrEmpty(path)) Import(path);
        }

        public static void Import(string manifestPath)
        {
            var manifest = JsonUtility.FromJson<Manifest>(File.ReadAllText(manifestPath));
            if (manifest == null || manifest.schema_version != Schema)
                throw new InvalidOperationException("Expected " + Schema);
            if (manifest.terrain == null || manifest.terrain.resolution < 33 || manifest.terrain.height_normalization_m.range <= 0f)
                throw new InvalidOperationException("Manifest has no usable terrain");
            if (!IsTerrainResolutionValid(manifest.terrain.resolution))
                throw new InvalidOperationException("Unity Terrain requires a 2^n+1 heightmap resolution");

            var batchRoot = Path.GetDirectoryName(manifestPath);
            var outputRoot = "Assets/CodewealdGenerated/" + Sanitize(manifest.zone_id);
            Directory.CreateDirectory(outputRoot);
            var terrainData = CreateTerrain(manifest, batchRoot, outputRoot);
            var terrain = Terrain.CreateTerrainGameObject(terrainData).GetComponent<Terrain>();
            terrain.name = "CodewealdTerrain_" + manifest.zone_id;
            terrain.transform.position = new Vector3(-manifest.terrain.size_m.x * 0.5f, manifest.terrain.height_normalization_m.min, -manifest.terrain.size_m.z * 0.5f);
            var featureRoot = new GameObject("CodewealdFeatures_" + manifest.zone_id).transform;
            var report = new ImportReport { zone_id = manifest.zone_id };
            var featureNodes = new Dictionary<string, CodewealdZoneFeature>();
            var copiedAssets = new Dictionary<string, string>();
            foreach (var feature in manifest.features ?? Array.Empty<Feature>())
            {
                var node = CreateFeature(feature, featureRoot, terrain, manifest.terrain.size_m);
                featureNodes[feature.id] = node;
                foreach (var source in EnumerateSourceAssets(feature.placement))
                {
                    var destination = CopyVerifiedSource(source, manifestPath, outputRoot, report);
                    if (!string.IsNullOrEmpty(destination)) copiedAssets[source.path] = destination;
                }
            }
            AssetDatabase.SaveAssets();
            AssetDatabase.Refresh();
            foreach (var feature in manifest.features ?? Array.Empty<Feature>())
                if (featureNodes.TryGetValue(feature.id, out var node)) report.placed_instances += InstantiateFeatureAssets(feature, node, terrain, copiedAssets, report);
            ApplyRuntimeEffects(manifest.runtime_effects, featureNodes);
            File.WriteAllText(Path.Combine(outputRoot, "codeweald_unity_import_report.json"), JsonUtility.ToJson(report, true));
            Debug.Log("Codeweald imported " + manifest.zone_id + "; unavailable assets: " + report.unavailable_assets.Count);
        }

        private static TerrainData CreateTerrain(Manifest manifest, string batchRoot, string outputRoot)
        {
            var height = ReadImage(Path.Combine(batchRoot, manifest.terrain.heightmap_16));
            var splat = ReadImage(Path.Combine(batchRoot, manifest.terrain.splatmap));
            var data = new TerrainData { heightmapResolution = manifest.terrain.resolution, size = new Vector3(manifest.terrain.size_m.x, manifest.terrain.size_m.y, manifest.terrain.size_m.z), alphamapResolution = ClosestPowerOfTwo(manifest.terrain.resolution - 1) };
            var heights = new float[manifest.terrain.resolution, manifest.terrain.resolution];
            for (var y = 0; y < manifest.terrain.resolution; y++) for (var x = 0; x < manifest.terrain.resolution; x++)
                heights[y, x] = height.GetPixelBilinear((float)x / (manifest.terrain.resolution - 1), (float)y / (manifest.terrain.resolution - 1)).r;
            data.SetHeightsDelayLOD(0, 0, heights);
            data.SyncHeightmap();
            data.terrainLayers = CreateLayers(outputRoot);
            var a = data.alphamapResolution;
            var alpha = new float[a, a, 4];
            for (var y = 0; y < a; y++) for (var x = 0; x < a; x++)
            {
                var weight = splat.GetPixelBilinear((float)x / (a - 1), (float)y / (a - 1));
                var total = Mathf.Max(weight.r + weight.g + weight.b + weight.a, 0.0001f);
                alpha[y, x, 0] = weight.r / total; alpha[y, x, 1] = weight.g / total; alpha[y, x, 2] = weight.b / total; alpha[y, x, 3] = weight.a / total;
            }
            data.SetAlphamaps(0, 0, alpha);
            AssetDatabase.CreateAsset(data, outputRoot + "/" + Sanitize(manifest.zone_id) + "_TerrainData.asset");
            return data;
        }

        private static TerrainLayer[] CreateLayers(string root)
        {
            var colors = new[] { new Color(0.44f, 0.45f, 0.22f), new Color(0.58f, 0.50f, 0.38f), new Color(0.32f, 0.32f, 0.32f), new Color(0.94f, 0.95f, 0.98f) };
            var names = new[] { "Grass", "Road", "Rock", "Snow" };
            var layers = new TerrainLayer[4];
            for (var i = 0; i < layers.Length; i++)
            {
                var texture = new Texture2D(1, 1); texture.SetPixel(0, 0, colors[i]); texture.Apply();
                AssetDatabase.CreateAsset(texture, root + "/" + names[i] + "_Tint.asset");
                layers[i] = new TerrainLayer { diffuseTexture = texture, tileSize = new Vector2(12, 12) };
                AssetDatabase.CreateAsset(layers[i], root + "/" + names[i] + ".terrainlayer");
            }
            return layers;
        }

        private static CodewealdZoneFeature CreateFeature(Feature feature, Transform parent, Terrain terrain, Size size)
        {
            var node = new GameObject(feature.id).AddComponent<CodewealdZoneFeature>();
            node.transform.SetParent(parent, false); node.FeatureId = feature.id; node.Category = feature.category; node.Semantic = feature.semantic;
            var points = feature.geometry?.points_m_xz ?? Array.Empty<Point>(); node.WorldPoints = new Vector3[points.Length];
            for (var i = 0; i < points.Length; i++)
            {
                var local = new Vector3(points[i].x + size.x * 0.5f, 0f, points[i].y + size.z * 0.5f);
                node.WorldPoints[i] = terrain.transform.position + local + Vector3.up * terrain.SampleHeight(local);
            }
            if ((feature.semantic == "lane" || feature.semantic == "stream") && node.WorldPoints.Length > 1)
            {
                var line = node.gameObject.AddComponent<LineRenderer>(); line.positionCount = node.WorldPoints.Length; line.widthMultiplier = feature.semantic == "stream" ? 14f : 3f; line.SetPositions(node.WorldPoints); line.useWorldSpace = true;
                if (feature.semantic == "stream") line.startColor = line.endColor = new Color(0.05f, 0.22f, 0.32f, 0.9f);
            }
            return node;
        }

        private static IEnumerable<SourceAsset> EnumerateSourceAssets(Placement placement)
        {
            if (placement == null) yield break;
            foreach (var source in placement.source_assets ?? Array.Empty<SourceAsset>()) yield return source;
            foreach (var layer in placement.layers ?? Array.Empty<EcologicalLayer>())
                foreach (var source in layer?.source_assets ?? Array.Empty<SourceAsset>()) yield return source;
        }

        private static IEnumerable<EcologicalLayer> EnumerateLayers(Placement placement)
        {
            if (placement == null) yield break;
            if (placement.layers != null && placement.layers.Length > 0)
            {
                foreach (var layer in placement.layers) if (layer != null) yield return layer;
                yield break;
            }
            yield return new EcologicalLayer
            {
                id = "primary",
                role = placement.profile_id,
                instance_count = placement.instance_count,
                scale_m = new[] { 1f, 1f },
                source_assets = placement.source_assets ?? Array.Empty<SourceAsset>()
            };
        }

        private static int InstantiateFeatureAssets(Feature feature, CodewealdZoneFeature featureNode, Terrain terrain, Dictionary<string, string> copiedAssets, ImportReport report)
        {
            if (feature?.placement == null) return 0;
            var placed = 0;
            foreach (var layer in EnumerateLayers(feature.placement))
            {
                var sources = layer.source_assets ?? Array.Empty<SourceAsset>();
                if (sources.Length == 0 || layer.instance_count <= 0) continue;
                var random = new System.Random(StableHash(feature.id + ":" + layer.id));
                for (var index = 0; index < layer.instance_count; index++)
                {
                    var source = sources[random.Next(sources.Length)];
                    if (source == null || !copiedAssets.TryGetValue(source.path, out var unityPath)) continue;
                    var prefab = AssetDatabase.LoadAssetAtPath<GameObject>(unityPath);
                    if (prefab == null)
                    {
                        report.unavailable_assets.Add(source.path + " (Unity could not create a prefab)");
                        continue;
                    }
                    var instance = PrefabUtility.InstantiatePrefab(prefab, featureNode.transform) as GameObject;
                    if (instance == null) continue;
                    instance.name = layer.id + "_" + index.ToString("D4");
                    instance.transform.position = SampleFeaturePosition(feature, featureNode, terrain, random);
                    instance.transform.rotation = Quaternion.Euler(0f, (float)random.NextDouble() * 360f, 0f);
                    var lower = layer.scale_m != null && layer.scale_m.Length > 0 ? layer.scale_m[0] : 1f;
                    var upper = layer.scale_m != null && layer.scale_m.Length > 1 ? layer.scale_m[1] : lower;
                    instance.transform.localScale *= Mathf.Lerp(lower, upper, (float)random.NextDouble());
                    placed++;
                }
            }
            return placed;
        }

        private static Vector3 SampleFeaturePosition(Feature feature, CodewealdZoneFeature featureNode, Terrain terrain, System.Random random)
        {
            var points = featureNode.WorldPoints ?? Array.Empty<Vector3>();
            if (points.Length == 0) return terrain.transform.position;
            if (feature.geometry == null || feature.geometry.type == "point" || points.Length == 1) return points[0];
            var minX = points[0].x; var maxX = minX; var minZ = points[0].z; var maxZ = minZ;
            foreach (var point in points) { minX = Mathf.Min(minX, point.x); maxX = Mathf.Max(maxX, point.x); minZ = Mathf.Min(minZ, point.z); maxZ = Mathf.Max(maxZ, point.z); }
            Vector3 candidate;
            if (feature.geometry.type == "polygon")
            {
                candidate = points[0];
                for (var attempt = 0; attempt < 48; attempt++)
                {
                    var proposed = new Vector3(Mathf.Lerp(minX, maxX, (float)random.NextDouble()), 0f, Mathf.Lerp(minZ, maxZ, (float)random.NextDouble()));
                    if (!IsInsidePolygon(points, proposed)) continue;
                    candidate = proposed;
                    break;
                }
            }
            else
            {
                var segment = random.Next(Mathf.Max(1, points.Length - 1));
                candidate = Vector3.Lerp(points[segment], points[Mathf.Min(segment + 1, points.Length - 1)], (float)random.NextDouble());
            }
            var terrainLocal = candidate - terrain.transform.position;
            candidate.y = terrain.transform.position.y + terrain.SampleHeight(terrainLocal);
            return candidate;
        }

        private static bool IsInsidePolygon(Vector3[] polygon, Vector3 point)
        {
            var inside = false;
            for (var a = 0, b = polygon.Length - 1; a < polygon.Length; b = a++)
            {
                var intersects = ((polygon[a].z > point.z) != (polygon[b].z > point.z)) &&
                    (point.x < (polygon[b].x - polygon[a].x) * (point.z - polygon[a].z) / (polygon[b].z - polygon[a].z) + polygon[a].x);
                if (intersects) inside = !inside;
            }
            return inside;
        }

        private static void ApplyRuntimeEffects(RuntimeEffects runtimeEffects, Dictionary<string, CodewealdZoneFeature> featureNodes)
        {
            if (runtimeEffects?.effects == null) return;
            foreach (var effect in runtimeEffects.effects)
            {
                if (effect == null || !featureNodes.TryGetValue(effect.feature_id, out var node)) continue;
                var parameters = effect.parameters ?? new RuntimeParameters();
                if (effect.kind == "objective_pulse")
                {
                    var pulse = node.gameObject.AddComponent<CodewealdObjectivePulse>();
                    pulse.PeriodSeconds = parameters.period_s;
                    pulse.MinimumEnergy = parameters.light_energy_min;
                    pulse.MaximumEnergy = parameters.light_energy_max;
                    if (node.WorldPoints != null && node.WorldPoints.Length > 0) pulse.transform.position = node.WorldPoints[0] + Vector3.up * 3f;
                }
                else if (effect.kind == "water_flow" && node.GetComponent<LineRenderer>() != null)
                {
                    var water = node.gameObject.AddComponent<CodewealdWaterFlow>();
                    water.FlowSpeed = parameters.flow_speed_mps;
                    water.WaveAmplitude = parameters.wave_amplitude_m;
                }
                else if (effect.kind == "foliage_wind")
                {
                    var wind = node.gameObject.AddComponent<CodewealdFoliageWind>();
                    wind.GustPeriodSeconds = parameters.gust_period_s;
                    wind.SwayDegrees = parameters.sway_degrees;
                }
                else if (effect.kind == "banner_wave" && node.WorldPoints != null && node.WorldPoints.Length > 0)
                {
                    var banner = GameObject.CreatePrimitive(PrimitiveType.Quad);
                    banner.name = "CodewealdRealmBanner";
                    banner.transform.SetParent(node.transform, true);
                    banner.transform.position = node.WorldPoints[0] + Vector3.up * Mathf.Max(12f, parameters.banner_height_m);
                    banner.transform.localScale = new Vector3(14f, 9f, 1f);
                    var renderer = banner.GetComponent<MeshRenderer>();
                    renderer.sharedMaterial = new Material(Shader.Find("Standard"));
                    if (parameters.color_srgb != null && parameters.color_srgb.Length == 3)
                        renderer.sharedMaterial.color = new Color(parameters.color_srgb[0], parameters.color_srgb[1], parameters.color_srgb[2]);
                    var wave = banner.AddComponent<CodewealdBannerWave>();
                    wave.PeriodSeconds = parameters.period_s;
                    wave.AmplitudeDegrees = Mathf.Max(2f, parameters.amplitude_m * 10f);
                }
            }
        }

        private static int StableHash(string value)
        {
            unchecked
            {
                var hash = 17;
                foreach (var character in value ?? string.Empty) hash = hash * 31 + character;
                return hash & 0x7fffffff;
            }
        }

        private static string CopyVerifiedSource(SourceAsset source, string manifestPath, string outputRoot, ImportReport report)
        {
            var found = FindSource(Path.GetDirectoryName(manifestPath), source.path);
            if (found == null) { report.unavailable_assets.Add(source.path); return null; }
            if (!string.IsNullOrEmpty(source.sha256) && !string.Equals(HashFile(found), source.sha256, StringComparison.OrdinalIgnoreCase))
            { report.unavailable_assets.Add(source.path + " (SHA-256 mismatch)"); return null; }
            var destination = Path.Combine(outputRoot, "SourceAssets", source.sha256 + Path.GetExtension(source.path)); Directory.CreateDirectory(Path.GetDirectoryName(destination));
            if (!File.Exists(destination)) File.Copy(found, destination);
            report.imported_assets.Add(source.path);
            return destination.Replace('\\', '/');
        }

        private static string FindSource(string start, string relative)
        {
            var current = new DirectoryInfo(start);
            for (var depth = 0; current != null && depth < 8; depth++, current = current.Parent)
            { var candidate = Path.Combine(current.FullName, relative); if (File.Exists(candidate)) return candidate; }
            return null;
        }
        private static Texture2D ReadImage(string path) { var texture = new Texture2D(2, 2, TextureFormat.RGBA32, false, true); if (!texture.LoadImage(File.ReadAllBytes(path), false)) throw new InvalidOperationException("Cannot decode " + path); return texture; }
        private static string HashFile(string path) { using (var hash = SHA256.Create()) using (var file = File.OpenRead(path)) { return BitConverter.ToString(hash.ComputeHash(file)).Replace("-", "").ToLowerInvariant(); } }
        private static bool IsTerrainResolutionValid(int value) => value > 2 && ((value - 1) & (value - 2)) == 0;
        private static int ClosestPowerOfTwo(int value) { return Mathf.ClosestPowerOfTwo(value); }
        private static string Sanitize(string value) { foreach (var invalid in Path.GetInvalidFileNameChars()) value = value.Replace(invalid, '_'); return value; }
    }
}
