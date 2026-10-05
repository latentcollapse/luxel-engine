# WGE Pre-Demo Audit

Status: **audit complete; blockers identified; fix order frozen**
Date: 2026-10-02
Method: /gauntlet whole-system recon — campaign map review, live verification gates, worker/contract surface inspection, toolchain checks.

This audit answers one question: **can the first playable demo be built on the current engine spine, and what must be fixed first?**

## 1. Verification runs executed during this audit

| Gate | Result | Evidence |
|---|---|---|
| `cargo test -p wge-native-graphics-contract --test session -- --test-threads=1` | PASS 5/5 (4.90s) | deterministic camera walk, world divergence rejection, camera-extent/degenerate-state/request rejection |
| Repo state | clean | HEAD `0f435f0`; only untracked path is quarantined `graphics_lab/tet-lab/` (by design) |
| Julia `Pkg.test(test_args=["lava_adapter"])` | **FAIL → FIXED → PASS 26/26, exit 0** | FR-0014 below; test target was missing from `graphics_lab/Project.toml` on Julia 1.12.6 |
| Blender availability | `/usr/bin/blender` 5.2.2 LTS | provider jobs feasible without any install |
| GLFW.jl gamepad surface | joystick API present (GLFW `wA4ue`) | DS3-style Xbox layout implementable through existing dependency |

The Julia gate failure is the audit's first catch: a gate recorded green at C3 closure was not reproducible on the current toolchain. Fixed in this audit (environment declaration only; no source/schema change) and logged as FR-0014.

## 2. Findings

Severity rule: BLOCKER = the demo cannot ship without it; HIGH = the demo is not credible or not fun without it; MEDIUM = quality/robustness debt that can trail the v1 wedge; LOW = recorded, no action; DEFERRED = explicitly outside the demo critical path.

### BLOCKER

**B-1 — Input → simulation loop does not exist (C3.2).**
The presented window is live (C3, commit `3933c24`), but the worker exposes only `open_window` / `render_window` / `close_window` with a Rust-owned deterministic camera walk. Nothing samples a human. The physics seam is ready and typed (`KinematicInput` → `PhysicsWorld::step()` → `KinematicStepReceipt` with receipt validation in `reference_runtime/src/physics.rs`); it is simply not wired to any input source or fixed tick.
Fix: C3.2 — input sampling (keyboard/mouse first, gamepad mapping layer immediately behind), fixed-step tick, camera intent, deterministic input-trace capture. This is the single highest-leverage task in the repository.

**B-2 — Frame pacing and frame budget are unattributed (quality-gap item 8).**
Cold close sample: 31.131 s wall to first promoted frame. Warm 30-frame sample: 1,144.686 ms wall p95 vs 69.660 ms adapter-reported interval p95 (GPU 485 µs p95). The wall-minus-renderer gap is not attributed to Rust transport, Julia scheduling, readback/upload, or present. A demo that stutters or eats a core is not shippable, and the user requirement is explicitly "cool without turning the GPU into a grenade."
Fix: attribute the gap with instrumented vs uninstrumented runs, then set a pacing policy (fixed sim tick + present cadence + vsync decision) in C3.3's remaining scope. A 16.6 ms budget with honest telemetry is the target for the arena slice.

**B-3 — No characters, no skeletal animation, no skinning (C5 fully open).**
Explicitly recorded as a deferred/negative control: "character rigging, skinning, and retargeting" — nothing renders as an animated humanoid. A third-person melee demo without animated fighters is not the demo.
Fix: C5.1–C5.5 — `humanoid_v1` skeleton profile, Blender build-time provider jobs (Blender 5.2.2 LTS confirmed installed), GPU skinning packet through Lava, minimal animation graph (idle/locomotion/attack/hit/death/dodge), gameplay binding. Blender stays absent from runtime per doctrine.

### HIGH

**H-1 — Combat kit not composed.** `gameplay_contract` has GCS/runtime/live_runtime machinery, but no stamina melee loop (dodge/guard/parry/poise/stagger), no weapon-class movesets, no powerstancing rule. Demo A v1 needs a deliberately small composed loop; the mega-sprint's "small combat loop" foundation is green, the composition is not.

**H-2 — AI opponents do not exist.** v1 needs 3+ deterministic personalities (aggression/spacing/punish axes) with decision quality as the difficulty axis; Phase 2 needs build-aware adaptation and the mass-matchup dominance scan. Nothing exists yet.

**H-3 — Visual baseline is below the demo bar.** Single directional shadow + PCF, no IBL, no alpha-tested foliage, spatial 2× resolve, no tone-mapping grade (quality gaps 3–5). The C4 exit gate ("commercially plausible game screenshot" by independent human review) is the bar for the arena's first public screenshot; C4.1 (materials) and C4.5 (atmosphere/tone map) are the minimum slices.

**H-4 — Imported-object collision participation and alpha/LOD policy open (C2 remainder).** Arena props, pillars, and verticality need imported meshes to participate in collision; foliage needs an alpha policy. Recorded in the gap register; must close for the arena build.

**H-5 — No UI path.** HP/stamina bars, hub menu, loadout, duel queue prompt. The composite already has a shared overlay pass (`_composite_capture!`), so the honest path is a typed Rust-owned overlay/HUD packet, not a second renderer.

### MEDIUM

**M-1 — Cold start 31 s.** Tolerable during construction, unacceptable for the packaged demo (C8.3/C8.4 own the sysimage/pipeline-cache program). Tracked, not blocking v1 development.

**M-2 — Julia gate reproducibility.** FR-0014, **fixed in this audit** (test target declared; 26/26 green). Keep `Pkg.test()` in every Julia campaign exit gate.

**M-3 — Gamepad path unproven on hardware.** GLFW.jl exposes the joystick/gamepad API and libglfw supports mappings, but no device has been exercised through the worker. DS3-style Xbox layout (user requirement) lands as a mapping table over the C3.2 input abstraction; risk is low but nonzero until a controller is polled successfully.

**M-4 — Checkpoint/restore and long-session policy open (C3.1 remainder).** Worker `render_window` caps at 1024 frames/batch; a persistent game session needs a chunking/checkpoint policy. No v1 blocker (sessions will be short), but ranked/BR modes must not build on a session contract that cannot checkpoint.

**M-5 — Elden Ring automatic powerstancing is a gameplay-contract decision, not a renderer one.** Same-class dual wield auto-pairs into a shared moveset with altered frames/damage. Must be represented in the typed gameplay contract (weapon pairing rule + moveset resolution), never hardcoded in presentation. Cheap, but it shapes the combat kit's data model — decide it before composing weapons, not after.

### LOW

- **L-1** — Riverwatch canonical overview remains diagnostic-grade (gap 1); superseded for the demo by the authored arena scene.
- **L-2** — Bevy instrument retained as inspection oracle only (gap 10); no action.
- **L-3** — Visual-quality gate is a technical floor, not an aesthetic judgment (gap 11); schedule human-review checkpoints at each demo milestone.

### DEFERRED (explicit, not silently passed)

- **TetCageRT integration** — the mega-sprint lists TetCageRT promotion outside the demo critical path ("research until the conventional path exposes a measured problem it solves"). The user has now explicitly authorized integration as a measured A/B (ARENA BASELINE vs ARENA+TetCageRT, publish metrics, spend the savings on density). Decision: integration happens **after Demo A v1 is playable**, as Phase 3 — this preserves the sprint doctrine (baseline first, measured problem stated) while honoring the user authorization. TetLab remains quarantined until the post-reboot hookup; its Spiral H evidence (graph-fused skin ≈ 9–11 µs, ~0.06% of a 16.6 ms frame, ~2.9× instance capacity) is the pinned economic argument.
- **Netcode/multiplayer** — the 100-AI battle royale is scoped as a **local-simulation AI showcase** (Phase 4), not a networking project. Shrinking-mist zone logic is deterministic gameplay simulation.
- **Ragdolls/destruction, streaming world, meshlets/virtual geometry, ray tracing** — unchanged, outside demo critical path.

### Toolchain answers recorded during audit

- **Blender**: installed (`/usr/bin/blender`, 5.2.2 LTS). No install needed.
- **Blender MCP**: **not required for the demo.** The C2.3 provider-job boundary is the authority path: bounded, digest-bound provider jobs whose output re-enters through typed Rust validation. Blender-MCP would be an interactive convenience for a human artist, not an autonomous-construction requirement — the model drives provider jobs through the pipeline, not through an MCP session. Decision: skip for Demo A; revisit only if interactive human-in-the-loop art iteration becomes a real workflow.
- **Controller support**: implementable through the already-pinned GLFW dependency; DS3-style Xbox layout is a mapping table + gamepad state op in the C3.2 input contract.

## 3. Demo A — frozen scope (dark-fantasy PvP arena training game)

Per the approved strategy brief, one absurdly polished systems-dense game; not "any game," one game. Phases:

```text
Phase 0  (now)        Fix B-1/B-2/B-3 in order; minimal C4.1/C4.5; combat kit
Phase 1  Demo A v1    Roundtable-style hub + training dummy + duel queue +
                      1 arena + 3 weapon families + DS3-style Xbox layout +
                      automatic powerstancing + stamina melee core +
                      3 deterministic AI personalities. Playable end to end.
Phase 2  Depth        Buildcraft (stats/armor/resistances/status/scaling),
                      20+ weapons / 15+ spells trajectory, ranked ladder,
                      3-build dominance proof via hundreds–thousands of
                      simulated AI matches with a dominance scan.
Phase 3  TetCage      (post-reboot) ARENA BASELINE vs ARENA+TetCageRT on
                      identical content; publish package/runtime memory, GPU
                      memory, animated-geometry cost, frame time, density,
                      entity counts, visual error; then SPEND the savings
                      (300 → 1,500 animated background entities, banners,
                      crowds, living forest).
Phase 4  Scale        100-AI battle royale (local sim, shrinking mist) +
                      Ambient Arena / Living Wallpaper low-power mode.
Demo B               Alpine Citadel (world/scale axis; already provisionally
                      scoped in the mega-sprint §2) — separate demo, do not
                      collapse into Demo A.
```

Hub doctrine: the hub is social space — no combat code — and doubles as tutorial (training dummy = first input/animation/feedback test), loadout UI host, matchmaker lobby, and the TetCage living-density showcase stage.

## 4. Fix order (the demo wedge plan)

1. **C3.2 input loop** (B-1): input sampling → fixed tick → `PhysicsWorld::step` → camera intent; gamepad mapping behind it (M-3).
2. **Frame budget attribution + pacing policy** (B-2): instrument, attribute, set the 16.6 ms budget with honest telemetry.
3. **C5 character path** (B-3): skeleton profile → Blender provider job → GPU skinning → minimal animation graph. Longest pole; starts immediately after C3.2 lands its first playable movement, in parallel with 4 if running workers in parallel.
4. **Combat kit** (H-1 + M-5): stamina melee core, weapon families as data, powerstancing pairing rule in the contract.
5. **C4.1 + C4.5 minimal** (H-3): material calibration and tone map/exposure for the arena look.
6. **C2 remainder for the arena** (H-4): imported-prop collision participation, alpha/LOD policy.
7. **HUD/UI overlay packet** (H-5): typed Rust-owned overlay through the existing composite pass.
8. **AI personalities v1** (H-2): deterministic decision-quality opponents.
9. **Demo construction gauntlet** → playable v1 in `Playable Demos/`.
10. Phase 2 → Phase 3 (TetCage A/B, post-reboot) → Phase 4.

## 5. Gate

This audit is satisfied when: B-1/B-2/B-3 are closed with their campaign exit gates, the Julia gate remains green (FR-0014 regression watched), and Demo A v1 runs end-to-end in a native window at the pacing target. The demo construction gauntlet then runs autonomous-until-done per the user mandate.
