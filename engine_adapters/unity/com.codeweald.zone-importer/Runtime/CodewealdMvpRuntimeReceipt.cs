using UnityEngine;

namespace Codeweald.ZoneImporter
{
    /// <summary>
    /// Legacy scene/debug metadata retained for package compatibility. These
    /// fields are not a WGE receipt and must never be used to pass a runtime gate.
    /// Use the independently verified wge.unity-runtime-receipt/v1 contract.
    /// </summary>
    public sealed class CodewealdMvpRuntimeReceipt : MonoBehaviour
    {
        public string ProjectId;
        public string SnapshotSha256;
        public bool PlaythroughPassed;
        public string Outcome = "not_run";
        public string EvidencePath;
    }
}
