"""Content-hash provenance for tools, not just data.

Luxel tooling item 1 (see Luxel/docs/platform/tooling-upgrades.md): a full day of Bevy
captures once ran a stale pre-change binary and reported "passed", because
capture_bevy.py's staleness check compared file *mtimes* -- fragile against
git checkouts, rsync, and backup restores that reset mtimes without changing
content. render_plan.json already does this correctly for data
(zone_spec_sha256, asset_plan_sha256, heightfield_sha256,
terrain_manifest_sha256); this module generalises the same discipline to the
tools that produce data. A consumer that cannot verify an artifact's tool
provenance must refuse it, not assume it is trustworthy.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Iterable


def source_tree_digest(roots: Iterable[Path], extensions: tuple[str, ...]) -> str:
    """Deterministic sha256 over every matching file under the given roots.

    Same algorithm must be used on both sides of a comparison: whoever sets
    CODEWEALD_SOURCE_DIGEST before a build, and whoever later recomputes it
    to check a binary's --provenance output against current sources.
    """
    files: list[Path] = []
    for root in roots:
        files.extend(
            path
            for path in root.rglob("*")
            if path.is_file() and path.suffix in extensions
        )
    hasher = hashlib.sha256()
    for path in sorted(files, key=str):
        hasher.update(str(path).encode("utf-8"))
        hasher.update(path.read_bytes())
    return hasher.hexdigest()


def file_digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()
