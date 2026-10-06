# WGE Grindstone — Interactive Playtesting, Tuning, Diagnosis, and Refinement Subsystem

Status: **architecture / product specification, registered 2026-10-02**
Provenance: operator-authored product specification, registered verbatim-intent as the
post-TetCage-integration build target. Namespace `wge.grindstone`; operator surface
"Refinement Lab"; primary operator: human playtester/designer; primary agent: Cyan;
authority: WGE Rust kernel; execution: Julia / Lava / runtime workers; optional
analytics: Gesso; future representation experiments: TetCageRT.

This document is the registered condensation the build proceeds from. Where it and the
operator's full text differ, the operator's text wins; file corrections as evidence.

## 0. Executive thesis

Grindstone closes the gap between "the game works" and "the game feels right." The
traditional loop (play → stop → note → inspect → edit → rebuild → reproduce → retest)
collapses into: play → report (voice/text ticket) → automatic context capture → Cyan
diagnosis → bounded candidates → WGE validation → hot/warm/cold reload → human retests →
accept/reject → Rust promotes. The human remains the final judge of subjective
experience; Cyan translates "that grab is bullshit" into
"phase-2 grab volume active 117 ms beyond the visible contact envelope."

## 1. Binding doctrine (non-negotiable)

1. **Canonical game is never casually mutated.** canonical_generation → fork →
   candidate_generation → playtest → accept/reject → promote/discard. Cyan proposes;
   Julia/Lava execute; the human evaluates; Rust commits.
2. **Subjective reports are evidence, not instructions.** "The sword feels bad"
   initiates diagnosis (startup latency, recovery, animation curve, hitstop, stamina
   economics, input buffering, …), never a blind parameter bump.
3. **Experiential questions become experiments.** ExperienceProbe A/B/C variants with
   isolated dimensions; compositional selection ("B's commitment, C's impact") is
   recorded per-dimension and synthesized, never recorded as a single winner.
4. **Recognition over specification.** "That one" is a first-class answer.
5. **Isolate variables.** Composed simultaneous changes teach nothing.

## 2. Core objects

- `PlaytestSession`: session_id, canonical_generation, active_candidate,
  runtime_session, world/player identity, capability_manifest, telemetry_window,
  replay_buffer, ticket_stream, probe_history, candidate_history, checkpoint_chain,
  accepted/rejected decisions, promotion_receipts. Must answer: what was experienced,
  against what build/generation/candidate, was it reproducible, what was accepted,
  what became canonical.
- `GrindstoneTicket`: human_report (+voice_transcript), gameplay_context,
  replay/telemetry references, entity references, inferred_subsystems, diagnosis,
  proposed_candidates, disposition, receipts. Categories route but never constrain
  diagnosis (a BALANCE report may be caused by input latency).
- `ExperienceProbe`: question, invariant_state, dimension_under_test, variants
  (candidate_id, semantic_delta, implementation_delta, expected_effect), ordering,
  human observations, selected/rejected properties, canonical decision.
- `RefinementReceipt`: candidate_id, parent/resulting generation, ticket origin,
  semantic_delta_digest, evidence refs, validations, user acceptance, authority
  signature. Promotion requires one. Rejected candidates remain as preference evidence.

## 3. Automatic context capture (the defining advantage)

Rolling diagnostic buffers frozen into the ticket at report time: replay ~20 s, input
~30 s, combat events ~60 s, frame telemetry (10 s hi-res + summarized minutes), bounded
AI decision traces, relevant collision contacts, encounter snapshot. The human never
reproduces technical context manually. **The C3.2 input trace (digest-bound
`wge.input-trace/v1`) is the first concrete instance of the input buffer.**

## 4. Reload classes (mutation cost is formalized)

- **HOT** (immediate → seconds): weapon coefficients, stamina costs, movement curves,
  hitbox timing, cooldowns, AI utility weights, camera springs, hitstop, poise
  thresholds, difficulty values, material/fog/exposure values. Julia's policy-object /
  typed-configuration model is deliberate leverage here — but hot reload is not magical
  structural mutation; type-layout and resource changes are classified honestly.
- **WARM** (localized subsystem reconstruction; world stays continuous): animation
  graph changes, shader/kernel changes, AI policy implementation, resource
  reconstruction, ability-system topology. UX: "resetting this encounter."
- **COLD** (checkpoint → restart → restore): data-layout changes, scene topology,
  new native capability registration, renderer contract changes. UX: server-restart-like.

## 5. Refinement domains (each with the probe model)

Movement feel, camera, weapon timing/damage/stamina, hitbox timing, AI aggression and
13+ tuning dimensions (aggression, spacing, retreat threshold, punish confidence,
parry/dodge tendency, zoning, baiting, stamina discipline, prediction horizon, …;
difficulty from decision quality, not stat cheating), boss refinement (combo-function
analysis → candidate continuations → embodied selection), balance laboratory (human
playtest × synthetic matchup sweeps; role-preserving repairs over raw buffs), visual
probes (gloomy-but-readable and friends), performance tickets (auto-captured frame
telemetry, allocation spikes, compilation hitches; same candidate/promotion system),
hitbox/hurtbox transient diagnostic overlays ("show me why that hit").

## 6. Safety, roles, autonomy

- Failure-mode defenses: overfitting to one playtester (provenance, aggregation),
  too-many-variables (controlled probes), symptom tuning (causal diagnosis),
  candidate drift (ancestry, consolidation, semantic diff), hidden technical
  regressions (automated validation before promotion), hallucinated diagnosis
  (measured facts vs hypotheses, explicit UNKNOWN, targeted probes).
- Roles: playtester may play, ticket, annotate, rate, answer; may not promote or
  authorize structural change. Maintainer approves promotion.
- Autonomy levels 0–4: observe-only → candidate generation → bounded live tuning after
  confirmation → automated A/B/C probes → trusted low-risk repair. Promotion always
  follows project policy. Critical authority stays explicit.
- Friction telemetry (ticket→diagnosis, ticket→candidate, reload latency,
  HOT/WARM/COLD mix, reverted promotions) feeds `docs/platform/friction-ledger.md`.

## 7. MVP (frozen)

1. PlaytestSession; 2. canonical/candidate separation; 3. in-game Open Ticket;
4. voice/text report; 5. automatic replay + context capture; 6. simple telemetry
attachment; 7. Cyan ticket diagnosis; 8. HOT parameter candidate; 9. apply/revert;
10. user accept/reject; 11. Rust promotion receipt; 12. ticket history.
First target domains: movement feel, camera, weapon timing, damage/stamina, hitbox
timing, AI aggression, boss attack timing — all mapping onto the Demo A arena game,
whose hub (dummy, build station, configurable AI, queue) doubles as the Grindstone Lab
in developer mode.

## 8. Integration order (registered decision)

Grindstone enters **after** TetCageRT integration (Phase 3) and rides Demo A's systems:
the C3.2 input trace is already its input buffer; the presented session's telemetry is
already its frame window; TetCage A/B (baseline vs RT candidates, measured frame/VRAM/
memory/visual-error/density) is literally Grindstone §30's representation-policy
laboratory. Demo A therefore ships as both public demo and continuous refinement
benchmark. "WGE builds the blade. Grindstone sharpens it."
