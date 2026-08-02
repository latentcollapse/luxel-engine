#!/usr/bin/env python3
"""Small deterministic ComfyUI texture lane for reviewed Codeweald asset jobs.

The pipeline never treats the model output as a complete material.  This tool
produces a named albedo candidate with its prompt/seed recorded beside it, so a
later normal/roughness stage and visual review can accept or reject it.
"""

from __future__ import annotations

import argparse
import json
import time
import urllib.parse
import urllib.request
from pathlib import Path
from typing import Any

from texture_material_pipeline import materialize


def _request(url: str, body: dict[str, Any] | None = None) -> dict[str, Any]:
    request = urllib.request.Request(url, data=json.dumps(body).encode("utf-8") if body else None, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read().decode("utf-8"))


def _workflow(prompt: str, negative: str, seed: int, width: int, height: int, checkpoint: str, prefix: str) -> dict[str, Any]:
    return {
        "1": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": checkpoint}},
        "2": {"class_type": "CLIPTextEncode", "inputs": {"text": prompt, "clip": ["1", 1]}},
        "3": {"class_type": "CLIPTextEncode", "inputs": {"text": negative, "clip": ["1", 1]}},
        "4": {"class_type": "EmptyLatentImage", "inputs": {"width": width, "height": height, "batch_size": 1}},
        "5": {"class_type": "KSampler", "inputs": {"model": ["1", 0], "seed": seed, "steps": 7, "cfg": 2.0, "sampler_name": "dpmpp_2m", "scheduler": "karras", "positive": ["2", 0], "negative": ["3", 0], "latent_image": ["4", 0], "denoise": 1.0}},
        "6": {"class_type": "VAEDecode", "inputs": {"samples": ["5", 0], "vae": ["1", 2]}},
        "7": {"class_type": "SaveImage", "inputs": {"images": ["6", 0], "filename_prefix": prefix}},
    }


def generate(server: str, output: Path, prompt: str, negative: str, seed: int, size: int, checkpoint: str) -> Path:
    output = output.resolve()
    prefix = "codeweald/" + output.stem
    queued = _request(server.rstrip("/") + "/prompt", {"prompt": _workflow(prompt, negative, seed, size, size, checkpoint, prefix)})
    job = queued.get("prompt_id")
    if not isinstance(job, str):
        raise RuntimeError("ComfyUI did not return a prompt id: %s" % queued)
    for _ in range(180):
        history = _request(server.rstrip("/") + "/history/" + job)
        completed = history.get(job, {})
        status = completed.get("status", {}) if isinstance(completed, dict) else {}
        if status.get("status_str") == "error":
            messages = status.get("messages", [])
            detail = next((str(message[1].get("exception_message", "unknown ComfyUI error")) for message in messages if isinstance(message, list) and len(message) > 1 and isinstance(message[1], dict) and "exception_message" in message[1]), "unknown ComfyUI error")
            raise RuntimeError("ComfyUI job %s failed: %s" % (job, detail))
        images = completed.get("outputs", {}).get("7", {}).get("images", []) if isinstance(completed, dict) else []
        if images:
            image = images[0]
            query = urllib.parse.urlencode({"filename": image["filename"], "subfolder": image.get("subfolder", ""), "type": image.get("type", "output")})
            with urllib.request.urlopen(server.rstrip("/") + "/view?" + query, timeout=30) as response:
                output.parent.mkdir(parents=True, exist_ok=True)
                output.write_bytes(response.read())
            output.with_suffix(".json").write_text(json.dumps({"schema_version": "codeweald.comfy-texture/v1", "server": server, "checkpoint": checkpoint, "seed": seed, "prompt": prompt, "negative": negative, "image": image}, indent=2) + "\n", encoding="utf-8")
            return output
        time.sleep(1)
    raise TimeoutError("ComfyUI job %s did not finish within three minutes" % job)


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate a reviewable Codeweald texture candidate via local ComfyUI")
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--prompt", required=True)
    parser.add_argument("--negative", default="text, letters, logo, frame, border, object, character, tile seams, saturated neon green")
    parser.add_argument("--seed", type=int, default=26072026)
    parser.add_argument("--size", type=int, default=768, choices=(512, 768, 1024))
    # Verified against the local ComfyUI server on 2026-07-27. The two
    # DreamShaper entries registered there currently fail checkpoint loading;
    # defaulting to either turns a normal texture request into a stack trace.
    parser.add_argument("--checkpoint", default="sd_xl_base_1.0.safetensors")
    parser.add_argument("--server", default="http://127.0.0.1:8188")
    parser.add_argument("--material-output-dir", type=Path, help="run the texture acceptance/PBR derivation stage after generation")
    parser.add_argument("--material-id", help="lowercase material identifier required with --material-output-dir")
    args = parser.parse_args()
    if bool(args.material_output_dir) != bool(args.material_id):
        parser.error("--material-output-dir and --material-id must be supplied together")
    candidate = generate(args.server, args.output, args.prompt, args.negative, args.seed, args.size, args.checkpoint)
    print("Generated texture candidate: %s" % candidate)
    if args.material_output_dir:
        material = materialize(candidate, args.material_output_dir, args.material_id)
        print("Texture material %s: %s" % (material["material_id"], material["status"]))
        return 0 if material["status"] != "rejected" else 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
