#!/usr/bin/env python3
"""Deterministic source-image roles for multi-view Codeweald concept batches."""

from __future__ import annotations

from typing import Any


RECONCILIATION_VERSION = "codeweald.image-reconciliation/v1"
EVIDENCE_CLAIMS = {
    "topology",
    "placement",
    "biome",
    "silhouette",
    "elevation",
    "material",
    "asset_scale",
    "style",
    "occlusion",
}
MAP_SOURCE_ROLES = {"painted_overview", "minimap", "orthographic_map"}
ROLE_ALLOWED_CLAIMS = {
    "painted_overview": {
        "topology", "placement", "biome", "silhouette", "style", "occlusion",
    },
    "minimap": {"topology", "placement", "biome", "silhouette"},
    "orthographic_map": {
        "topology", "placement", "biome", "silhouette", "style",
    },
    "perspective": {
        "silhouette", "elevation", "material", "asset_scale", "style", "occlusion",
    },
    "landmark_detail": {"silhouette", "material", "asset_scale", "style", "occlusion"},
    "material_reference": {"material", "style"},
    "foliage_reference": {"biome", "material", "asset_scale", "style", "occlusion"},
    "architecture_reference": {
        "silhouette", "material", "asset_scale", "style", "occlusion",
    },
    "elevation_reference": {"silhouette", "elevation", "occlusion"},
    # Backward-compatible generic roles remain evidence-only in multi-image
    # batches. A lone generic image can still become the canonical map.
    "reference": {
        "biome", "silhouette", "elevation", "material", "asset_scale", "style", "occlusion",
    },
    "detail": {
        "biome", "silhouette", "elevation", "material", "asset_scale", "style", "occlusion",
    },
}
ROLE_VIEW_KIND = {
    **{role: "map" for role in MAP_SOURCE_ROLES},
    "perspective": "perspective",
    "landmark_detail": "detail",
    "material_reference": "material",
    "foliage_reference": "detail",
    "architecture_reference": "detail",
    "elevation_reference": "elevation",
    "reference": "unregistered_reference",
    "detail": "detail",
}


class ReconciliationError(ValueError):
    """The image batch does not have an unambiguous coordinate authority."""


def build_reconciliation(
    images: list[dict[str, Any]],
    canonical_map_image_id: str | None,
    width_m: float,
    length_m: float,
) -> dict[str, Any]:
    if not images:
        raise ReconciliationError("At least one source image is required")
    by_id = {str(image.get("id", "")): image for image in images}
    if len(by_id) != len(images) or "" in by_id:
        raise ReconciliationError("Source image ids must be unique and non-empty")
    for image_id, image in by_id.items():
        role = str(image.get("role", "reference"))
        if role not in ROLE_ALLOWED_CLAIMS:
            raise ReconciliationError(
                "Unsupported role %r for source image %s" % (role, image_id)
            )
    if canonical_map_image_id is None:
        map_candidates = [
            image_id
            for image_id, image in by_id.items()
            if str(image.get("role", "reference")) in MAP_SOURCE_ROLES
        ]
        if len(map_candidates) == 1:
            canonical_map_image_id = map_candidates[0]
        elif len(images) == 1:
            canonical_map_image_id = str(images[0]["id"])
        else:
            raise ReconciliationError(
                "Multi-image batches require exactly one explicit canonical map"
            )
    if canonical_map_image_id not in by_id:
        raise ReconciliationError(
            "canonical map references unknown image id %s" % canonical_map_image_id
        )
    canonical = by_id[canonical_map_image_id]
    canonical_role = str(canonical.get("role", "reference"))
    if len(images) > 1 and canonical_role not in MAP_SOURCE_ROLES:
        raise ReconciliationError(
            "Multi-image canonical map %s must use a map role" % canonical_map_image_id
        )

    policies: list[dict[str, Any]] = []
    for image in images:
        image_id = str(image["id"])
        role = str(image.get("role", "reference"))
        claims = set(ROLE_ALLOWED_CLAIMS[role])
        registration: dict[str, Any]
        if image_id == canonical_map_image_id:
            claims.update({"topology", "placement", "biome"})
            registration = {
                "type": "canonical_world_rect",
                "normalized_world_rect": [0.0, 0.0, 1.0, 1.0],
            }
            width_px = image.get("width_px")
            height_px = image.get("height_px")
            if (
                isinstance(width_px, int)
                and width_px > 0
                and isinstance(height_px, int)
                and height_px > 0
            ):
                registration["meters_per_pixel"] = [
                    round(width_m / width_px, 8),
                    round(length_m / height_px, 8),
                ]
        else:
            registration = {"type": "evidence_only"}
        policies.append(
            {
                "image_id": image_id,
                "role": role,
                "view_kind": ROLE_VIEW_KIND[role],
                "allowed_claims": sorted(claims),
                "registration": registration,
            }
        )
    return {
        "schema_version": RECONCILIATION_VERSION,
        "canonical_map_image_id": canonical_map_image_id,
        "source_policies": policies,
        "cross_view_policy": {
            "geometry_requires_registered_map": True,
            "evidence_claim_required_for_multi_image": True,
            "every_source_image_requires_evidence": len(images) > 1,
            "inferred_visibility_requires_review": True,
        },
    }
