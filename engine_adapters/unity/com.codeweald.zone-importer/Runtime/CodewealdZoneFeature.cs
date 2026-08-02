using UnityEngine;

namespace Codeweald.ZoneImporter
{
    /// <summary>Engine-native feature metadata retained after manifest import.</summary>
    public sealed class CodewealdZoneFeature : MonoBehaviour
    {
        public string FeatureId;
        public string Category;
        public string Semantic;
        public Vector3[] WorldPoints;
    }
}
