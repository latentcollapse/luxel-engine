using UnityEngine;

namespace Codeweald.ZoneImporter
{
    /// <summary>
    /// Runtime-side receipt anchor for the WGE MVP handoff. The gameplay
    /// adapter fills the outcome fields after a real playthrough; import alone
    /// never marks the runtime gate as passed.
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
