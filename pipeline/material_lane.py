#!/usr/bin/env python3
"""Single entry point for turning a material request into an accepted bundle.

Three ways to produce a terrain albedo already exist -- a hand/Codex/Gemini
file, the procedural granite generator, and a ComfyUI diffusion job -- but
none of them is reachable from a batch, so a gamedev has to know which script
to run by hand and where its output has to land. This module is the wiring:
it picks the provider, materializes the result through the one accepted
texture lane, and skips regeneration when nothing about the request has
changed. `material_catalog.resolve_terrain_materials` is the sole consumer of
the bundle this writes, so the bundle layout and `material_manifest.json`
schema here are exactly what that module already expects; nothing here may
drift from that contract.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import tempfile
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Iterable

import comfy_generate_texture
import generate_granite_material
import procedural_surface
from texture_material_pipeline import TextureMaterialError, materialize


LANE_VERSION = "codeweald.material-lane/v1"

_VALID_PROVIDERS = ("procedural", "surface", "comfyui", "file")

# Keyword arguments accepted by procedural_surface.generate beyond the
# positional size/seed; kept as a tuple rather than introspected from the
# function signature so a provider_args typo produces the same explicit
# "missing" style error as the other providers instead of a bare TypeError.
_SURFACE_KEYWORD_ARGS = (
    "macro_scale_px",
    "macro_contrast",
    "meso_scale_px",
    "meso_contrast",
    "fine_scale_px",
    "fine_contrast",
    "base_value",
    "value_range",
    "tint",
    "tint_variation",
)


@dataclass
class MaterialRequest:
    material_id: str
    provider: str
    provider_args: dict[str, Any] = field(default_factory=dict)
    normal_strength: float = 2.0
    repair_seams: bool = False


def _canonical_digest(request: MaterialRequest) -> str:
    # Sorted-key, whitespace-free JSON is what makes this digest reproducible
    # across processes and platforms; anything looser (dict ordering, default
    # separators) would make the same logical request hash differently and
    # defeat the idempotency this whole lane exists to provide.
    payload = {
        "provider": request.provider,
        "provider_args": request.provider_args,
        "normal_strength": request.normal_strength,
        "repair_seams": request.repair_seams,
    }
    canonical = json.dumps(payload, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def _produce_albedo(request: MaterialRequest, destination: Path) -> Path:
    provider = request.provider
    args = request.provider_args
    if provider == "file":
        source = args.get("path")
        if not source:
            raise TextureMaterialError("file provider requires provider_args.path")
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(Path(source).read_bytes())
        return destination
    if provider == "procedural":
        destination.parent.mkdir(parents=True, exist_ok=True)
        kwargs: dict[str, Any] = {}
        if "size" in args:
            kwargs["size"] = int(args["size"])
        if "seed" in args:
            kwargs["seed"] = int(args["seed"])
        pixels = generate_granite_material.generate(**kwargs)
        from PIL import Image

        Image.fromarray(pixels, mode="RGB").save(destination)
        return destination
    if provider == "surface":
        destination.parent.mkdir(parents=True, exist_ok=True)
        kwargs = {}
        if "size" in args:
            kwargs["size"] = int(args["size"])
        if "seed" in args:
            kwargs["seed"] = int(args["seed"])
        for name in _SURFACE_KEYWORD_ARGS:
            if name in args:
                kwargs[name] = args[name]
        pixels = procedural_surface.generate(**kwargs)
        from PIL import Image

        Image.fromarray(pixels, mode="RGB").save(destination)
        return destination
    if provider == "comfyui":
        required = ("server", "prompt", "negative", "seed", "size", "checkpoint")
        missing = [name for name in required if name not in args]
        if missing:
            raise TextureMaterialError("comfyui provider missing provider_args: %s" % ", ".join(missing))
        return comfy_generate_texture.generate(
            args["server"],
            destination,
            args["prompt"],
            args["negative"],
            int(args["seed"]),
            int(args["size"]),
            args["checkpoint"],
        )
    raise TextureMaterialError(
        "unknown material provider %r; valid providers are: %s" % (provider, ", ".join(_VALID_PROVIDERS))
    )


def ensure_material(request: MaterialRequest, asset_root: Path, *, force: bool = False) -> dict[str, Any]:
    if request.provider not in _VALID_PROVIDERS:
        raise TextureMaterialError(
            "unknown material provider %r; valid providers are: %s" % (request.provider, ", ".join(_VALID_PROVIDERS))
        )
    bundle_dir = asset_root.resolve() / "generated" / "codeweald_materials" / request.material_id
    digest = _canonical_digest(request)
    provenance_path = bundle_dir / "provenance.json"
    manifest_path = bundle_dir / "material_manifest.json"
    if not force and manifest_path.is_file() and provenance_path.is_file():
        try:
            existing_manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            existing_provenance = json.loads(provenance_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            existing_manifest = None
            existing_provenance = None
        if (
            isinstance(existing_manifest, dict)
            and existing_manifest.get("status") in {"accepted", "accepted_with_warnings"}
            and isinstance(existing_provenance, dict)
            and existing_provenance.get("digest") == digest
        ):
            existing_manifest["provenance"] = existing_provenance
            return existing_manifest

    with tempfile.TemporaryDirectory(prefix="codeweald_material_lane_") as temporary:
        candidate = Path(temporary) / (request.material_id + "_candidate.png")
        albedo_path = _produce_albedo(request, candidate)
        manifest = materialize(
            albedo_path,
            bundle_dir,
            request.material_id,
            normal_strength=request.normal_strength,
            repair_seams=request.repair_seams,
        )

    provenance = {
        "schema_version": LANE_VERSION,
        "provider": request.provider,
        "provider_args": request.provider_args,
        "digest": digest,
        "normal_strength": request.normal_strength,
        "repair_seams": request.repair_seams,
    }
    # Provenance is written even for a rejected candidate so the digest that
    # produced it is on record for debugging; a rejected bundle has no
    # material_manifest.json (materialize() only writes one on acceptance),
    # so the reuse check above can never mistake a rejected provenance file
    # for an accepted one and will simply try the request again next time.
    bundle_dir.mkdir(parents=True, exist_ok=True)
    provenance_path.write_text(json.dumps(provenance, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    manifest["provenance"] = provenance
    return manifest


def _request_from_namespace(args: argparse.Namespace) -> MaterialRequest:
    provider_args: dict[str, Any] = {}
    if args.provider == "file":
        if args.path:
            provider_args["path"] = args.path
    elif args.provider == "procedural":
        if args.seed is not None:
            provider_args["seed"] = args.seed
        if args.size is not None:
            provider_args["size"] = args.size
    elif args.provider == "comfyui":
        if args.server:
            provider_args["server"] = args.server
        if args.prompt:
            provider_args["prompt"] = args.prompt
        if args.negative:
            provider_args["negative"] = args.negative
        if args.seed is not None:
            provider_args["seed"] = args.seed
        if args.size is not None:
            provider_args["size"] = args.size
        if args.checkpoint:
            provider_args["checkpoint"] = args.checkpoint
    return MaterialRequest(
        material_id=args.material_id,
        provider=args.provider,
        provider_args=provider_args,
        normal_strength=args.normal_strength,
        repair_seams=args.repair_seams,
    )


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Produce or reuse an accepted Codeweald terrain material bundle")
    parser.add_argument("--asset-root", type=Path, default=Path("."))
    parser.add_argument("--force", action="store_true")
    parser.add_argument("--request-file", type=Path, help="JSON object with MaterialRequest fields, overrides other flags")
    parser.add_argument("--material-id")
    parser.add_argument("--provider", choices=_VALID_PROVIDERS)
    parser.add_argument("--normal-strength", type=float, default=2.0)
    parser.add_argument("--repair-seams", action="store_true")
    parser.add_argument("--path", help="file provider: path to an existing image")
    parser.add_argument("--seed", type=int)
    parser.add_argument("--size", type=int)
    parser.add_argument("--server")
    parser.add_argument("--prompt")
    parser.add_argument("--negative")
    parser.add_argument("--checkpoint")
    args = parser.parse_args(argv)

    if args.request_file:
        payload = json.loads(args.request_file.read_text(encoding="utf-8"))
        request = MaterialRequest(
            material_id=payload["material_id"],
            provider=payload["provider"],
            provider_args=payload.get("provider_args", {}),
            normal_strength=payload.get("normal_strength", 2.0),
            repair_seams=payload.get("repair_seams", False),
        )
    else:
        if not args.material_id or not args.provider:
            parser.error("--material-id and --provider are required without --request-file")
        request = _request_from_namespace(args)

    try:
        manifest = ensure_material(request, args.asset_root, force=args.force)
    except (TextureMaterialError, OSError) as exc:
        parser.error(str(exc))
    print("Material %s: %s" % (manifest["material_id"], manifest["status"]))
    return 0 if manifest["status"] != "rejected" else 2


if __name__ == "__main__":
    raise SystemExit(main())
