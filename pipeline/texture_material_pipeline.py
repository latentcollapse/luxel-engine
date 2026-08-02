#!/usr/bin/env python3
"""Turn a reviewable albedo candidate into an auditable lightweight PBR set.

Diffusion output is never automatically a game texture.  This lane measures
basic tiling/contrast viability, records the decision, and derives deterministic
normal and roughness maps only for a candidate that passes.  It is deliberately
small enough to run locally after ComfyUI, Blender, or an artist supplies an
albedo image.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
from typing import Any, Iterable

import numpy as np
from PIL import Image


MATERIAL_VERSION = "codeweald.pbr-material/v1"
QUALITY_VERSION = "codeweald.texture-quality/v1"


class TextureMaterialError(ValueError):
    pass


def _sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def _load(path: Path) -> np.ndarray:
    try:
        with Image.open(path) as image:
            return np.asarray(image.convert("RGB"), dtype=np.float32) / 255.0
    except (OSError, ValueError) as exc:
        raise TextureMaterialError("Cannot decode texture candidate %s: %s" % (path, exc)) from exc


# A terrain material is almost never seen at one texel per pixel. On the
# reference overview of a 256 m arena at 1440x900, one screen pixel covers 36 to
# 71 texels of an 8 m-repeat 1024px material, so mipmapping averages away
# everything finer than roughly a thirty-sixth of the tile before it is ever
# drawn. Fine grain is real work that is invisible at any distance the world is
# usually judged from.
#
# What matters at range is therefore not how detailed a texture is but how much
# contrast is left once the fine detail is filtered out. Measured across the
# accepted bundles, that separates cleanly: granite keeps 0.061 and reads as
# rock from the air, while wetland peat keeps 0.012 and windpacked snow 0.014
# and both flatten to a single colour.
#
# The floor is set just above the render-side block-variation floor of 0.012.
# A material at exactly that value could not register as varied even before
# lighting and albedo attenuation reduce it further, so it needs headroom rather
# than parity.
MINIFICATION_TEXELS_PER_PIXEL = 36
MINIMUM_COARSE_CONTRAST = 0.020


def coarse_contrast(luminance: np.ndarray, texels: int = MINIFICATION_TEXELS_PER_PIXEL) -> tuple[float, float]:
    """Contrast and energy share left after removing sub-``texels`` detail.

    Returns ``(contrast, energy_fraction)``. The first is absolute and is what
    decides whether a distant surface reads as material or as paint; the second
    says how much of the texture's effort went into detail that survives, which
    is diagnostic rather than pass/fail -- a low fraction is fine on a texture
    with contrast to spare.
    """
    size = min(luminance.shape)
    square = luminance[:size, :size]
    spectrum = np.fft.fftshift(np.fft.fft2(square - square.mean()))
    rows, columns = np.mgrid[0:size, 0:size]
    radius = np.hypot(rows - size // 2, columns - size // 2)
    total = float((np.abs(spectrum) ** 2).sum())
    survives = radius <= max(size / max(texels, 1), 1.0)
    fraction = float((np.abs(spectrum[survives]) ** 2).sum() / max(total, 1e-12))
    spectrum[~survives] = 0.0
    filtered = np.real(np.fft.ifft2(np.fft.ifftshift(spectrum)))
    return float(filtered.std()), fraction


def assess(albedo: np.ndarray) -> dict[str, Any]:
    if albedo.ndim != 3 or albedo.shape[2] != 3:
        raise TextureMaterialError("Albedo must be an RGB image")
    height, width = albedo.shape[:2]
    failures: list[str] = []
    warnings: list[str] = []
    if width < 256 or height < 256:
        failures.append("texture is smaller than 256 pixels")
    if width != height:
        failures.append("texture must be square for the current terrain triplanar lane")
    luminance = albedo @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    standard_deviation = float(luminance.std())
    if standard_deviation < 0.025:
        failures.append("texture has insufficient tonal variation")
    # A seamless material has an edge discontinuity comparable to ordinary
    # adjacent-pixel change, not a hard frame or unrelated opposite border.
    interior_x = float(np.abs(albedo[:, 1:] - albedo[:, :-1]).mean())
    interior_y = float(np.abs(albedo[1:, :] - albedo[:-1, :]).mean())
    seam_x = float(np.abs(albedo[:, 0] - albedo[:, -1]).mean())
    seam_y = float(np.abs(albedo[0, :] - albedo[-1, :]).mean())
    baseline = max((interior_x + interior_y) * 0.5, 1e-5)
    edge_ratio = max(seam_x, seam_y) / baseline
    if edge_ratio > 2.25:
        failures.append("opposite texture edges have a visible tiling discontinuity")
    elif edge_ratio > 1.55:
        warnings.append("texture edges are borderline for seamless tiling")
    # Diffusion failures often present as full-width/height bands or a regular
    # contact sheet. Natural surface detail can be directional, but should not
    # concentrate nearly all of its edge energy in a few image-spanning lines.
    vertical_profile = np.abs(np.diff(luminance, axis=1)).mean(axis=0)
    horizontal_profile = np.abs(np.diff(luminance, axis=0)).mean(axis=1)
    # Edge discontinuities have their own gate above. Exclude a narrow border
    # here so one repairable seam is not misclassified as an interior grid.
    if vertical_profile.size > 16:
        vertical_profile = vertical_profile[4:-4]
    if horizontal_profile.size > 16:
        horizontal_profile = horizontal_profile[4:-4]
    vertical_line_ratio = float(np.percentile(vertical_profile, 99) / max(float(vertical_profile.mean()), 1e-5))
    horizontal_line_ratio = float(np.percentile(horizontal_profile, 99) / max(float(horizontal_profile.mean()), 1e-5))
    # Four times the ordinary per-pixel energy is already a pronounced
    # image-spanning band.  Natural rock can have veins and directional grain,
    # but a tiled wall, a concept-art frame, or an inpaint boundary should not
    # clear this material lane merely because it has varied colour.
    if (
        min(vertical_line_ratio, horizontal_line_ratio) > 1.80
        or max(vertical_line_ratio, horizontal_line_ratio) > 4.0
    ):
        failures.append("texture contains implausibly concentrated full-span line/grid artifacts")
    # A seamless tile can still be unusable when a handful of Fourier modes
    # dominate it: the texture then stamps the same wave, tread, or fish-scale
    # motif over every surface. Natural stochastic material detail distributes
    # energy across many frequencies even when it has broad veins.
    centered = luminance - luminance.mean()
    spectral_power = np.abs(np.fft.fft2(centered)) ** 2
    spectral_power[0, 0] = 0.0
    ranked_power = np.sort(spectral_power.ravel())[::-1]
    total_power = max(float(ranked_power.sum()), 1e-12)
    dominant_spectral_fraction = float(
        ranked_power[: min(24, ranked_power.size)].sum() / total_power
    )
    if dominant_spectral_fraction > 0.40:
        failures.append(
            "texture contains an implausibly repeated spectral motif"
        )
    # A warning rather than a failure: a material may legitimately be authored
    # for close inspection, and the distance it flattens at depends on a repeat
    # scale this function does not know. Saying so here still turns a full
    # build-and-capture cycle into an accept-time answer.
    surviving_contrast, surviving_fraction = coarse_contrast(luminance)
    if surviving_contrast < MINIMUM_COARSE_CONTRAST:
        warnings.append(
            "texture flattens to a single colour under minification; only "
            "%.4f contrast survives past %d texels per pixel"
            % (surviving_contrast, MINIFICATION_TEXELS_PER_PIXEL)
        )
    # Very dark or very bright images are almost always a rendered object/frame,
    # not a usable surface scan. This does not attempt to judge art style.
    mean_luminance = float(luminance.mean())
    if mean_luminance < 0.045 or mean_luminance > 0.94:
        failures.append("texture has implausible overall exposure for a surface albedo")
    return {
        "schema_version": QUALITY_VERSION,
        "status": "failed" if failures else ("warnings" if warnings else "passed"),
        "dimensions_px": {"width": width, "height": height},
        "metrics": {
            "mean_luminance": round(mean_luminance, 6),
            "luminance_stddev": round(standard_deviation, 6),
            "edge_discontinuity_ratio": round(edge_ratio, 6),
            "seam_x": round(seam_x, 6),
            "seam_y": round(seam_y, 6),
            "vertical_line_concentration": round(vertical_line_ratio, 6),
            "horizontal_line_concentration": round(horizontal_line_ratio, 6),
            "dominant_spectral_fraction": round(
                dominant_spectral_fraction, 6
            ),
            "coarse_contrast": round(surviving_contrast, 6),
            "coarse_energy_fraction": round(surviving_fraction, 6),
            "minification_texels_per_pixel": MINIFICATION_TEXELS_PER_PIXEL,
        },
        "failures": failures,
        "warnings": warnings,
    }


def _normal_map(albedo: np.ndarray, strength: float) -> np.ndarray:
    luminance = albedo @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    # Wrapped differences make the derived map tile even when its source only
    # passed the seam gate; no unbounded edge sampling artifacts are introduced.
    dx = (np.roll(luminance, -1, axis=1) - np.roll(luminance, 1, axis=1)) * strength
    dy = (np.roll(luminance, -1, axis=0) - np.roll(luminance, 1, axis=0)) * strength
    normal = np.dstack((-dx, -dy, np.ones_like(luminance)))
    normal /= np.maximum(np.linalg.norm(normal, axis=2, keepdims=True), 1e-6)
    return np.clip((normal * 0.5 + 0.5) * 255.0, 0, 255).astype(np.uint8)


def _roughness_map(albedo: np.ndarray) -> np.ndarray:
    luminance = albedo @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    local = np.abs(luminance - (np.roll(luminance, 1, axis=0) + np.roll(luminance, -1, axis=0) + np.roll(luminance, 1, axis=1) + np.roll(luminance, -1, axis=1)) * 0.25)
    roughness = np.clip(0.58 + local * 1.8 + (1.0 - luminance) * 0.14, 0.20, 0.96)
    return np.clip(roughness * 255.0, 0, 255).astype(np.uint8)


def mean_linear_luminance(albedo: np.ndarray) -> float:
    """Return the mean Rec.709 luma of an sRGB-encoded albedo, in linear light.

    A shader normalises a sample against this value in linear space, so the
    average has to be computed there too. Averaging the sRGB-encoded channels
    first and only decoding the result afterwards looks like an equivalent
    shortcut, but sRGB decode is nonlinear: it systematically overweights the
    darker pixels of the mix, so the two orders give different numbers and
    only the per-pixel-then-average order matches what the shader actually
    integrates.
    """
    if albedo.ndim != 3 or albedo.shape[2] != 3:
        raise TextureMaterialError("Albedo must be an RGB image")
    linear = np.where(albedo <= 0.04045, albedo / 12.92, ((albedo + 0.055) / 1.055) ** 2.4)
    luma = linear @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    return float(luma.mean())


def seamless_repair(albedo: np.ndarray) -> np.ndarray:
    """Make opposite edges agree through a deterministic low-frequency correction.

    This is deliberately limited to candidates that failed *only* the seam test;
    it does not invent missing detail or rescue frames/grids/objects. The source
    image remains recorded in the material manifest and the repaired image must
    pass the ordinary quality gate before it can be used.
    """
    height, width = albedo.shape[:2]
    x = np.linspace(0.0, 1.0, width, dtype=np.float32)[None, :, None]
    y = np.linspace(0.0, 1.0, height, dtype=np.float32)[:, None, None]
    repaired = albedo + x * (albedo[:, :1, :] - albedo[:, -1:, :])
    repaired = repaired + y * (repaired[:1, :, :] - repaired[-1:, :, :])
    return np.clip(repaired, 0.0, 1.0)


def materialize(albedo_path: Path, output_dir: Path, material_id: str, *, normal_strength: float = 2.0, repair_seams: bool = False) -> dict[str, Any]:
    if not material_id or any(character not in "abcdefghijklmnopqrstuvwxyz0123456789_" for character in material_id):
        raise TextureMaterialError("material_id must be lowercase letters, digits, and underscores")
    albedo_path = albedo_path.resolve()
    albedo = _load(albedo_path)
    quality = assess(albedo)
    repair_applied = False
    if repair_seams and quality["status"] == "failed" and quality["failures"] == ["opposite texture edges have a visible tiling discontinuity"]:
        albedo = seamless_repair(albedo)
        quality = assess(albedo)
        repair_applied = True
    quality["source_albedo"] = str(albedo_path)
    quality["source_sha256"] = _sha256(albedo_path)
    output_dir = output_dir.resolve()
    output_dir.mkdir(parents=True, exist_ok=True)
    (output_dir / "quality_report.json").write_text(json.dumps(quality, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if quality["status"] == "failed":
        # Never leave a former accepted manifest/maps beside a newer rejected
        # evaluation; engine adapters must only be able to discover a current,
        # accepted material contract.
        for stale in ("material_manifest.json", "albedo.png", "normal.png", "roughness.png"):
            stale_path = output_dir / stale
            if stale_path.is_file():
                stale_path.unlink()
        return {"schema_version": MATERIAL_VERSION, "material_id": material_id, "status": "rejected", "quality_report": "quality_report.json"}
    albedo_output = output_dir / "albedo.png"
    Image.fromarray(np.clip(albedo * 255.0, 0, 255).astype(np.uint8), mode="RGB").save(albedo_output)
    normal_output, roughness_output = output_dir / "normal.png", output_dir / "roughness.png"
    Image.fromarray(_normal_map(albedo, normal_strength), mode="RGB").save(normal_output)
    Image.fromarray(_roughness_map(albedo), mode="L").save(roughness_output)
    manifest = {
        "schema_version": MATERIAL_VERSION,
        "material_id": material_id,
        "status": "accepted" if quality["status"] == "passed" else "accepted_with_warnings",
        "source": {"path": str(albedo_path), "sha256": quality["source_sha256"]},
        "processing": {"seam_repair": repair_applied},
        "quality_report": "quality_report.json",
        "maps": {
            "albedo": {"path": "albedo.png", "sha256": _sha256(albedo_output)},
            "normal": {"path": "normal.png", "sha256": _sha256(normal_output), "strength": normal_strength},
            "roughness": {"path": "roughness.png", "sha256": _sha256(roughness_output)},
        },
    }
    (output_dir / "material_manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return manifest


def main(argv: Iterable[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Assess and materialize a Codeweald albedo candidate into PBR maps")
    parser.add_argument("albedo", type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument("--material-id", required=True)
    parser.add_argument("--normal-strength", type=float, default=2.0)
    parser.add_argument("--repair-seams", action="store_true", help="try deterministic edge correction only for an otherwise-valid candidate")
    args = parser.parse_args(argv)
    try:
        manifest = materialize(args.albedo, args.output_dir, args.material_id, normal_strength=args.normal_strength, repair_seams=args.repair_seams)
    except (TextureMaterialError, OSError) as exc:
        parser.error(str(exc))
    print("Texture material %s: %s" % (manifest["material_id"], manifest["status"]))
    return 0 if manifest["status"] != "rejected" else 2


if __name__ == "__main__":
    raise SystemExit(main())
