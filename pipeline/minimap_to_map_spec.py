"""Deprecated exploratory pixel-mask experiment.

This predates ConceptAnnotations and does not participate in `build_zone.py`.
It may be useful for a quick artist-side mask preview, but its luminance and
colour thresholds are explicitly not topology, terrain, lane, or asset truth.
New concept art must enter through `concept_batch_intake.py` and the reviewed
annotation contract.
"""

import os
import json
import numpy as np
from PIL import Image, ImageFilter

def process_minimap(image_path, output_dir):
    print(f"Loading concept image: {image_path}")
    img = Image.open(image_path).convert("RGB")
    width, height = img.size
    arr = np.array(img, dtype=np.float32)

    r, g, b = arr[:, :, 0], arr[:, :, 1], arr[:, :, 2]
    
    # 1. Heightmap Generation
    luminance = 0.299 * r + 0.587 * g + 0.114 * b
    lum_norm = (luminance - luminance.min()) / (luminance.max() - luminance.min() + 1e-5)
    
    heightmap_data = (lum_norm * 255.0).astype(np.uint8)
    heightmap_img = Image.fromarray(heightmap_data).resize((1024, 1024), Image.Resampling.LANCZOS)
    heightmap_img = heightmap_img.filter(ImageFilter.GaussianBlur(radius=3))
    
    heightmap_path = os.path.join(output_dir, "heightmap.png")
    heightmap_img.save(heightmap_path)
    print(f"Saved heightmap to: {heightmap_path}")

    # 2. Road/Dirt Mask Extraction
    road_mask = (r > 100) & (g > 80) & (b < 100) & (abs(r - g) < 40)
    road_img = Image.fromarray((road_mask * 255).astype(np.uint8)).resize((1024, 1024), Image.Resampling.BILINEAR)
    road_img = road_img.filter(ImageFilter.GaussianBlur(radius=2))
    road_mask_path = os.path.join(output_dir, "road_mask.png")
    road_img.save(road_mask_path)
    print(f"Saved road mask to: {road_mask_path}")

    # 3. Water Mask Extraction
    water_mask = (b > g) & (b > r * 0.9) & (b > 60)
    water_img = Image.fromarray((water_mask * 255).astype(np.uint8)).resize((1024, 1024), Image.Resampling.BILINEAR)
    water_img = water_img.filter(ImageFilter.GaussianBlur(radius=2))
    water_mask_path = os.path.join(output_dir, "water_mask.png")
    water_img.save(water_mask_path)
    print(f"Saved water mask to: {water_mask_path}")

    # 4. Forest Mask Extraction
    forest_mask = (g > r * 1.1) & (g > b * 1.1) & (g > 40)
    forest_img = Image.fromarray((forest_mask * 255).astype(np.uint8)).resize((1024, 1024), Image.Resampling.BILINEAR)
    forest_mask_path = os.path.join(output_dir, "forest_mask.png")
    forest_img.save(forest_mask_path)
    print(f"Saved forest mask to: {forest_mask_path}")

    # 5. Extract Map Vector Metadata JSON
    map_spec = {
        "world_size": [1000, 1000],
        "heightmap": "res://assets/generated/heightmap.png",
        "masks": {
            "road": "res://assets/generated/road_mask.png",
            "water": "res://assets/generated/water_mask.png",
            "forest": "res://assets/generated/forest_mask.png"
        },
        "keeps": {
            "albion_sw": { "position": [-380, -380], "color": "blue", "tier_radii": [180, 145, 110, 75, 45] },
            "hibernia_ne": { "position": [380, 380], "color": "green", "tier_radii": [180, 145, 110, 75, 45] }
        },
        "lanes": {
            "mid": [[-380, -380], [-180, -180], [0, 0], [180, 180], [380, 380]],
            "top": [[-380, -380], [-350, 0], [-250, 280], [0, 350], [380, 380]],
            "bot": [[-380, -380], [0, -350], [280, -250], [350, 0], [380, 380]]
        },
        "bridges": [
            { "name": "mid_bridge", "position": [0, 0], "rotation": 45 },
            { "name": "top_bridge", "position": [-250, 280], "rotation": 15 },
            { "name": "bot_bridge", "position": [280, -250], "rotation": 75 }
        ],
        "boss_pit": { "position": [-220, -180], "radius": 40.0 }
    }

    spec_path = os.path.join(output_dir, "map_layout_spec.json")
    with open(spec_path, "w") as f:
        json.dump(map_spec, f, indent=2)
    print(f"Saved map layout spec to: {spec_path}")

if __name__ == "__main__":
    image_file = "/home/matt/Downloads/ChatGPT Image Jul 26, 2026, 02_10_11 AM.png"
    out_dir = "/mnt/d/Language Projects/Game Projects/Codeweald/godot_renderer/assets/generated"
    process_minimap(image_file, out_dir)
