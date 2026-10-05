# RPD SOP

    Research / Plan / Develop
    Audience:     team leads (Grok, Codex, Claude) when Matt asks for
                  R&D on a problem
    Copies:       identical files at Gesso/, Palette/, and
                  Project Cyan/ (Cyan's folder on disk).
                  Edit all three or they will drift.
    This is not:  a Buffy /goal, a work item, or permission to implement.

RPD is a spiral with an exit ramp, not a waterfall and not an infinite
chat. Buffy-class implementation starts only from a closed contract.

---

## Trigger

This SOP applies when Matt asks for R&D, research, a spiral, "what
should this be," or "figure out this problem."

It does **not** apply when he hands you a closed recipe, a bug with a
known repair, or "just implement it." Those go straight to Develop
(micro) or to the implementer.

One lead owns a spiral. Name the owner, the repo, and the layer
(Gesso / Palette / Cyan / Lab / WGE) in the first reply.

---

## The spiral

```
    RESEARCH  →  PLAN  →  DEVELOP  →  RESEARCH
         ↑                               │
         └──────── leftover question ────┘
```

Exit this spiral when every **load-bearing** question for **this
problem** has a disposition. The leftover question starts the *next*
spiral. It is not a reason to keep talking.

---

## Eleven laws

1. **Research before architecture.**
   Do not design around assumptions that can cheaply be investigated.
   If the experiment is expensive: name the assumption, proceed, tag
   it for the next spiral.

2. **Research produces evidence, not requirements.**
   A complaint such as "I need a better revival report" is an
   observation. The requirement might actually be "the system needs
   explicit state identity and survivorship semantics."

3. **Planning is compression.**
   Explain many observations with fewer primitives. Seventeen UX
   problems becoming four architectural concepts is progress.

4. **Planning must be adversarial.**
   The same lead red-teams their own abstraction before the roadmap.
   Kill for: seams, measurement, non-composability, scaling,
   hardware/runtime, correctness, decisive cost. Semantic nitpicks
   are not kills. Hold existing canon until evidence forces a change.

5. **Prototype to answer questions, not merely to make progress.**
   A throwaway 100-line experiment that kills a bad abstraction is
   enormously productive. Prototypes stay throwaway. They do not land
   in product `src/` of a layer that does not own the question.

6. **Development generates research data.**
   Bugs, operator friction, strange usage patterns, performance
   results, and unexpected inventions all flow back into Research.

7. **Real use outranks synthetic reasoning.**
   If the architecture says something should be ergonomic and an
   operator repeatedly trips over it, take that seriously. One trip
   is a receipt, not an automatic rewrite. It becomes architecture
   after it repeats, survives a probe, and still cannot be explained
   by missing skill, docs, or probes. Law 2 still holds: a complaint
   is an observation.

8. **Preserve provenance.**
   Observation → evidence → interpretation → decision →
   implementation → validation. Otherwise six months later you retain
   the strange constraint but forget why it exists.

9. **Promote abstractions slowly.**
   Local fix → repeated pattern → demonstrated generality →
   lower-layer primitive. This is the Gesso / Palette / Cyan
   discipline. See **Layer gate** below.

10. **Every development loop ends with another research question.**
    That question is the seed of the *next* spiral.

11. **Win condition for this spiral.**
    Every load-bearing question for this problem has a disposition,
    and you have produced a bounded roadmap *for this problem*
    (explicit non-goals, first implementation slice if the answer is
    build). Reject and "stays in Palette/Cyan/Lab" count as done.
    Congratulations.

---

## What you emit (pick one or more)

A spiral produces artifacts. It does not start a PR unless Develop
was in scope **and** a closed contract exists.

| Artifact | When |
|---|---|
| Disposition | always: `BUILD` / `PARK` / `REJECT` / `PALETTE` / `CYAN` / `LAB` / `GESSO LATER` |
| Parking-lot note | architecture or research that is not this week's code |
| Experiment design / prototype | a cheap question is still open |
| Closed recipe | someone is about to implement (owner, permitted files, tests, non-goals) |
| Remaining questions | the next spiral's start |

A roadmap is not a `/goal`. A `/goal` is a closed recipe.

Precision follows the mode:

    exploration      semantic elasticity
    architecture     explicit boundaries
    experiment       operational definitions
    implementation   exact contracts
    verification     zero ambiguity

---

## Layer gate

Before anything moves **down** a layer, all three must be yes:

    Generality        would a developer want this without Cyan or Palette?
    Composability     can it be expressed without agents, context windows,
                      playbooks, or Palette sessions?
    Ecosystem leverage
                      does centralizing it cut duplicated glue across
                      real components of that layer?

Any no: keep it in the layer that hurt. Being low-level is not
enough. Scarcity of Gesso fallout from a Cyan trial is a successful
boundary.

---

## Loop sizes

**Micro** — a known bug.

    research bug → plan repair → implement → test → learn

**Meso** — one subsystem question (a Gesso phase, a Palette primitive).

    research question → closed recipe → one slice → operate → learn

**Macro** — a problem space.

    research space → architecture → build subsystem → operate →
    study emergence → redesign

Default to meso. Micro is for repairs. Macro is for "what is this
whole thing." Do not redesign the stack to fix a telemetry field.

---

## Time-box and collisions

Cap a spiral: the load-bearing questions named at kickoff, at most
one throwaway prototype, one written disposition. Overflow is the
next spiral.

Do not reopen a spiral another lead already closed unless you have
**new primary evidence**. Agent confidence is not evidence. Citation
means having read it. Testimony yields to transcripts and logs. No
performance claim without a receipt from the repo under test.

---

## Handoff into Develop

RPD output does not implement the product.

Implementation starts from a closed contract in the owning repo
(`docs/goals/` in Gesso; the equivalent work-item form elsewhere).
The contract names owner, permitted files, tests, and non-goals.

Until that exists, stay in Research and Plan.
