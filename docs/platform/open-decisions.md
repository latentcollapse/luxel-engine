# Open decisions — calls made without review

Written 2026-08-02. Ten systems were built in one session past the last point
Matt confirmed anything. Everything in it is tested and committed in small
pieces, but a decision nobody has looked at is not the same as a decision that
has been made, and the further the work goes the more expensive each of these is
to change.

**This document exists to be argued with.** Each entry is a call I made alone,
with the value, why, where it lives, and what it costs to reverse today. None of
them is load-bearing on the others unless the note says so.

Ordered by what they cost if left wrong.

---

## Review status — 2026-08-03

Matt reviewed the whole document. Every item is now resolved. Each entry below
carries a **Resolved** line; the argument above it is left intact, because the
reasoning is why the decision is what it is.

Agreed work order, in dependency order rather than value order:

1. **Item 5** — bog depth. Cheap, and it must land *before* item 6: surfacing a
   misclassified wetland renders the lie at higher fidelity.
2. **Item 6** — the fifth splat channel. Chosen over deferring it behind item 7.
   Contract change, so it runs while nothing else is in flight.
3. **Items 7 + 8** — scatter crossing, then move the obstructing placements.
4. **Item 2** — the D25 gate fix.
5. **Item 4** — the second, gentler world, which also validates 2, 5 and 6.

**D26** (item 1's survivor) is agreed but unscheduled: it is terrain shape and is
orthogonal to all five above, so it can land whenever it is convenient rather
than blocking anything. It should precede item 4, since the second world would
otherwise be judged against the same flat-topped profile.

**Item 9 is a programme, not a queue entry.** Driving Gaea from WGE is agreed in
principle and scoped nowhere; it does not displace the order above.

### Progress — 2026-08-04

**The work order is complete except for item 8, which is blocked on D29.**
Items 5, 6 and 7 landed 2026-08-03; D26, item 2 and item 4 landed 2026-08-04.
Read [docs/archive/2026-08_terrain-sessions/2026-08-04_session-handoff.md](../archive/2026-08_terrain-sessions/2026-08-04_session-handoff.md) rather than
relying on this summary — in particular for two claims made about item 7 on
2026-08-03 that turned out to be wrong.

- **D26 done**, and its recorded cause was wrong. The transition was not
  saturating early; it was barely completing at all, on 56.1% of the perimeter.
  Delivered relief went 39% → 63% of authored, tallest point 58.2 m → 94.9 m.
- **Item 2 done.** The gate now fails on crushed pixels rather than dark ones,
  against a threshold derived from 8-bit quantisation rather than tuned.
- **Item 4 done** — `glenmara_highland_vale_v1`, the arena's gameplay graph on
  highland terrain at 78 m relief. It found three defects on its first build
  (D32, D33, and its own authoring error), which is a better argument for a
  second world than the one that scheduled it. Its layout is deliberately
  identical to the arena's: the question D28 asks is about terrain, so changing
  anything else would have made the comparison answer nothing.
- **Three new defects logged: D31, D32, D33**, plus D34 for a viewer hang.
  D31 in particular is larger than D26 was: the containment floor had been
  replacing 87.7% of the massif.

**D28 is now one decision away, and it is Matt's.** The two worlds come out
85.9% and 92.0% heath scrub — the *gentler* one more so, with a treeline nearly
three times higher and less canopy. **The terrain is ruled out**: heath dominance
belongs to the ecology, not to the arena's cliffs. What remains is whether ~90%
heath is the intended world. If it is, the foliage gate is the defect; if it is
not, the ecology is. That is taste, and the measurement should not pretend to
answer it.

---

## 1. Vertical budget: 62 m of border relief on a 256 m map

**Where:** `zone_compiler.DEFAULT_BORDER_POLICY["height_m"]`
**Reverse:** one number.

A ratio of roughly 1:4. Lauterbrunnen — the stated reference — is nearer 1:1;
the walls are as tall as the valley is wide. Genuinely Alpine proportions here
would mean 150–250 m walls.

I stopped at 62 m because playable area had already fallen from 68% to 50% of
the world, and past this the border stops framing the map and starts eating it.
That is a design trade, not a technical limit, and it is Matt's to make.

**Load-bearing on:** everything visual. Raising it changes shadow, camera
framing, how much sky a third-person camera sees, and the snowline (which is a
fraction of relief). Worth settling before more is built on top.

**Resolved 2026-08-03 — superseded, no decision taken.** The massif inversion
landed the same night and put `massif_relief_m: 150.0` *above* this number, so
`height_m: 62.0` is no longer the mountains — it is the inner rampart standing
inside a 150 m range. The 1:4 ratio this entry argues against is now nearer the
1:1 Lauterbrunnen proportion it says would be required. The playable-area cost
it worried about did materialise (protected relief 12% → 50.19%) and was paid
for separately by moving the accessibility gate to a world-relative denominator.

What survives from this entry is **not** the vertical budget but the *shape* of
the transition, now tracked as [D26](debt-ledger.md): `wall_width_m` is a single
global scalar, so the world reads as a range from the east wall and as a
flat-topped mesa at player height. Raising `massif_relief_m` makes the plateau
taller, not more mountainous — the obvious knob is the wrong one.

**D26 resolved 2026-08-03 — fix it; make the transition a field.** Wall width
varies per direction so `into` stops saturating wherever the envelope has room.
Holding it on the grounds that item 9 might demote this code was offered and
declined, correctly: the driven-Gaea path is a larger programme than this fix,
and leaving the reference terrain visibly wrong in the meantime would mean every
judgement made against it in the interim is made against a known-bad surface.
Same defect class as the two other scalars-standing-in-for-fields removed on
2026-08-03.

---

## 2. Refusing the black-pixel gate rather than flattening the mountains

**Where:** `bevy_visual_acceptance`, threshold 0.08; current value 0.094.
**Reverse:** either re-baseline the threshold or lower `height_m`.

The gate fails. I did not fix it, because the cheapest way to satisfy it is to
build flatter mountains, which is the same standing-incentive defect as
[D16](debt-ledger.md) — a metric arguing against the art direction it is meant
to serve. Logged as [D25](debt-ledger.md) with a fix direction (separate
*unreadable* from *shadowed* by measuring contrast inside the dark region).

**The risk of my choice:** a red gate that everyone learns to ignore is worse
than no gate. If this is not re-baselined reasonably soon it becomes noise.

**Resolved 2026-08-03 — implement the D25 fix; do not re-baseline.** Change what
the gate measures rather than what it permits: separate *unreadable* from merely
*shadowed* by measuring contrast inside the dark region. A dark but legible cliff
face passes; a black void fails. Re-baselining was explicitly rejected because a
threshold fitted to the current world encodes that world rather than a standard —
the same defect already logged as [D27](debt-ledger.md), and not one worth
committing twice in two days.

---

## 3. Rivers capped at 1.15 m deep

**Where:** `hydrology.channel_depth_field(maximum_depth_m=1.15)`
**Reverse:** one number.

Taken directly from the stated direction — rivers shallow enough to walk,
bridges as an aesthetic choice. The consequence I did not anticipate: at that
depth on a 256 m map they are **invisible from the overview camera**. They will
read at player height and they matter to navigation, but the map does not look
like it has rivers.

The fix is probably not deeper water. It is surfacing that follows the channels
— wet stone, gravel banks, riparian green — which is blocked on item 6.

---

## 4. Heath scrub takes 69.5% of the map

**Where:** `forestry.HIGHLAND_NICHES`, the slope and wetness bands.
**Reverse:** band values, but see below.

Canopy covers 7%. The conifer's 34° slope limit excludes most of a world that is
now mostly cliff. **I could not tell from one map whether this is honest or
miscalibrated**, and I would rather say so than tune numbers until the picture
looks nice.

Disambiguating it wants a second, gentler world — which is exactly the Phase 3
experiment. Tuning it against this map alone risks fitting the niches to one
piece of terrain.

**Resolved 2026-08-03 — build the second world first, do not tune the bands.**
Tuning against one cliff-heavy map was explicitly rejected as fitting the ecology
to a single piece of terrain. The gentler world settles it in one build, and
doubles as the validation surface for items 2, 5 and 6 — every one of which is
currently judged against the same single map. Scheduled last in the work order
because it is the largest piece and benefits from the other three landing first.

---

## 5. Bogs classified on catchment alone

**Where:** `hydrology.BOG_CATCHMENT`, `hydrology.classify`.
**Reverse:** add a depth term; small and self-contained.

A closed sink with low inflow is called a bog regardless of depth, so the alpine
arena currently has a **15.19 m deep "bog"**. That is a tarn. The concept is
right and the implementation is cruder than the concept — depth needs to enter
the classification.

This one I think is simply wrong and would fix without asking, given the chance.

**Resolved 2026-08-03 — agreed, it is a defect rather than a decision. Fix it.**
Depth enters the classification; a deep closed sink is a tarn, not a bog. Ordered
*before* item 6 deliberately: the fifth splat channel renders wetland, so
surfacing a misclassified body would render the lie at higher fidelity instead of
exposing it.

---

## 6. Peat and alluvium deferred rather than added

**Where:** the splatmap's fixed RGBA of grass/road/rock/snow.
**Reverse:** not cheap — a contract change touching material contracts and every
backend shader.

S2 now identifies twenty stagnant water bodies and S1 knows where the ground is
wet, and none of it reaches the surface because there is no channel to put it
in. A bog that renders as grass is the same class of lie as a keep that reads as
enterable.

I deferred it rather than half-doing it. But it is the blocker on items 3 and 4
both looking right, so it may deserve to jump the queue.

**Resolved 2026-08-03 — do it now. It jumps the queue.** Deferring it behind item
7 was offered and declined. The reasoning that carried: it is the only item in
this document that *unblocks* two others rather than sitting beside them — item 3's
invisible rivers get gravel banks and riparian green, and item 4's ecology stops
being judged on a surface that cannot express wet ground. Being a contract change
touching material contracts and every backend shader, it runs while nothing else
is in flight, which is why item 7 follows it rather than interleaving.

Item 3 (rivers capped at 1.15 m) needs no separate decision: the depth stays, and
this is the fix for it. The rivers were never meant to be deeper — they were meant
to be *visible*, and surfacing is what makes them so.

---

## 7. The Rust scatter crossing, specified but not made

**Where:** `render_plan.rs::compile_foliage`, and its validator.
**Reverse:** n/a — not yet done.

Four solvers are correct and unobeyed: forestry does not place trees, siting
does not move villages, S14 split the guardian out and nothing places one. The
field is emitted and the change is specified in
[docs/world/systems-roadmap.md](../world/systems-roadmap.md#s6-forestry-and-vegetation-ecology--v1-done-2026-08-02).

I stopped short deliberately: compile *and* validate must apply an identical
rule or the validator rejects every plan the compiler just built, and starting
that at the end of a long unreviewed session is how you get a broken render plan
and no idea which of eleven changes caused it.

**Resolved 2026-08-03 — go, and close item 8 with it in the same pass.** Holding
until the DSL settles was offered and declined; the placement *vocabulary* may
move, but the compiler/validator crossing is beneath the DSL and would be built
the same way regardless. The identical-rule hazard stands and is handled by
landing the shared rule first and the two call sites second, not by splitting the
work across sessions.

**Built 2026-08-03.** The scatter reads `canopy_suitability_u8.bin`. Notes worth
keeping, because two of them changed the design:

- **The layer declares which field governs it** (`asset_plan.ECOLOGY_FIELDS`),
  rather than Rust sniffing `role` for "canopy". Canopy suitability governs
  canopy; gating groundcover or forest-floor rock on it would strip scree out of
  the open ground it belongs on.
- **The scatter samples the field rather than rejecting against it.** Uniform
  darts at the bounding box would have hit suitable ground ~2% of the time
  against a 50-darts-per-instance budget, so builds would have failed the
  `placed N of target` contract intermittently, on terrain rather than on code.
  Drawing in proportion to suitability also thins the treeline instead of
  stamping an edge, which is what point 4 above asked for.
- **The validator checks a floor, not a re-derivation.** Under proportional
  sampling there is no single value to demand without reimplementing the RNG and
  guaranteeing drift. Nothing may stand where its field is zero; compiler and
  validator agree exactly at that boundary, which is where agreement is
  load-bearing. The feared "validator rejects every plan the compiler built"
  never had to arise.
- **The authored instance count became a ceiling** for ecology-governed layers.
  `central_forest` asks 28 conifers and its ecology holds nine. Shortfalls are
  recorded in `render_plan.ecology_shortfalls` and printed by the build, never
  swallowed. Layers with no declared field keep the strict quota.

Result: 50 conifers where 112 stood, every one on suitability ≥ 151/255, none on
zero. 275 render-plan instances, down from 337. Rebuilds are byte-identical.

It also tripped a visual gate — see [D28](debt-ledger.md), which is not a
regression but the ecology and the gate disagreeing for the first time.

---

## 8. D13 is named, not closed

**Where:** `siting_plan.json`, `obstructing_placements`.

Twelve placements obstruct lanes, worst `southwest_hamlet over central_lane by
19.5 m`. The audit reports structure, lane, metres and repair — a better place
than "no lane runs end to end", but the villages have not moved and the three
lanes still fail.

Moving them means adopting the proposals into `annotations.json` or teaching the
Rust anchor compiler to read the plan. Both are item 7's neighbourhood.

**Resolved 2026-08-03 — closed as part of item 7, via the Rust anchor compiler.**
Teaching the compiler to read the siting plan is preferred over adopting the
proposals into `annotations.json`, because the latter bakes one solver run into
authored content and the placements stop tracking the solver that produced them.
This is what clears the navmesh acceptance gate.

**Not built, and the last sentence above is wrong — see [D29](debt-ledger.md).**

Breaking the obstructions down by owner, which nobody had done, moving every
hamlet clears `central_lane` and leaves the other two lanes failing:
`north_lane` is 12/14 blocked by `hibernia_keep` and `south_lane` 14/17 by
`albion_keep` — gate piers, flank roofs and flank towers standing in the lane
the gate exists to admit. Those are not in the siting plan and correctly so: a
keep is exempt from lane keep-out because the lane is *meant* to pass through
its gate.

So D13 is still worth closing and is no longer sufficient. The dominant defect
is whether a gate is a doorway or a solid object shaped like one, which is a new
decision and is recorded as D29. Work stopped here rather than building a fix
whose stated purpose it could not achieve.

---

## 9. Provenance grade for imported heightfields

**Where:** not yet built. Raised 2026-08-03, when Gaea came online.
**Reverse:** n/a — undecided.

An externally authored heightfield is not reproducible from the intent file,
which is the assumption the whole determinism story rests on. So either imports
are capped below Grade A, or "deterministic" is redefined as *reproducible from
spec plus pinned input bytes* — which is the same shape as the existing
provenance binding, since that already ties analysis to exact raster bytes.

**Correction, same day:** the redefinition is unnecessary. §17 of the language
spec already grades determinism over *"equal declared inputs"*, not over the
intent file alone. A digest-pinned heightfield is a declared input, so a pinned
import is Grade A under the existing text. The framing above overstated the
determinism claim.

The real trade is **reproducibility versus regenerability**. A pinned file
reproduces exactly and forever; it cannot be *regenerated* if lost, and cannot be
varied by turning a parameter. That second loss is the expensive one — a model
cannot turn a dial on a mountain it did not generate, which is precisely the
Aisling thesis.

**Resolved 2026-08-03 — WGE drives Gaea. Imports stay Grade A.**

WGE emits the `.terrain` graph and builds it headlessly; the built heightfield is
digest-pinned as a declared input. The world stays regenerable *and* byte-exact,
and the skill floor survives, because the authored surface is still DSL rather
than a GUI. Accepting hand-authored imports was declined: it was available almost
immediately and would have made each such world a fixed asset rather than a
compiled one.

What makes this viable rather than aspirational, from inspecting the 2.3.0.1
install:

- `.terrain` is plain JSON — a Newtonsoft-serialized object graph with `$id` /
  `$values` reference tracking. WGE can *write* it, not merely read it.
- `Gaea.Swarm.exe`, `Gaea.Server.exe` and `Gaea.BuildManager.exe` ship in the
  box, alongside `Grpc.Core.Api.dll`. There is a headless build path and a gRPC
  surface without writing a single line against Gaea's UI.

**No Gaea plugin.** A plugin would put WGE logic inside a proprietary Windows
application running under Proton, which is the same error the README forbids for
backends: a change expressible in only one of them is in the wrong layer.
Emitting `.terrain` makes Gaea an adapter target, droppable exactly like the
Unity and Unreal adapters.

### Swarm's CLI surface — verified 2026-08-04, and it is better than assumed

Run under Proton against the 2.3.0.1 install. `Gaea.Swarm --help` returns:

```
Gaea.Swarm [[--Filename] <String>] [--buildpath <String>] [--ignorecache]
  [--interactive] [--node <Int32>] [-p <String>] [-r <String>]
  [--resolution <String>] [--safemode] [--seed <Int32>] [--silent]
  [-v <String=String>...] [--va <String>...] [--vars <String>] [--verbose]
```

The four that matter, and they change the design:

- **`--vars <path>`** — a `.json` or `.txt` of variable name/value pairs, and
  **`-v name=value`** repeated. Gaea graphs can expose variables, so WGE does not
  have to synthesise a `.terrain` graph to parameterise a world. It can drive a
  *hand-authored* graph.
- **`--seed <int32>`** — the mutation seed, so stochastic nodes are pinned.
- **`--resolution`** and **`--buildpath`** — both overridable per invocation, so
  build size and destination are the driver's business rather than the file's.
- **`--silent`** — "disables all interactivity, useful for automation", plus
  `--ignorecache` for a guaranteed cold build.

**This is a better contract than emitting `.terrain` JSON.** The graph — which is
where the *taste* lives, and which Matt can shape in the GUI — stays authored in
Gaea. WGE supplies intent as variables and pins seed, resolution and output. It
keeps the "no Gaea plugin" rule intact: Gaea stays an adapter target, driven from
outside, and the authored surface is still a WGE artifact.

Emitting `.terrain` directly remains possible (it is plain JSON) and is the right
escalation if variables prove too narrow. It is no longer the *starting* point.

### Blocked: every headless entry point needs WMI, and Wine has no shim

`Gaea.Swarm.exe`, `Gaea.SwarmHost.exe` and `Gaea.Server.exe` all fail during
Gaea's shared "Initializing environment" startup:

```
System.TypeInitializationException: 'System.Management.WmiNetUtilsHelper'
  Win32Exception (126): Failed to load required native library
  'C:\windows\Microsoft.NET\Framework64\v4.0.30319\wminet_utils.dll'
```

Argument parsing succeeds first, so the CLI surface above is confirmed against
the real binary — the failure is strictly at hardware/node enumeration. Wine's
built-in .NET Framework directories exist in the prefix but contain no WMI
interop DLL, and `Gaea.BuildManager.exe` is a dead end (its `.dll` is absent from
this install).

### SOLVED 2026-08-04 — headless Gaea builds work, end to end

A real build ran in **29 s** and produced `Shade_Out.png` and
`Cartography_Out.png` at 512², with dendritic drainage networks, a glacier
terminus and connected ridgelines. **No licence prompt at any point.**

Four blockers, each one layer deeper into the app, and the order matters:

| symptom | cause | fix |
|---|---|---|
| `WMINet_Utils.dll` not found | Wine's stub .NET has no WMI interop | `winetricks dotnet48` |
| `hostfxr.dll` not found | Gaea is a .NET 8 app | `winetricks dotnetdesktop8` |
| `u_charsToUChars from libicuuc` | ICU missing under Proton's *bare* wine | use `proton run`, not the wine binary |
| `SetConsoleOutputEncoding: Invalid access`, then `IOException: Invalid handle` at build time | Gaea renders progress with Spectre.Console and needs a real console | **run under a pty (`script -qec`)** |

**Two traps worth naming.** `DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1` looks like
the ICU fix and is a dead end — Swarm explicitly requests `en-US` and dies with
`CultureNotFoundException`. And **Proton's `proton run` wrapper is not
interchangeable with `Proton - Experimental/files/bin/wine`**; only the wrapper
sets up a console Swarm accepts.

**The working invocation:**

```bash
export STEAM_ROOT="$HOME/.local/share/Steam"
export STEAM_COMPAT_CLIENT_INSTALL_PATH="$STEAM_ROOT"
export STEAM_COMPAT_DATA_PATH="$STEAM_ROOT/steamapps/compatdata/gaea-swarm"
PROTON="$STEAM_ROOT/steamapps/common/Proton - Experimental/proton"
cd ~/.local/share/gaea2                       # NVMe copy; see below
script -qec "'$PROTON' run cmd.exe /c \"Gaea.Swarm.exe <file.terrain> \
  --buildpath <outdir> --resolution 512 --silent\"" build.log
```

Paths crossing into Windows use `Z:\...`. The prefix at
`compatdata/gaea-swarm` was provisioned with winetricks against a plain
`WINEPREFIX`, then staged into the compatdata layout (`pfx/` plus `version`,
`config_info`, `tracked_files` copied from the working `gaea` prefix) so
`proton run` accepts it. **The interactive `gaea` prefix was never modified.**

**Run Gaea from NVMe.** `/mnt/d` is a mechanical disk behind FUSE: `Swarm --help`
takes **33 s** from there with a warm cache against **1 s** from
`~/.local/share/gaea2`. At 33 s of startup per invocation the driver would be
unusable. See [D30](debt-ledger.md).

**A graph only builds headlessly if it defines a `SaveDefinition`.** Only 3 of
the 59 bundled examples do; the rest are graph demos with no output node, and
they exit 0 having written nothing — which looks like a failure and is not one.
The WGE graph must mark its export nodes.

**Scheduling:** this is a programme, not a task, and it does not enter the work
order above. It also does not moot D26 — see item 1.

---

## What I would do next, if asked

Superseded by the reviewed work order at the top of this document. Kept for the
record: the original recommendation was 1 → 5 → 6 → 7, and the review changed it
only by dropping item 1 (superseded by the massif inversion) and appending items
2 and 4 rather than leaving them unscheduled.
