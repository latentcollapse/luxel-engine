"""Where the test suite finds the reference world it measures against.

**This exists because WGE's tests are not yet standalone (D23).** Much of the
suite asserts against a real compiled batch -- the alpine arena and caledonia --
and against real generated kits. That is deliberately good testing: it is what
caught the sealed keeps and the bridges over nothing, and synthetic fixtures
would not have. But those batches and kits are *Codeweald's content*, and WGE
was separated from Codeweald so it could be pointed at any game (D8).

So the coupling that used to be implicit -- `parents[1]` happened to be both the
engine and the art -- is named here instead of being spread across ten files.
One place to look, one environment variable to redirect, and a single honest
statement of what WGE still borrows.

Set `WGE_REFERENCE_CONTENT` to test against a different game's content.
"""

from __future__ import annotations

import os
from pathlib import Path

# The engine itself. Always correct: these tests live inside it.
ENGINE_ROOT = Path(__file__).resolve().parents[1]

_DEFAULT_CONTENT = (
    ENGINE_ROOT.parent / "Game Projects" / "Codeweald" / "godot_renderer"
)

CONTENT_ROOT = Path(
    os.environ.get("WGE_REFERENCE_CONTENT", str(_DEFAULT_CONTENT))
).resolve()


def content_available() -> bool:
    """Whether the reference content is present.

    Tests that need it should skip loudly on false rather than pass quietly:
    a suite that silently shrinks when its fixtures move is the same failure
    mode as a gate that cannot fail.
    """
    return (CONTENT_ROOT / "concept_batches").is_dir()
