"""Erosion: the process that makes terrain look like somewhere (systems S3).

Every landform Luxel builds is noise with a silhouette. Ridged multifractal (S18)
gets you *ridges*, and ridges are not the Alps. Reviewed 2026-08-02 against
Lauterbrunnen and the verdict was fair: the mountains are still odd shapes.

**The reason is that Alpine form is not a shape, it is a history.** A glacial
trough has a flat floor and walls that go vertical because ice a kilometre thick
sat in it and ground sideways as well as down. Spurs are truncated because the
ice cut across them. Tributary valleys hang above the trunk floor -- and deliver
the waterfalls the reference photograph is full of -- because they carried less
ice and so cut less deeply. None of that can be authored as a noise function,
and all of it falls out of simulating the process for a few hundred steps.

Three processes, each producing a signature the others cannot:

**Fluvial** (stream power) cuts V-shaped valleys and dendritic networks. Erosion
goes as discharge^m x slope^n. This is what carves the drainage the map already
measures.

**Glacial** cuts U-shaped troughs. The critical difference from fluvial is that
it is *lateral*: erosion is applied over a width that scales with ice flux, so a
trunk valley is widened into a U while a tributary stays narrow and is left
hanging. Ice only exists above the equilibrium line, which is why the U-troughs
start partway up the mountain and the ground below them is river-cut.

**Thermal** moves material that stands steeper than its angle of repose. It
rounds ridges, builds talus fans at cliff feet, and is what stops the other two
producing knife-edges nothing could stand on.

The character decides the mix. The Alps are glacial over fluvial with strong
widening; the Highlands are the same processes run longer on softer rock, giving
whalebacks and broad straths; the Andes are steeper and less widened, leaving
spires.

Pure arithmetic over arrays -- no batch, no I/O, no `bpy`, no rng. Deterministic
in its input.
"""

from __future__ import annotations

import math
import atexit
import hashlib
import json
import os
import queue
import re
import shlex
import signal
import shutil
import subprocess
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path

import numpy as np

# Eight-neighbour offsets and their centre-to-centre distance in cells.
_NEIGHBOURS = (
    (1, 0, 1.0), (-1, 0, 1.0), (0, 1, 1.0), (0, -1, 1.0),
    (1, 1, math.sqrt(2.0)), (1, -1, math.sqrt(2.0)),
    (-1, 1, math.sqrt(2.0)), (-1, -1, math.sqrt(2.0)),
)


@dataclass(frozen=True)
class ErosionProfile:
    """How hard each process ran, and for how long.

    These are not physical constants. They are the knobs that decide which
    signature dominates, and they are named for the landform they produce rather
    than for the equation term they scale, because the equation term is not what
    an author is choosing between.
    """

    key: str
    iterations: int
    # Fluvial: cuts V-valleys and the dendritic network.
    stream_power: float
    # Glacial: cuts U-troughs. `ice_line` is the fraction of *ground* that lies
    # below the equilibrium line, so 0.8 means the top fifth of the map carries
    # ice. Expressed by area rather than by height because "min + 0.46 x relief"
    # is measured against whatever the single lowest cell happens to be: on a
    # world with a high border that put 97.9% of the map above the snowline.
    glacial_strength: float
    ice_line: float
    # How far the ice reaches sideways relative to its flux. This single number
    # is most of the difference between a U and a V.
    lateral_widening: float
    # Thermal: the angle above which rock will not stand.
    talus_degrees: float
    # How much cirque over-deepening happens at the heads of the ice network.
    cirque_strength: float
    # What fraction of the wedge above the valley floor the ice removes over the
    # *whole* run. This is the number that decides how completely a V becomes a
    # U, and it has to be expressed per-run rather than per-iteration: a fixed
    # per-step fraction compounds, so 0.22 over 64 steps planes 99.99% of the
    # wedge away and takes the mountains with it -- measured, relief fell from
    # 73 m to 32 m and the terrain gate failed on 24 m gouges.
    planation: float = 0.7


PROFILES: dict[str, ErosionProfile] = {
    "alps": ErosionProfile(
        key="alps",
        iterations=40,
        stream_power=0.55,
        glacial_strength=1.0,
        ice_line=0.80,
        lateral_widening=1.0,
        talus_degrees=38.0,
        cirque_strength=0.85,
        planation=0.35,
    ),
    "highlands": ErosionProfile(
        key="highlands",
        # The same processes run longer on softer rock: everything rounds off,
        # the troughs go broad, and the relief comes down. This is also the
        # right model for a worn range like the Appalachians.
        iterations=48,
        stream_power=0.75,
        glacial_strength=0.7,
        ice_line=0.72,
        lateral_widening=1.7,
        talus_degrees=31.0,
        cirque_strength=0.45,
        planation=0.45,
    ),
    "andes": ErosionProfile(
        key="andes",
        iterations=36,
        stream_power=0.9,
        glacial_strength=0.85,
        ice_line=0.86,
        # Barely widened: ice cuts down hard and leaves the rock between the
        # troughs standing as towers.
        lateral_widening=0.45,
        talus_degrees=44.0,
        cirque_strength=1.0,
        planation=0.4,
    ),
}

DEFAULT_PROFILE = "alps"


def _shift(values: np.ndarray, dr: int, dc: int) -> np.ndarray:
    """Shift by (dr, dc), repeating the edge rather than wrapping.

    Wrapping would let the north rim drain into the south one, which produces a
    seam of impossible erosion straight across the map.
    """
    out = np.empty_like(values)
    if dr > 0:
        out[dr:, :] = values[:-dr, :]
        out[:dr, :] = values[0, :]
    elif dr < 0:
        out[:dr, :] = values[-dr:, :]
        out[dr:, :] = values[-1, :]
    else:
        out[:] = values
    if dc > 0:
        out[:, dc:] = out[:, :-dc]
        out[:, :dc] = out[:, dc : dc + 1]
    elif dc < 0:
        out[:, :dc] = out[:, -dc:]
        out[:, dc:] = out[:, dc - 1 : dc]
    return out


def _python_flux_field(
    height: np.ndarray,
    *,
    source: np.ndarray | None = None,
    exponent: float = 1.15,
) -> np.ndarray:
    """Multiple-flow-direction accumulation, in one pass down the topology.

    **Relaxation does not work here and it took a measurement to see why.** An
    iterative version propagates flux one cell per iteration, so twelve
    iterations move water twelve cells -- on a 257-grid the downstream half of
    every valley accumulated nothing, ice flux read 0.00 across an entire
    cross-section, and both the glacial and fluvial terms were multiplying by
    zero while appearing to run.

    Processing cells in descending height order is a topological sort of the
    flow graph: by the time a cell is handled every cell that drains into it has
    already given up its share, so one pass is exact at any domain size.

    Multiple-flow rather than D8: sending a cell's whole discharge to one
    neighbour makes single-cell stringy channels and, on a planar hillside,
    picks arbitrarily between equal neighbours and prints the result as stripes.
    """
    rows, columns = height.shape
    flux = np.ones_like(height) if source is None else np.maximum(source, 0.0).copy()

    # Slope-weighted share to each lower neighbour, precomputed.
    weights = []
    total = np.zeros_like(height)
    for dr, dc, distance in _NEIGHBOURS:
        difference = np.maximum((height - _shift(height, dr, dc)) / distance, 0.0)
        weighted = np.power(difference, exponent)
        weights.append(weighted)
        total += weighted
    safe = np.where(total > 0.0, total, 1.0)
    shares = [w / safe for w in weights]

    order = np.argsort(height, axis=None)[::-1]
    flat = flux.ravel()
    share_flat = [s.ravel() for s in shares]
    for index in order:
        index = int(index)
        carried = flat[index]
        if carried <= 0.0:
            continue
        row, column = divmod(index, columns)
        for offset, (dr, dc, _distance) in enumerate(_NEIGHBOURS):
            portion = share_flat[offset][index]
            if portion <= 0.0:
                continue
            # `_shift(height, dr, dc)[r, c]` is `height[r - dr, c - dc]`, so
            # the weight computed for offset (dr, dc) describes the drop toward
            # `(row - dr, column - dc)`. Sending to `+` instead sent every
            # cell's discharge *uphill*: measured, the flux maximum sat at the
            # top row of a valley that drained downward, and the outlet had
            # none. A sign error that a magnitude check would never have found,
            # because the numbers were entirely plausible.
            r, c = row - dr, column - dc
            if 0 <= r < rows and 0 <= c < columns:
                flat[r * columns + c] += carried * portion
    return flat.reshape(height.shape)


def _python_thermal_erosion(
    height: np.ndarray, *, cell_m: float, talus_degrees: float, rate: float = 0.35
) -> np.ndarray:
    """Move material that stands steeper than rock will hold.

    One pass. Anything above the angle of repose slides to its downhill
    neighbours, which rounds ridge crests and piles talus at cliff feet. Without
    it the other two processes sharpen indefinitely and produce knife-edges no
    body could stand on and no gate would pass.
    """
    limit = math.tan(math.radians(talus_degrees)) * cell_m
    moved = np.zeros_like(height)
    excesses = []
    total = np.zeros_like(height)
    largest = np.zeros_like(height)
    for dr, dc, distance in _NEIGHBOURS:
        difference = height - _shift(height, dr, dc)
        excess = np.maximum(difference - limit * distance, 0.0)
        excesses.append(excess)
        total += excess
        largest = np.maximum(largest, excess)
    safe = np.where(total > 0.0, total, 1.0)

    # **Move a fraction of the largest excess, not of their sum.** Summing
    # across eight neighbours lets a cell shed more than any single height
    # difference justifies, so material sloshes back and forth with growing
    # amplitude: measured, a cliff run to rest reached 1e11 metres instead of
    # settling. Capping at half the steepest single drop is the standard
    # formulation and is unconditionally stable, because no cell can overshoot
    # past its lowest neighbour.
    amount = rate * 0.5 * largest
    for (dr, dc, _distance), excess in zip(_NEIGHBOURS, excesses):
        share = amount * (excess / safe)
        moved -= share
        moved += _shift(share, -dr, -dc)
    return height + moved


def _ice_source(height: np.ndarray, ice_line_m: float) -> np.ndarray:
    """How much ice each cell contributes: none below the equilibrium line.

    This is why glacial troughs start partway up a mountain instead of running
    to the sea. Below the line the same terrain is being cut by rivers, and the
    junction between the two is one of the most recognisable things about a
    glaciated range.
    """
    return np.maximum(height - ice_line_m, 0.0)


def _morphology(values: np.ndarray, radius: int, op) -> np.ndarray:
    """Iterated four-neighbour min or max -- a lower envelope or a dilation."""
    out = values.copy()
    for _ in range(max(radius, 0)):
        merged = out.copy()
        for dr, dc, _distance in _NEIGHBOURS[:4]:
            merged = op(merged, _shift(out, dr, dc))
        out = merged
    return out


def _widen(values: np.ndarray, radius: int) -> np.ndarray:
    """Separable box mean -- the lateral reach of the ice."""
    if radius < 1:
        return values
    padded = np.pad(values, radius, mode="edge")
    window = 2 * radius + 1
    cumulative = np.cumsum(padded, axis=0)
    rows = cumulative[window - 1 :, :].copy()
    rows[1:] -= cumulative[: -window, :]
    cumulative = np.cumsum(rows, axis=1)
    out = cumulative[:, window - 1 :].copy()
    out[:, 1:] -= cumulative[:, : -window]
    return out / float(window * window)


def _python_erode(
    height: np.ndarray,
    profile: ErosionProfile | str,
    *,
    cell_m: float,
    protect: np.ndarray | None = None,
) -> tuple[np.ndarray, dict]:
    """Run the three processes and return the eroded surface with a report.

    `protect` is ground the erosion may not touch -- lane corridors and landmark
    pads, which the compiler has already graded and which a stream-power term
    would happily cut a gully through.
    """
    if isinstance(profile, str):
        if profile not in PROFILES:
            raise ValueError(
                "unknown erosion profile %r; choose one of %s"
                % (profile, sorted(PROFILES))
            )
        profile = PROFILES[profile]

    original = height.astype(np.float64, copy=True)
    working = original.copy()
    floor, ceiling = float(original.min()), float(original.max())
    relief = max(ceiling - floor, 1e-6)
    # A height percentile, not a fraction of relief.
    ice_line_m = float(np.percentile(original, profile.ice_line * 100.0))

    # Lateral reach of the ice, in cells. This is the number that turns a V into
    # a U, so it is derived from the world's own scale rather than fixed: on a
    # coarse grid a three-cell blur is a wide valley, on a fine one it is a
    # scratch.
    side = max(height.shape)
    # A glacier occupies a valley, not a county. Dilating by a seventh of the
    # map from every trunk cell produced a corridor covering 100% of the world,
    # so the planation applied everywhere and flattened it -- relief fell from
    # 73 m to 33 m on the first real run.
    widening = max(2, int(round(profile.lateral_widening * side / 20.0)))

    # How much the whole run is allowed to remove, spread over its steps. A
    # range that loses a fifth of its relief to ice is heavily glaciated; one
    # that loses two percent has not been touched, which is what the first
    # calibration produced (measured: 2.55% over 64 iterations, and the valley
    # cross-section was unchanged to four decimal places).
    step_budget = relief * 0.30 / max(profile.iterations, 1)

    # Per-step rate that compounds to the declared planation over the whole run.
    # Removing a fraction of the remaining wedge each step is geometric, so the
    # run-level number is the only one an author can reason about.
    planation = min(max(profile.planation, 0.0), 0.999)
    glacial_rate = profile.glacial_strength * (
        1.0 - math.pow(1.0 - planation, 1.0 / max(profile.iterations, 1))
    )

    glacial_total = 0.0
    fluvial_total = 0.0
    for step in range(profile.iterations):
        gradient_r, gradient_c = np.gradient(working, cell_m)
        slope = np.hypot(gradient_r, gradient_c)

        # --- fluvial: stream power, E = K * A^m * S^n ---
        water = _python_flux_field(working)
        # Scaled by relief per iteration, so a coefficient means "this much of
        # the world's own relief per step" rather than "this many metres" --
        # which would erode a 600 m range by the same absolute amount as a 60 m
        # one and flatten the small map while barely marking the large one.
        water_normalised = water / max(float(water.max()), 1e-9)
        stream = (
            profile.stream_power
            * step_budget
            * np.power(water_normalised, 0.45)
            * np.power(np.clip(slope, 0.0, 3.0), 0.9)
        )

        # --- glacial: remove the wedge inside the glacier's reach ---
        #
        # **Erosion proportional to ice flux makes a sharper V, not a U.** Flux
        # peaks on the valley axis, so scaling erosion by it deepens the axis
        # and steepens the walls -- measured, it drove the valley form ratio
        # *down* from 0.340 to 0.284 while fluvial alone left it at 0.326.
        #
        # A glacier does something else. It occupies a corridor of a definite
        # width and planes its whole bed toward one level, so what it removes is
        # the wedge of rock standing above the valley floor *within that
        # corridor*. Cutting proportional to height-above-thalweg does exactly
        # that, and it is what turns a V into a U: the floor widens while the
        # walls outside the ice are untouched.
        #
        # `lateral_widening` is therefore a corridor width, and it is most of
        # the difference between the three characters -- measured at this grid
        # size the form ratio goes 0.305 / 0.368 / 0.474 as the corridor widens,
        # against 0.340 for an unglaciated V and 0.633 for a textbook U.
        ice = _python_flux_field(working, source=_ice_source(working, ice_line_m))
        ice_peak = float(ice.max())
        ice_normalised = ice / ice_peak if ice_peak > 0.0 else ice
        # Tapered at the margins, not a hard edge. A binary corridor cuts to
        # full depth right up to its boundary and leaves a step there -- the
        # 99th-percentile slope of a Highland trough came out *steeper* than an
        # Andean one, which is backwards, and the cause was the corridor wall
        # rather than the rock. Real ice thins toward its margins and so does
        # what it removes.
        corridor = _morphology(
            (ice_normalised > 0.02).astype(np.float64), widening, np.maximum
        )
        corridor = _widen(corridor, max(2, widening // 3))
        thalweg = _morphology(working, widening + 4, np.minimum)
        above_floor = np.clip(working - thalweg, 0.0, None)
        glacier = glacial_rate * corridor * above_floor

        # --- cirques: over-deepening where the ice network begins ---
        # High ground carrying moderate flux is a collecting bowl, not a trunk
        # valley, and it hollows out rather than running away.
        headward = np.clip((working - ice_line_m) / (relief * 0.5), 0.0, 1.0)
        cirque = (
            profile.cirque_strength
            * step_budget
            * 0.9
            * headward
            * np.clip(ice_normalised * 4.0, 0.0, 1.0)
            * (1.0 - ice_normalised)
        )
        cirque = _widen(cirque, max(1, widening + 1))

        removed = stream + glacier + cirque
        if protect is not None:
            removed = np.where(protect, 0.0, removed)
        # Never cut below the original floor: erosion redistributes relief, it
        # does not excavate the world downward forever.
        working = np.maximum(working - removed, floor - 2.0)

        glacial_total += float(glacier.sum())
        fluvial_total += float(stream.sum())

        # Thermal runs every step: it is the term that keeps the other two from
        # sharpening into geometry nothing can stand on.
        working = _python_thermal_erosion(
            working, cell_m=cell_m, talus_degrees=profile.talus_degrees
        )
        if protect is not None:
            working = np.where(protect, original, working)

    # **Cap the total.** Erosion redistributes relief; it does not delete a
    # mountain range. Without this the run compounds into whatever the
    # parameters happen to imply -- measured on the real world, relief fell from
    # 73 m to 28 m and a walkable slope reached grade 25 against a limit of 12.
    # A cap is not a substitute for calibration, but it is the difference
    # between "under-eroded" and "the map is gone".
    # Settle first, then cap -- in that order, and the order is the point.
    # Capping before settling lets the thermal pass move material *past* the cap
    # again, because sliding does not know a limit was imposed: measured, the
    # capped run came back at 47 m of lowering against a 13 m cap and the
    # maximum grade went from 11.1 back to 23.9. The cap has to be the last word
    # or it is not a cap.
    for _ in range(8):
        working = _python_thermal_erosion(
            working, cell_m=cell_m, talus_degrees=profile.talus_degrees, rate=0.5
        )
        if protect is not None:
            working = np.where(protect, original, working)

    # **Cap the total.** Erosion redistributes relief; it does not delete a
    # mountain range. Without this the run compounds into whatever the
    # parameters happen to imply -- measured on the real world, relief fell from
    # 73 m to 28 m and a walkable slope reached grade 25 against a limit of 12.
    ceiling_cut = relief * 0.15
    working = np.maximum(working, original - ceiling_cut)
    if protect is not None:
        working = np.where(protect, original, working)

    delta = working - original
    report = {
        "profile": profile.key,
        "iterations": profile.iterations,
        "ice_line_m": round(ice_line_m, 2),
        "lateral_widening_cells": widening,
        "mean_lowering_m": round(float(-delta.mean()), 4),
        "maximum_lowering_m": round(float(-delta.min()), 3),
        "maximum_raising_m": round(float(delta.max()), 3),
        "glacial_share": round(
            glacial_total / max(glacial_total + fluvial_total, 1e-9), 4
        ),
        "relief_before_m": round(relief, 2),
        "relief_after_m": round(float(working.max() - working.min()), 2),
    }
    return working, report


def _python_valley_cross_section(height: np.ndarray, row: int) -> dict:
    """Measure whether a valley is U-shaped or V-shaped.

    Emitted so "it looks glacial" can be checked rather than asserted.

    **The ratio of width near the floor to width near the rim**, because the
    obvious measure does not work: counting cells within the bottom tenth of
    relief is relief-*relative*, so a V eroded uniformly smaller is still a V
    and the number does not move. That measure reported an identical 0.0769
    across three different glacial formulations, two of which were wrong, and it
    was the metric that was broken rather than all three attempts.

    A V widens linearly with height, so its 25%-width is a third of its
    75%-width. A U is nearly as wide at the floor as at the rim. Measured here:
    0.34 for an unglaciated V, 0.63 for a textbook U, 0.71 for an Alpine trough.
    """
    line = height[row].astype(np.float64)
    low, high = float(line.min()), float(line.max())
    if high - low < 1e-6:
        return {"form_ratio": 0.0, "shape": "flat", "relief_m": 0.0}
    near_floor = float((line <= low + (high - low) * 0.25).sum())
    near_rim = float((line <= low + (high - low) * 0.75).sum())
    ratio = near_floor / max(near_rim, 1e-9)
    return {
        "form_ratio": round(ratio, 4),
        # Half way between a clean V and a clean U, which is where a partly
        # glaciated valley honestly sits.
        "shape": "u" if ratio >= 0.48 else "v",
        "relief_m": round(high - low, 3),
    }


class ErosionWorkerError(RuntimeError):
    """The Julia numerical authority rejected or failed a worker request."""

    def __init__(self, code: str, detail: str) -> None:
        self.code = code
        self.detail = detail
        super().__init__(f"{code}: {detail}")


@dataclass(frozen=True)
class ErosionWorkerReceipt:
    """Transport provenance minted by the Rust semantic-kernel supervisor."""

    schema: str
    job_index: int
    operation: str
    project_sha256: str
    request_sha256: str
    response_sha256: str
    result_sha256: str | None
    script_sha256: str
    solver_image: str
    worker_pid: int
    receipt_sha256: str

    def payload(self) -> dict[str, object]:
        """Return the exact Rust-minted receipt body, excluding its outer digest."""
        return {
            "job_index": self.job_index,
            "operation": self.operation,
            "project_sha256": self.project_sha256,
            "request_sha256": self.request_sha256,
            "response_sha256": self.response_sha256,
            "result_sha256": self.result_sha256,
            "schema": self.schema,
            "script_sha256": self.script_sha256,
            "solver_image": self.solver_image,
            "worker_pid": self.worker_pid,
        }


_WORKSPACE_ROOT = Path(__file__).resolve().parents[1]
_EROSION_REQUEST_SCHEMA = "codeweald.erosion-request/v1"
_EROSION_RESULT_SCHEMA = "codeweald.erosion-result/v1"
_EROSION_RECEIPT_SCHEMA = "luxel.erosion-worker-receipt/v1"
_SUPERVISOR_IDLE_SECONDS = 120.0
_SUPERVISOR_MAX_JOBS = 32
_SUPERVISOR_REQUEST_TIMEOUT_SECONDS = 300.0
_SUPERVISOR_MAX_LINE_BYTES = 1_000_000
_SUPERVISOR_STDERR_TAIL_BYTES = 8_192


def _erosion_backend() -> str:
    backend = os.environ.get("LUXEL_EROSION_BACKEND", "julia").strip().lower()
    if backend not in {"julia", "python"}:
        raise ValueError(
            "LUXEL_EROSION_BACKEND must be 'julia' or 'python', got %r" % backend
        )
    return backend


def _sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _canonical_json(value: object) -> str:
    return json.dumps(
        value,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
        allow_nan=False,
    )


def _canonical_digest(value: object) -> str:
    return "sha256:" + _sha256_bytes(_canonical_json(value).encode("utf-8"))


def _is_sha256(value: object, *, prefixed: bool = False) -> bool:
    if not isinstance(value, str):
        return False
    digest = value[7:] if prefixed and value.startswith("sha256:") else value
    return bool(re.fullmatch(r"[0-9a-f]{64}", digest)) and (
        not prefixed or value.startswith("sha256:")
    )


def _resolve_workspace_path(value: str | None, default: Path) -> Path:
    path = Path(value).expanduser() if value else default
    if not path.is_absolute():
        path = _WORKSPACE_ROOT / path
    return path.resolve()


def _supervisor_configuration() -> tuple[list[str], tuple[tuple[object, ...], ...]]:
    julia = os.environ.get("LUXEL_EROSION_JULIA") or shutil.which("julia") or "julia"
    project = _resolve_workspace_path(
        os.environ.get("LUXEL_TERRAIN_PROJECT"), _WORKSPACE_ROOT / "terrain_lab"
    )
    worker = _resolve_workspace_path(
        os.environ.get("LUXEL_EROSION_WORKER"), project / "bin" / "erosion_worker.jl"
    )
    manifest = _resolve_workspace_path(
        os.environ.get("LUXEL_TERRAIN_MANIFEST"), project / "Manifest.toml"
    )
    configured_kernel = os.environ.get("LUXEL_SEMANTIC_KERNEL")
    if configured_kernel:
        command = shlex.split(configured_kernel)
        if not command:
            raise ErosionWorkerError("worker_unavailable", "LUXEL_SEMANTIC_KERNEL is empty")
    else:
        command = [
            "cargo",
            "run",
            "--quiet",
            "--offline",
            "--manifest-path",
            str(_WORKSPACE_ROOT / "world_core" / "Cargo.toml"),
            "-p",
            "luxel-semantic-kernel",
            "--",
        ]
    command.extend(
        [
            "erosion-supervisor",
            "--julia",
            julia,
            "--project",
            str(project),
            "--manifest",
            str(manifest),
            "--worker",
            str(worker),
        ]
    )

    fingerprints: list[tuple[object, ...]] = []
    for path in (project / "Project.toml", manifest, worker):
        try:
            stat = path.stat()
        except OSError as error:
            raise ErosionWorkerError(
                "worker_unavailable", "cannot inspect supervisor input %s: %s" % (path, error)
            ) from error
        fingerprints.append((str(path), stat.st_size, stat.st_mtime_ns))
    return command, tuple(fingerprints)


class _ErosionSupervisorClient:
    """Bounded newline client for the Rust supervisor's JSON event stream."""

    def __init__(self, command: list[str], fingerprint: tuple[tuple[object, ...], ...]) -> None:
        try:
            self.process = subprocess.Popen(
                command,
                cwd=_WORKSPACE_ROOT,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                bufsize=0,
                start_new_session=(os.name == "posix"),
            )
        except OSError as error:
            raise ErosionWorkerError(
                "worker_unavailable", "could not start the Rust erosion supervisor: %s" % error
            ) from error
        assert self.process.stdin is not None
        assert self.process.stdout is not None
        assert self.process.stderr is not None
        self.fingerprint = fingerprint
        self.events: queue.Queue[bytes | BaseException | None] = queue.Queue()
        self._stderr = bytearray()
        self._stderr_lock = threading.Lock()
        self._ready: dict[str, object] | None = None
        self.jobs = 0
        self.last_used = time.monotonic()
        self._stdout_thread = threading.Thread(
            target=self._read_stdout, name="luxel-erosion-supervisor-stdout", daemon=True
        )
        self._stderr_thread = threading.Thread(
            target=self._read_stderr, name="luxel-erosion-supervisor-stderr", daemon=True
        )
        self._stdout_thread.start()
        self._stderr_thread.start()

    def _read_stdout(self) -> None:
        assert self.process.stdout is not None
        try:
            while True:
                line = self.process.stdout.readline(_SUPERVISOR_MAX_LINE_BYTES + 1)
                if not line:
                    self.events.put(None)
                    return
                if len(line) > _SUPERVISOR_MAX_LINE_BYTES + 1 or not line.endswith(b"\n"):
                    self.events.put(
                        ErosionWorkerError(
                            "worker_protocol", "supervisor event exceeds the line limit or is truncated"
                        )
                    )
                    self.events.put(None)
                    return
                self.events.put(line[:-1])
        except BaseException as error:
            self.events.put(error)
            self.events.put(None)

    def _read_stderr(self) -> None:
        assert self.process.stderr is not None
        try:
            while True:
                chunk = self.process.stderr.read(1024)
                if not chunk:
                    return
                with self._stderr_lock:
                    self._stderr.extend(chunk)
                    overflow = len(self._stderr) - _SUPERVISOR_STDERR_TAIL_BYTES
                    if overflow > 0:
                        del self._stderr[:overflow]
        except OSError:
            return

    def _stderr_excerpt(self) -> str:
        with self._stderr_lock:
            return bytes(self._stderr).decode("utf-8", errors="replace").strip()

    def _next_event(self) -> dict[str, object]:
        try:
            item = self.events.get(timeout=_SUPERVISOR_REQUEST_TIMEOUT_SECONDS)
        except queue.Empty as error:
            self.close(graceful=False)
            raise ErosionWorkerError(
                "worker_timeout",
                "Rust erosion supervisor did not return an event within %.0f seconds"
                % _SUPERVISOR_REQUEST_TIMEOUT_SECONDS,
            ) from error
        if isinstance(item, BaseException):
            raise ErosionWorkerError("worker_protocol", str(item)) from item
        if item is None:
            detail = self._stderr_excerpt() or "supervisor closed stdout"
            raise ErosionWorkerError("worker_exited", detail)
        try:
            event = json.loads(item.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise ErosionWorkerError("worker_protocol", "supervisor event is not valid UTF-8 JSON") from error
        if not isinstance(event, dict):
            raise ErosionWorkerError("worker_protocol", "supervisor event must be a JSON object")
        return event

    def _accept_ready(self, event: dict[str, object]) -> None:
        expected = {
            "cold_ms",
            "event",
            "manifest_sha256",
            "pid",
            "project_sha256",
            "script_sha256",
            "solver_image",
            "wrapper_version",
        }
        if set(event) != expected or event.get("event") != "worker_ready":
            raise ErosionWorkerError("worker_protocol", "first supervisor event must be a typed worker_ready")
        if (
            not isinstance(event.get("pid"), int)
            or isinstance(event.get("pid"), bool)
            or event["pid"] <= 0
            or not isinstance(event.get("cold_ms"), int)
            or event["cold_ms"] < 0
            or event.get("wrapper_version") != "luxel.erosion-worker/v1"
            or not all(
                _is_sha256(event.get(key))
                for key in ("manifest_sha256", "project_sha256", "script_sha256")
            )
            or not _is_sha256(event.get("solver_image"), prefixed=True)
        ):
            raise ErosionWorkerError("worker_protocol", "worker_ready identity fields are malformed")
        self._ready = event

    def _parse_result(
        self, event: dict[str, object], request: dict[str, object]
    ) -> tuple[dict[str, object], ErosionWorkerReceipt]:
        expected = {
            "event",
            "job_index",
            "job_us",
            "phase",
            "pid",
            "receipt",
            "receipt_sha256",
            "response",
        }
        if event.get("event") == "failure":
            code = event.get("code")
            detail = event.get("detail")
            raise ErosionWorkerError(
                code if isinstance(code, str) else "worker_failure",
                detail if isinstance(detail, str) else "Rust supervisor rejected the request",
            )
        if set(event) != expected or event.get("event") != "worker_result":
            raise ErosionWorkerError("worker_protocol", "supervisor event must be a typed worker_result")
        job_index = event.get("job_index")
        pid = event.get("pid")
        if (
            not isinstance(job_index, int)
            or isinstance(job_index, bool)
            or job_index != self.jobs + 1
            or not isinstance(pid, int)
            or isinstance(pid, bool)
            or pid <= 0
            or event.get("phase") != ("first_job" if job_index == 1 else "warm")
            or not isinstance(event.get("job_us"), int)
            or event["job_us"] < 0
            or self._ready is None
            or pid != self._ready.get("pid")
        ):
            raise ErosionWorkerError("worker_protocol", "worker_result job identity is inconsistent")
        body = event.get("receipt")
        if not isinstance(body, dict):
            raise ErosionWorkerError("worker_protocol", "worker_result omitted its receipt object")
        body_keys = {
            "job_index",
            "operation",
            "project_sha256",
            "request_sha256",
            "response_sha256",
            "result_sha256",
            "schema",
            "script_sha256",
            "solver_image",
            "worker_pid",
        }
        if set(body) != body_keys:
            raise ErosionWorkerError("worker_protocol", "worker receipt fields do not match its schema")
        response = event.get("response")
        if not isinstance(response, dict):
            raise ErosionWorkerError("worker_protocol", "worker_result response must be an object")
        response_sha = _canonical_digest(response)
        request_sha = _canonical_digest(request)
        receipt_sha = event.get("receipt_sha256")
        if (
            body.get("schema") != _EROSION_RECEIPT_SCHEMA
            or body.get("job_index") != job_index
            or body.get("worker_pid") != pid
            or body.get("operation") != request.get("operation")
            or body.get("request_sha256") != request_sha
            or body.get("response_sha256") != response_sha
            or body.get("script_sha256") != self._ready.get("script_sha256")
            or body.get("project_sha256") != self._ready.get("project_sha256")
            or body.get("solver_image") != self._ready.get("solver_image")
            or not _is_sha256(body.get("script_sha256"))
            or not _is_sha256(body.get("project_sha256"))
            or not _is_sha256(body.get("solver_image"), prefixed=True)
            or not _is_sha256(body.get("request_sha256"), prefixed=True)
            or not _is_sha256(body.get("response_sha256"), prefixed=True)
            or not _is_sha256(receipt_sha, prefixed=True)
        ):
            raise ErosionWorkerError("receipt_identity_mismatch", "Rust receipt does not bind this request and worker")
        result_sha = response.get("result_sha256")
        if result_sha is not None and not _is_sha256(result_sha):
            raise ErosionWorkerError("worker_protocol", "worker response result digest is malformed")
        if body.get("result_sha256") != result_sha:
            raise ErosionWorkerError("receipt_identity_mismatch", "receipt result digest does not match response")
        calculated_receipt_sha = _canonical_digest(body)
        if receipt_sha != calculated_receipt_sha:
            raise ErosionWorkerError("receipt_digest_mismatch", "Rust receipt digest does not match its body")

        self._validate_response(request, response)
        receipt = ErosionWorkerReceipt(
            schema=_EROSION_RECEIPT_SCHEMA,
            job_index=job_index,
            operation=str(body["operation"]),
            project_sha256=str(body["project_sha256"]),
            request_sha256=str(body["request_sha256"]),
            response_sha256=str(body["response_sha256"]),
            result_sha256=result_sha if isinstance(result_sha, str) else None,
            script_sha256=str(body["script_sha256"]),
            solver_image=str(body["solver_image"]),
            worker_pid=pid,
            receipt_sha256=calculated_receipt_sha,
        )
        return response, receipt

    @staticmethod
    def _validate_response(request: dict[str, object], response: dict[str, object]) -> None:
        operation = request.get("operation")
        shape = request.get("shape")
        if (
            response.get("schema") != _EROSION_RESULT_SCHEMA
            or response.get("status") != "ok"
            or response.get("operation") != operation
            or response.get("shape") != shape
        ):
            raise ErosionWorkerError("worker_protocol", "worker response schema, operation, or shape mismatched")
        base = {"schema", "status", "operation", "shape"}
        extras = {
            "erode": {"dtype", "result_sha256", "report"},
            "thermal_erosion": {"dtype", "result_sha256"},
            "flux_field": {"dtype", "result_sha256"},
            "valley_cross_section": {"measurement"},
        }.get(operation)
        if extras is None or set(response) != base | extras:
            raise ErosionWorkerError("worker_protocol", "worker response fields do not match the operation schema")
        if operation == "valley_cross_section":
            measurement = response.get("measurement")
            if not isinstance(measurement, dict) or set(measurement) != {
                "form_ratio", "shape", "relief_m"
            }:
                raise ErosionWorkerError("worker_protocol", "valley response omitted typed measurement fields")
            ratio = measurement.get("form_ratio")
            relief = measurement.get("relief_m")
            if (
                not isinstance(ratio, (int, float))
                or isinstance(ratio, bool)
                or not math.isfinite(ratio)
                or ratio < 0
                or not isinstance(relief, (int, float))
                or isinstance(relief, bool)
                or not math.isfinite(relief)
                or relief < 0
                or measurement.get("shape") not in {"u", "v", "flat"}
            ):
                raise ErosionWorkerError("worker_protocol", "valley response measurement is outside its domain")
            return
        if response.get("dtype") != "float64-le" or not _is_sha256(response.get("result_sha256")):
            raise ErosionWorkerError("worker_protocol", "worker output type or digest is malformed")
        if operation == "erode" and not isinstance(response.get("report"), dict):
            raise ErosionWorkerError("worker_protocol", "erode response omitted its report")

    def request(
        self, request: dict[str, object]
    ) -> tuple[dict[str, object], ErosionWorkerReceipt]:
        if time.monotonic() - self.last_used > _SUPERVISOR_IDLE_SECONDS:
            raise ErosionWorkerError("worker_expired", "warm supervisor exceeded its idle lifetime")
        if self.process.poll() is not None:
            raise ErosionWorkerError(
                "worker_exited", self._stderr_excerpt() or "Rust erosion supervisor exited"
            )
        encoded = _canonical_json(request).encode("utf-8")
        if not encoded or len(encoded) > _SUPERVISOR_MAX_LINE_BYTES:
            raise ErosionWorkerError("malformed_request", "erosion request exceeds the supervisor line limit")
        assert self.process.stdin is not None
        try:
            self.process.stdin.write(encoded + b"\n")
            self.process.stdin.flush()
        except OSError as error:
            raise ErosionWorkerError("worker_io", "could not write supervisor request: %s" % error) from error

        if self._ready is None:
            ready = self._next_event()
            if ready.get("event") == "failure":
                code = ready.get("code")
                detail = ready.get("detail")
                raise ErosionWorkerError(
                    code if isinstance(code, str) else "worker_failure",
                    detail if isinstance(detail, str) else "Rust supervisor failed before worker readiness",
                )
            self._accept_ready(ready)
        result_event = self._next_event()
        response, receipt = self._parse_result(result_event, request)
        self.jobs += 1
        self.last_used = time.monotonic()
        return response, receipt

    def close(self, *, graceful: bool = True) -> None:
        process = self.process
        if process.poll() is None and graceful and process.stdin is not None:
            try:
                process.stdin.close()
            except OSError:
                graceful = False
        if process.poll() is None:
            try:
                process.wait(timeout=2.0 if graceful else 0.0)
            except subprocess.TimeoutExpired:
                self._terminate_process_group(signal.SIGTERM)
                try:
                    process.wait(timeout=1.0)
                except subprocess.TimeoutExpired:
                    self._terminate_process_group(signal.SIGKILL)
                    try:
                        process.wait(timeout=1.0)
                    except subprocess.TimeoutExpired:
                        pass
        for stream in (process.stdin, process.stdout, process.stderr):
            if stream is not None and not stream.closed:
                try:
                    stream.close()
                except OSError:
                    pass

    def _terminate_process_group(self, sig: signal.Signals) -> None:
        if self.process.poll() is not None:
            return
        try:
            if os.name == "posix":
                os.killpg(self.process.pid, sig)
            elif sig == signal.SIGTERM:
                self.process.terminate()
            else:
                self.process.kill()
        except (OSError, ProcessLookupError):
            pass


_SUPERVISOR: _ErosionSupervisorClient | None = None
_SUPERVISOR_LOCK = threading.RLock()
_SUPERVISOR_BACKEND_SETTING: str | None = None


def _shutdown_erosion_supervisor() -> None:
    global _SUPERVISOR, _SUPERVISOR_BACKEND_SETTING
    with _SUPERVISOR_LOCK:
        client, _SUPERVISOR = _SUPERVISOR, None
        _SUPERVISOR_BACKEND_SETTING = None
        if client is not None:
            client.close(graceful=True)


def _discard_erosion_supervisor(client: _ErosionSupervisorClient | None = None) -> None:
    global _SUPERVISOR
    with _SUPERVISOR_LOCK:
        target = client or _SUPERVISOR
        if target is not None:
            target.close(graceful=False)
        if _SUPERVISOR is target:
            _SUPERVISOR = None


atexit.register(_shutdown_erosion_supervisor)


def _get_erosion_supervisor() -> _ErosionSupervisorClient:
    global _SUPERVISOR, _SUPERVISOR_BACKEND_SETTING
    command, fingerprint = _supervisor_configuration()
    backend_setting = os.environ.get("LUXEL_EROSION_BACKEND")
    with _SUPERVISOR_LOCK:
        if _SUPERVISOR is not None and (
            _SUPERVISOR_BACKEND_SETTING != backend_setting
            or
            _SUPERVISOR.fingerprint != fingerprint
            or _SUPERVISOR.jobs >= _SUPERVISOR_MAX_JOBS
            or time.monotonic() - _SUPERVISOR.last_used > _SUPERVISOR_IDLE_SECONDS
            or _SUPERVISOR.process.poll() is not None
        ):
            _SUPERVISOR.close(graceful=True)
            _SUPERVISOR = None
        if _SUPERVISOR is None:
            _SUPERVISOR = _ErosionSupervisorClient(command, fingerprint)
            _SUPERVISOR_BACKEND_SETTING = backend_setting
        return _SUPERVISOR


def _write_float64(path: Path, values: np.ndarray) -> str:
    encoded = np.asarray(values, dtype=np.dtype("<f8")).tobytes(order="C")
    path.write_bytes(encoded)
    return _sha256_bytes(encoded)


def _write_mask(path: Path, values: np.ndarray) -> str:
    encoded = np.asarray(values, dtype=np.uint8).tobytes(order="C")
    path.write_bytes(encoded)
    return _sha256_bytes(encoded)


def _run_erosion_worker(
    request: dict[str, object],
) -> tuple[dict[str, object], ErosionWorkerReceipt]:
    global _SUPERVISOR
    with _SUPERVISOR_LOCK:
        client = _get_erosion_supervisor()
        try:
            return client.request(request)
        except ErosionWorkerError:
            _discard_erosion_supervisor(client)
            raise


def _worker_height_result(
    path: Path,
    response: dict[str, object],
    shape: tuple[int, int],
) -> np.ndarray:
    try:
        raw = path.read_bytes()
    except OSError as error:
        raise ErosionWorkerError(
            "result_unreadable", "worker output could not be read: %s" % error
        ) from error
    expected_bytes = shape[0] * shape[1] * np.dtype("<f8").itemsize
    if len(raw) != expected_bytes:
        raise ErosionWorkerError("result_shape_mismatch", "worker output length does not match the request shape")
    expected_digest = response.get("result_sha256")
    if not isinstance(expected_digest, str) or _sha256_bytes(raw) != expected_digest:
        raise ErosionWorkerError("result_digest_mismatch", "worker output digest does not match its receipt")
    result = np.frombuffer(raw, dtype="<f8").reshape(shape).copy()
    if not np.isfinite(result).all():
        raise ErosionWorkerError("non_finite_result", "worker output contains non-finite heights")
    return result


def _profile_document(profile: ErosionProfile) -> dict[str, object]:
    return {
        "key": profile.key,
        "iterations": profile.iterations,
        "stream_power": profile.stream_power,
        "glacial_strength": profile.glacial_strength,
        "ice_line": profile.ice_line,
        "lateral_widening": profile.lateral_widening,
        "talus_degrees": profile.talus_degrees,
        "cirque_strength": profile.cirque_strength,
        "planation": profile.planation,
    }


def _julia_operation(
    operation: str,
    height: np.ndarray,
    *,
    output: bool,
    **fields: object,
) -> tuple[np.ndarray | None, dict[str, object], ErosionWorkerReceipt]:
    array = np.asarray(height, dtype=np.float64)
    if array.ndim != 2:
        raise ValueError("heightfield must be a two-dimensional array")
    if not np.isfinite(array).all():
        raise ValueError("heightfield must contain only finite values")
    with tempfile.TemporaryDirectory(prefix="luxel-erosion-") as directory:
        root = Path(directory)
        height_path = root / "height.f64"
        height_digest = _write_float64(height_path, array)
        request: dict[str, object] = {
            "schema": _EROSION_REQUEST_SCHEMA,
            "operation": operation,
            "shape": [int(array.shape[0]), int(array.shape[1])],
            "height_path": str(height_path),
            "height_sha256": height_digest,
        }
        request.update(fields)
        output_path = root / "result.f64"
        if output:
            request["output_path"] = str(output_path)
        try:
            response, receipt = _run_erosion_worker(request)
            result = _worker_height_result(output_path, response, array.shape) if output else None
            return result, response, receipt
        except ErosionWorkerError:
            _discard_erosion_supervisor()
            raise


def flux_field(
    height: np.ndarray,
    *,
    source: np.ndarray | None = None,
    exponent: float = 1.15,
    return_receipt: bool = False,
) -> np.ndarray | tuple[np.ndarray, ErosionWorkerReceipt]:
    """Run the flux solver through Julia; Python remains a transport facade."""
    if _erosion_backend() == "python":
        _shutdown_erosion_supervisor()
        if return_receipt:
            raise ErosionWorkerError(
                "receipt_unavailable", "the explicit legacy Python backend cannot provide a Rust receipt"
            )
        return _python_flux_field(height, source=source, exponent=exponent)
    source_path_fields: dict[str, object] = {"source_path": None, "source_sha256": None}
    with tempfile.TemporaryDirectory(prefix="luxel-erosion-source-") as directory:
        if source is not None:
            source_array = np.asarray(source, dtype=np.float64)
            if source_array.shape != np.asarray(height).shape:
                raise ValueError("source and heightfield dimensions differ")
            source_path = Path(directory) / "source.f64"
            source_path_fields = {
                "source_path": str(source_path),
                "source_sha256": _write_float64(source_path, source_array),
            }
        with tempfile.TemporaryDirectory(prefix="luxel-erosion-output-") as output_directory:
            output_path = Path(output_directory) / "result.f64"
            result, _response, receipt = _julia_operation(
                "flux_field",
                height,
                output=True,
                output_path=str(output_path),
                exponent=float(exponent),
                **source_path_fields,
            )
            assert result is not None
            return (result, receipt) if return_receipt else result


def thermal_erosion(
    height: np.ndarray,
    *,
    cell_m: float,
    talus_degrees: float,
    rate: float = 0.35,
    return_receipt: bool = False,
) -> np.ndarray | tuple[np.ndarray, ErosionWorkerReceipt]:
    """Run the thermal solver through Julia; Python remains a transport facade."""
    if _erosion_backend() == "python":
        _shutdown_erosion_supervisor()
        if return_receipt:
            raise ErosionWorkerError(
                "receipt_unavailable", "the explicit legacy Python backend cannot provide a Rust receipt"
            )
        return _python_thermal_erosion(
            height, cell_m=cell_m, talus_degrees=talus_degrees, rate=rate
        )
    with tempfile.TemporaryDirectory(prefix="luxel-erosion-output-") as directory:
        output_path = Path(directory) / "result.f64"
        result, _response, receipt = _julia_operation(
            "thermal_erosion",
            height,
            output=True,
            output_path=str(output_path),
            cell_m=float(cell_m),
            talus_degrees=float(talus_degrees),
            rate=float(rate),
        )
        assert result is not None
        return (result, receipt) if return_receipt else result


def erode(
    height: np.ndarray,
    profile: ErosionProfile | str,
    *,
    cell_m: float,
    protect: np.ndarray | None = None,
) -> tuple[np.ndarray, dict]:
    """Run canonical erosion in Julia; use Python only by explicit opt-in."""
    if isinstance(profile, str):
        if profile not in PROFILES:
            raise ValueError(
                "unknown erosion profile %r; choose one of %s"
                % (profile, sorted(PROFILES))
            )
        profile = PROFILES[profile]
    if not isinstance(profile, ErosionProfile):
        raise TypeError("profile must be an ErosionProfile or known profile name")
    if _erosion_backend() == "python":
        _shutdown_erosion_supervisor()
        return _python_erode(height, profile, cell_m=cell_m, protect=protect)
    with tempfile.TemporaryDirectory(prefix="luxel-erosion-output-") as directory:
        protect_path_fields: dict[str, object] = {
            "protect_path": None,
            "protect_sha256": None,
        }
        if protect is not None:
            protect_array = np.asarray(protect, dtype=bool)
            if protect_array.shape != np.asarray(height).shape:
                raise ValueError("protection mask and heightfield dimensions differ")
            protect_path = Path(directory) / "protect.mask"
            protect_path_fields = {
                "protect_path": str(protect_path),
                "protect_sha256": _write_mask(protect_path, protect_array),
            }
        output_path = Path(directory) / "result.f64"
        result, response, receipt = _julia_operation(
            "erode",
            height,
            output=True,
            output_path=str(output_path),
            cell_m=float(cell_m),
            profile=_profile_document(profile),
            **protect_path_fields,
        )
        assert result is not None
        report = response.get("report")
        if not isinstance(report, dict):
            raise ErosionWorkerError("worker_protocol", "erode response omitted its report")
        report = dict(report)
        report["worker_receipt"] = receipt.payload()
        report["worker_receipt_sha256"] = receipt.receipt_sha256
        return result, report


def valley_cross_section(
    height: np.ndarray, row: int, *, return_receipt: bool = False
) -> dict | tuple[dict, ErosionWorkerReceipt]:
    """Measure a cross-section through Julia's numerical worker."""
    if _erosion_backend() == "python":
        _shutdown_erosion_supervisor()
        if return_receipt:
            raise ErosionWorkerError(
                "receipt_unavailable", "the explicit legacy Python backend cannot provide a Rust receipt"
            )
        return _python_valley_cross_section(height, row)
    _result, response, receipt = _julia_operation(
        "valley_cross_section",
        height,
        output=False,
        row_index=int(row),
    )
    measurement = response.get("measurement")
    if not isinstance(measurement, dict):
        raise ErosionWorkerError("worker_protocol", "valley response omitted its measurement")
    return (measurement, receipt) if return_receipt else measurement
