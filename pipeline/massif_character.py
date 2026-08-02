"""What *kind* of mountains a world has (systems roadmap S18).

Every landform in WGE has been the same shape at different scales: smooth
fractal noise, jittered. That produces hills. It does not produce the Alps,
because Alpine form is not a noise function -- it is the signature of a
particular erosion history, and so is every other range worth naming.

Reviewed 2026-08-02 against Lauterbrunnen: the border read as "a square basin",
and once that was fixed it still read as a rampart rather than a range. The
diagnosis was not tuning. It was that the shaping system had no concept of
mountain type at all.

**The one technique that matters here is ridging.** Ordinary fractal noise is
smooth-crested: its maxima are rounded, because they are sums of rounded bumps.
Folding it about its midpoint -- `1 - |2n - 1|` -- turns every maximum into a
crease, and the creases connect into continuous ridgelines the way real
watersheds do. Weighting each octave by the one above it (a multifractal) then
concentrates detail on the ridges and leaves the valleys smooth, which is what
erosion does: rock is attacked hardest where it is most exposed.

The three characters differ in how sharp that fold is, how much relief they
carry, and how much the valleys are flattened afterwards:

- **alps** -- deep glacial troughs with flat floors and near-vertical walls,
  sharp arêtes between them, high relief over short distance
- **highlands** -- the same glacial process run longer on softer rock: rounded,
  lower, broad corries and wide straths. Also the right model for worn ranges
  like the Appalachians, at a lower relief budget
- **andes** -- granite spires and towers; the sharpest ridging, the highest
  relief, the least valley flattening

Deliberately not modelled yet: karst needle mountains, which are a genuinely
different process (dissolution, not glaciation) and are not served by adjusting
these parameters. Noted rather than faked.

No `bpy`, no batch, no I/O -- pure arithmetic over a grid, so it is testable
without Blender or a compiled world (D3's fix direction).
"""

from __future__ import annotations

from dataclasses import dataclass

import numpy as np


@dataclass(frozen=True)
class MassifCharacter:
    """One range's shaping parameters.

    `ridge_sharpness` is the exponent on the folded noise: higher pinches the
    crests into arêtes, lower leaves them as whalebacks. `valley_flatten` is
    how hard the low ground is pulled toward its floor, which is what makes a
    glacial trough U-shaped instead of V-shaped -- the single most recognisable
    difference between a glaciated range and a river-cut one.
    """

    key: str
    ridge_sharpness: float
    valley_flatten: float
    relief_scale: float
    octaves: tuple[int, ...]
    gain: float
    summit_bias: float


CHARACTERS: dict[str, MassifCharacter] = {
    "alps": MassifCharacter(
        key="alps",
        ridge_sharpness=2.1,
        valley_flatten=0.68,
        relief_scale=1.0,
        octaves=(2, 4, 9, 19),
        gain=0.52,
        summit_bias=0.30,
    ),
    "highlands": MassifCharacter(
        key="highlands",
        # Barely folded: Highland tops are whalebacks, not blades.
        ridge_sharpness=1.15,
        valley_flatten=0.42,
        relief_scale=0.62,
        octaves=(2, 4, 8),
        gain=0.44,
        summit_bias=0.10,
    ),
    "andes": MassifCharacter(
        key="andes",
        # The sharpest fold and almost no valley flattening: towers and
        # couloirs rather than troughs.
        ridge_sharpness=3.0,
        valley_flatten=0.22,
        relief_scale=1.25,
        octaves=(2, 5, 11, 23, 41),
        gain=0.58,
        summit_bias=0.44,
    ),
}

DEFAULT_CHARACTER = "alps"


def _value_noise(shape: tuple[int, int], cells: int, rng: np.random.Generator) -> np.ndarray:
    """Smooth value noise by bilinear upsampling of a coarse lattice.

    Deliberately not PIL-based: this module stays importable without an image
    library so it can be unit-tested anywhere the compiler runs.
    """
    rows, columns = shape
    lattice = rng.random((max(2, cells + 1), max(2, cells + 1)))
    row_position = np.linspace(0.0, lattice.shape[0] - 1.0, rows)
    column_position = np.linspace(0.0, lattice.shape[1] - 1.0, columns)

    def axis_weights(position: np.ndarray, size: int):
        low = np.clip(np.floor(position).astype(int), 0, size - 1)
        high = np.clip(low + 1, 0, size - 1)
        t = position - low
        # Smoothstep the interpolant, or the lattice shows as a visible grid of
        # creases that ridging then amplifies into a plaid mountain range.
        return low, high, t * t * (3.0 - 2.0 * t)

    row_low, row_high, row_t = axis_weights(row_position, lattice.shape[0])
    column_low, column_high, column_t = axis_weights(column_position, lattice.shape[1])
    top = lattice[row_low, :] * (1 - row_t)[:, None] + lattice[row_high, :] * row_t[:, None]
    return top[:, column_low] * (1 - column_t)[None, :] + top[:, column_high] * column_t[None, :]


def ridged_multifractal(
    shape: tuple[int, int], rng: np.random.Generator, character: MassifCharacter
) -> np.ndarray:
    """Ridged multifractal relief in [0, 1].

    Each octave is folded about its midpoint so its maxima become creases, then
    weighted by the accumulated relief above it. That weighting is what makes it
    *multi*fractal and is the whole reason it reads as a range: detail collects
    on the ridgelines and the valleys stay smooth, instead of the same roughness
    being sprayed everywhere at every altitude.
    """
    total = np.zeros(shape, dtype=np.float64)
    weight = np.ones(shape, dtype=np.float64)
    amplitude = 1.0
    normaliser = 0.0
    for cells in character.octaves:
        noise = _value_noise(shape, cells, rng)
        ridge = 1.0 - np.abs(2.0 * noise - 1.0)
        ridge = np.power(np.clip(ridge, 0.0, 1.0), character.ridge_sharpness)
        total += ridge * amplitude * weight
        normaliser += amplitude
        # Higher ground gets more detail; hollows get less.
        weight = np.clip(weight * (0.35 + character.gain * ridge), 0.0, 1.0)
        amplitude *= character.gain
    relief = total / max(normaliser, 1e-9)
    return np.clip(relief, 0.0, 1.0)


def shape_relief(
    shape: tuple[int, int], rng: np.random.Generator, character: str | MassifCharacter
) -> np.ndarray:
    """Normalised massif relief in [0, 1] for a named character.

    Ridging, then valley flattening. The order matters: flattening the low
    ground *after* ridging carves the U-profile out of terrain that already has
    a ridgeline, which is the glacial sequence. Flattening first would just
    produce a smooth surface with creases drawn on it.
    """
    if isinstance(character, str):
        if character not in CHARACTERS:
            raise ValueError(
                "unknown massif character %r; choose one of %s"
                % (character, sorted(CHARACTERS))
            )
        character = CHARACTERS[character]

    relief = ridged_multifractal(shape, rng, character)
    # Pull low ground toward the floor. `valley_flatten` at 1 would give a
    # perfectly flat floor meeting vertical walls; at 0 the profile stays as
    # the raw ridged noise left it.
    flattened = np.power(relief, 1.0 + 2.4 * character.valley_flatten)
    # Then push the summits back up, so flattening the valleys does not also
    # cost the range its height.
    peaked = flattened + character.summit_bias * np.power(relief, 3.0)
    return np.clip(peaked / max(float(peaked.max()), 1e-9), 0.0, 1.0)
