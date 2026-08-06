"""Erosion: the process that makes terrain look like somewhere (systems S3).

Every landform WGE builds is noise with a silhouette. Ridged multifractal (S18)
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
from dataclasses import dataclass

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


def flux_field(
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


def thermal_erosion(
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


def erode(
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
        water = flux_field(working)
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
        ice = flux_field(working, source=_ice_source(working, ice_line_m))
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
        working = thermal_erosion(
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
        working = thermal_erosion(
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


def valley_cross_section(height: np.ndarray, row: int) -> dict:
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
