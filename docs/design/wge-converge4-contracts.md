# WGE CONVERGE-4 Contracts

Status: **OPEN, 2026-10-06.** N-3 implemented and measured; awaiting Matt's review (`ab-converge4-n3c`). Order proposed below; not yet approved.
Source: "Carried into CONVERGE-4" in `WGE_CONVERGE3_CONTRACTS.md`, and the
landscape parity plan (`docs/design/2026-10-05_landscape-parity-plan.md`).

CONVERGE-3 finished the close frame and filled the world. CONVERGE-4 gives the
world a horizon: a km-scale mountain backdrop that the atmosphere can act on
(N-3, which also closes N-1's long-open ridge-contrast target). Shadows and
contact grounding then catch up with the larger world (L-1, N-8), and the
ground cover is chosen for the place rather than by availability (B-1).

Identity discipline as before: every new axis is absent by default and
byte-identical when absent. All CONVERGE-4 content lands under a new arm,
`converge4` (= converge3 plus these contracts). converge3 and every earlier arm
stay byte-identical; `tools/verify_identity.py` now includes
`ab-converge3-final`, so it covers 131 frames.

Proposed order: **N-3 → L-1 → N-8 → B-1.** N-3 first because it decides the
world's scale, and the audit is explicit that cascades buy nothing before the
world extends (`WGE_GRAPHICS_CONVERGENCE_AUDIT.md` §G). L-1 next because the
N-7 review's "some of the shadows from the trees seem kinda off" is the most
visible remaining defect. N-8 is measured after scatter, as CONVERGE-3
decided. B-1 is content and can move earlier if Matt prefers.

---

## 0. What the code says (checked 2026-10-05)

**Cameras see about 12° of sky.** All three views pitch 11-12° down with a
46-50° vertical field of view (`lib.rs` campaign2 view table), so the top of
the frame is ~12° above the horizon. A backdrop peak that should leave sky
above it subtends at most ~6-8°: height / distance ≈ 0.12, e.g. 1.2-1.8 km of
relief at 10-15 km.

**Depth is D32F, not reversed, near plane 0.1 m.** Depth resolution at
distance z is about `ulp(1) z² / near`: ~1.4 m at 1.5 km (today's far plane),
~15 m at 5 km, ~60 m at 10 km. Raising the far plane does not change near-field
precision. Ridges hundreds of metres apart resolve; no reversed-Z work needed.

**The shadow fit is view-relative** (`_view_fitted_shadow_frame`,
`LavaAdapter.jl`), 60 m for the converge arms. km-scale instances do not dilute
shadow resolution; they fall outside the shadow slice and read as lit, which is
correct for a sun at 25° (a 1.5 km peak 10 km away shadows the world only below
~8.5°).

**Packet budget.** converge3 is 61.8 MB against the 128 MiB worker frame bound.
Textures travel as RGBA8 plus mips, base64; meshes inline. The backdrop budget
is ~25-30 MB: about 260 k vertices and four 1024² textures.

**Gaea.** Headless builds work from an agent session (`pipeline/gaea_build.py`,
landscape plan P1). Eroded fields are not byte-reproducible, so a Gaea field is
a source artifact pinned by pixel digest.

---

## 1. N-3 — The km-scale backdrop

> **Status: IMPLEMENTED, AWAITING HUMAN REVIEW (2026-10-06).** Arm `converge4` (`WGE_PARITY_RENDER_POLICY=converge4`,
> kit `tools/kit/kit2.lock.json`, backdrop `tools/backdrop/backdrop1.lock.json`).

### Contract

- **Content, under `converge4` only:** a Gaea-built mountain field, 16 km
  square, that the world is seated in. The seat is a point of the field (a
  valley floor), chosen in the committed spec `tools/backdrop/backdrop1.json`;
  the field is generated without reference to the play space (GAEA_PROGRAMME.md
  §1). Around the seat the field is sunk under the near world and eases back to
  natural height; its outer margin eases to the seat floor so no cut edge
  stands against the sky. Placement puts the seat at the world centre, 2 m below
  the source terrain's mean height. The camera's far plane opens to 24 km.
- **Surface:** per-tile baked albedo from a material classification of the
  field (forest below the treeline on moderate slopes, meadow and scree above,
  rock on cliffs, snow above the snowline where the slope holds it). Lighting
  and haze come from the renderer and the N-1 atmosphere; nothing is baked.
- **Pipeline:** `tools/build_backdrop.py` (spec → Gaea build → GLB + lock),
  `backdrop.rs` (lock verification, conditioning, placement), the same seam as
  the kit.
- **Permitted files:** `backdrop.rs` (new), `lib.rs` (arm, inputs, lowering
  call), `main.rs` (`WGE_BACKDROP_SET`), `supervisor.rs` (inputs struct),
  `tools/build_backdrop.py`, `tools/backdrop/`, `tools/parity_ab_policy.sh`,
  `tools/verify_identity.py`, tests.
- **Tests** (`tests/converge4.rs`): converge4's policy is converge3's; the
  backdrop is refused outside converge4 and required by it; converge4 minus its
  backdrop instances, resources and far plane is converge3, byte for byte, in
  every view; the lock pins GLB size, digest and the Gaea height pixel digest,
  and each mismatch is refused with its reason.
- **Acceptance (measured):** N-1's ridge contrast ≤ 0.40 of the foreground's
  (0.65 today); world void fraction 0 in every view; GPU frame cost reported
  against converge3; packet ≤ 100 MB; converge3 and earlier arms byte-identical
  (131 frames).
- **Acceptance (human):** the wide and medium views read as a valley in a
  mountain range, at a believable distance, with sky above the peaks.

### Implementation notes (2026-10-05/06)

- **Field:** `Structure - Side Carved Mountains` rebuilt with `Terrain.Width`
  16 km / `Height` 4 km, so Gaea erodes at true scale (0-2885 m; slope p50 22°,
  p90 52°, the same as the 5 km original: slopes did not flatten). Builds at
  1024 only: 2048 exits 1 after ~8 min on this install (cause unverified). The
  seat (0.36, 0.88, yaw 75°) was chosen by a skyline search over the wide
  camera's 70° field: skyline median 7.4°, p90 10.1°, max 10.6°, defining
  ridges a median 10.7 km away.
- **First render (`ab-converge4-n3a`) found three defects, all fixed:**
  1. *Quality gate:* the terrain mask drops every pixel an authored mesh
     projects onto (captures carry no depth). The backdrop's tiles also
     project over the valley floor sunk under the world, so they erased the
     terrain: medium/wide coverage fell to 10-13%. Backdrop instances are now
     landform, not props (`visual_quality.rs`; test
     `backdrop_instances_are_landform_not_props`, mutation-checked).
  2. *The pool:* the mirrored pass clips terrain at the water plane but not
     meshes (CONVERGE-3 carried item). The sunk backdrop filled the reflection
     and the pool rendered as a dark blot. Backdrop instances are not drawn
     in the reflection pass (`LavaAdapter.jl`); the puddle's mirrored rays rise
     10-14°, which is sky.
  3. *Haze:* converge1's 45 bp / 100 m scale height was tuned on the 480 m
     world and put ~95% haze on a summit 10 km out. converge4 carries its own
     atmosphere, 6 bp / 250 m (`CONVERGE4_ATMOSPHERE`). This is a policy
     change, so converge4 is no longer content-only; the identity test asserts
     the atmosphere is the only policy difference.
- **N-1 is unmeasurable on converge3:** the scatter forest's projected
  bounding spheres mask 77% of the wide frame, leaving 0 edges in both
  classes. `tools/atmosphere_measure.py` now marches the backdrop as depth;
  the tree masking still needs a tighter mask before the gate can be read.
- **The backdrop prefix** (`backdrop-`) is shared by Rust, the Julia
  reflection pass and the Python metric; a test pins all three.

### Measured (`artifacts/parity/ab-converge4-n3c`)

| Gate | Result |
|---|---|
| Campaign 2 quality gate, 3 views | good (after the mask fix) |
| Identity, `tools/verify_identity.py` | **131/131 frames identical** (converge3 included) |
| Tests | Rust 154 passed / 0 failed (+6 converge4 incl. kit-dependent); Python 735 OK |
| Backdrop ridge contrast (Michelson, median) | wide 0.099, medium 0.094, close 0.101 |
| N-1 ratio | **not computable**: no foreground edge class survives in the converge3+ world (every near terrain edge backs onto forest or a mesh). Against converge1's foreground (0.18) it would be ~0.55, so **not met** on that reference |
| World void fraction | 0 (the converge0 ridge still closes every view) |
| Packet | **101.8 MB** compact (converge3 50.7 MB). Under the 128 MiB worker bound; over this contract's 100 MB. Backdrop meshes are ~48 MB of it, a third of that tangents the backdrop never uses |
| GPU frame cost | not measured: the deterministic receipt strips host timing |

Carried risks: packet headroom is 32 MB for the rest of CONVERGE-4
(remedies: tangent-free projection for maps-free assets, or a coarser
backdrop mesh); the N-1 metric needs a foreground class that works with
scatter before it can certify anything again.

### Non-goals

No backdrop shadows or far shadow cascade (L-1 decides). No normal map on the
backdrop (vertex normals at ~31 m cells first; add one only if the review asks
for surface detail). No full ring: the field is seated so the range lies in
the cameras' look direction; a 360° backdrop is a later item if a free camera
needs it.

---

## 2. L-1 — Cascaded shadows

> **Status: NOT STARTED.**

The single 512² view-fitted shadow map covers 60 m. N-7 review: "some of the
shadows from the trees seem kinda off". Contract to be written when N-3 is
accepted: 2-3 cascades, the audit's gate (≥ 20 texels/m within 30 m of the
camera), distant forest shadowed.

## 3. N-8 — Contact AO and depth capture

> **Status: NOT STARTED.**

Screen-space contact AO, and a depth capture so `grounding_contact` moves from
`indeterminate` to measured (audit table, N-8).

## 4. B-1 — Biome and ground cover

> **Status: NOT STARTED.**

CONVERGE-3 review: the trees and terrain read as arid; the fern is a jungle
plant and does not belong. Ground cover per biome (dry grasses, low shrubs
here), a second tree species through the leaf-card pipeline, species mix and
biome-driven density.

---

## Deferred to CONVERGE-5

| Item | Why not now |
|---|---|
| L-2 remainder (TAA, specular AA) with L-3 wind | Only visible in motion; captures are stills |
| Wear / wetness blends, several scans per family | Needs a material-layer system |
| Water beyond still pools (L-5) | Ripples, refraction, shoreline; the reflection pass clips terrain but not meshes below the plane |
| Instance compaction | Only if scatter grows |
| N-9 beauty shot | After N-3, L-1 and B-1 have set the frame's content |
| Bark depth review; kit `prepare` receipts; the two env-reading wrappers | Small; any session |
