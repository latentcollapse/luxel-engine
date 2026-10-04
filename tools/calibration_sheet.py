#!/usr/bin/env python3
"""Contact sheets for a `render-calibration` run (CALIBRATION-1 human review).

One sheet per rig: the row view on top, the close views tiled underneath in row
order, each labelled. A grazing run gets one sheet of its slabs. Like
parity_review_sheet.py this only lays images out; it scores nothing, because
the contract's acceptance for this scene is a human one.

Usage:
    python3 tools/calibration_sheet.py artifacts/calibration/run2 [--out DIR]
"""
import argparse
import json
import os

from PIL import Image, ImageDraw

COLUMN_ORDER = ["calibration", "metal", "rough_metal", "stone", "wet_stone", "bark", "wood", "painted", "terrain", "emissive"]
TILE_W = 480
LABEL_H = 18


def read_ppm(path):
    with open(path, "rb") as handle:
        return Image.open(handle).convert("RGB").copy()


def labelled(image, text, width):
    scaled = image.resize((width, round(image.height * width / image.width)), Image.LANCZOS)
    tile = Image.new("RGB", (width, scaled.height + LABEL_H), (16, 16, 20))
    tile.paste(scaled, (0, LABEL_H))
    ImageDraw.Draw(tile).text((6, 3), text, fill=(230, 230, 230))
    return tile


def grid(tiles, columns):
    rows = [tiles[i:i + columns] for i in range(0, len(tiles), columns)]
    width = columns * tiles[0].width
    height = sum(max(t.height for t in row) for row in rows)
    sheet = Image.new("RGB", (width, height), (16, 16, 20))
    y = 0
    for row in rows:
        for k, tile in enumerate(row):
            sheet.paste(tile, (k * tile.width, y))
        y += max(t.height for t in row)
    return sheet


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("run")
    parser.add_argument("--out")
    args = parser.parse_args()
    out = args.out or os.path.join(args.run, "sheets")
    os.makedirs(out, exist_ok=True)
    summary = json.load(open(os.path.join(args.run, "calibration_summary.json")))
    by_rig = {}
    for render in summary["renders"]:
        by_rig.setdefault(render["rig"], {})[render["view"]] = render
    for rig, views in by_rig.items():
        path = lambda view: os.path.join(args.run, rig, view, "native_capture.ppm")
        close = [f"close-{c}" for c in COLUMN_ORDER if f"close-{c}" in views]
        grazing = sorted(v for v in views if v.startswith("grazing-"))
        parts = []
        if "row" in views:
            parts.append(labelled(read_ppm(path("row")), f"{rig} / row", TILE_W * 5))
        if close:
            parts.append(grid([labelled(read_ppm(path(v)), f"{rig} / {v}", TILE_W) for v in close], 5))
        if grazing:
            parts.append(grid([labelled(read_ppm(path(v)), f"{rig} / {v}", TILE_W) for v in grazing], 4))
        width = max(p.width for p in parts)
        sheet = Image.new("RGB", (width, sum(p.height for p in parts)), (16, 16, 20))
        y = 0
        for part in parts:
            sheet.paste(part, (0, y))
            y += part.height
        target = os.path.join(out, f"{rig}.png")
        sheet.save(target)
        print("wrote", target)


if __name__ == "__main__":
    main()
