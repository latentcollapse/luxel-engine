"""Thin Neura-MCP launcher for the canonical WGE agent surface.

This module intentionally contains no WGE semantics.  It exists so a local
Codex/Luna/NIRA harness can launch a stable, named MCP adapter without knowing
the Python module layout.  Tool calls still flow through ``wge_agent_surface``
and the Rust control-plane binary owns all validation and promotion decisions.
"""

from __future__ import annotations

from pipeline.wge_agent_surface import _main


if __name__ == "__main__":
    raise SystemExit(_main())
