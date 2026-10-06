# Handoff — Gaea programme audit, and preview mode built

Session of 2026-08-04 (overnight). Goal was to audit and red-team
[docs/integrations/gaea-programme.md](../../integrations/gaea-programme.md), then run it out. Steps 1 and 2 of §8 are
done; step 3 onward needs Matt.

---

## Read this first

**One retraction.** Mid-session I recorded "the headless Gaea build does not
reproduce". **That was wrong.** Gaea runs fine under Proton — v2.3.0.1 confirmed
by capturing 1664 bytes of `--help`. What is broken is *observability*: console
output cannot reach a non-interactive session, and redirecting it to see the
error triggers blocker 4 (`IOException: Invalid handle`) and crashes the build.
Full detail in §3.3 of the programme.

**The one thing only you can do:** press `Tab` in the viewer and confirm the
world picker looks right. Its list logic is unit-tested, but nothing here can
send a keypress. Everything else below was verified by running it.

---

## What changed

### Four defects found in code and fixed

1. **Every Gaea-native resolution was unmeshable.** The viewer needs
   `(resolution - 1) % 4 == 0`; Gaea builds powers of two and `2^k - 1` never
   is. **Every preview the importer had ever written would have been rejected at
   load**, and §4 was blocked before it began. `import_heightfield.py` now
   resamples (1024→1025) and records `source_resolution` / `resampled`.
2. **Min/max normalisation silently rescaled every world.** Builds occupying 30%
   and 90% of the 16-bit range imported *identically* — the exact silent-rescale
   failure the module's own docstring said it existed to prevent. Now full-scale
   mapping; `--normalize` is opt-in and the choice is recorded, because the same
   bytes under the two rules are two different worlds and the digest cannot tell
   them apart.
3. **Non-square input was accepted**, writing a manifest resolution that
   disagreed with the buffer length. Refused at read time now.
4. **Neither Gaea module had a single test**, in a repo where all 32 others do.

### Built

| file | what |
|---|---|
| `pipeline/preview_metrics.py` | radial monotonicity, symmetry residual, slope, sink density. Schema `codeweald.preview-metrics/v1`, `"certifies": false` |
| `tests/test_gaea_import.py` | 14 cases |
| `tests/test_preview_metrics.py` | 13 cases |
| `world_core/apps/world_viewer/src/main.rs` | preview load path + world picker, 12 new Rust tests |

### Test state

- **Rust: 20/20** (`cargo test -p codeweald-world-viewer`) — 8 pre-existing plus
  12 new.
- **Python: 565 OK** (`python3 -m unittest discover -s tests`, 194 s) — the
  previous 552 plus the 13 new preview-metrics cases. No regressions.

---

## Design findings now written into the programme

- **§5.5 — the basin macro-shape is the massif defect in polar coordinates.**
  Relief rising with distance from centre forbids foothills in front of peaks
  along every radial, exactly as distance-from-lanes did. It survives only
  because the bias is applied *before* erosion, which is an empirical claim, so
  it is now measured. **Gate on the mean, not the fully-monotonic fraction** —
  measured 0.42 with no bias against 0.69 with a 28% basin bias, while the
  fully-monotonic fraction sat at 0.0% for both. A gate on the latter would have
  passed a substantially bowl-shaped world.
- **§5.1 — symmetry must be composited before erosion.** Blending a rotated copy
  afterwards averages two independent drainage networks; rivers meet their own
  mirror head-on. The spec's stated preference was backwards. Measurement is
  separable from imposition, so measure the residual either way. Also: a
  180°-symmetric field **always has zero gradient at the map centre** (verified),
  so every such map has a permanent critical point there — design with it.
- **§5.2** — three least-cost runs between one pair of keeps return one lane, not
  three. Diversity has to be an explicit constraint.
- **§5.6** — "local edits" were unbounded, permitting the forbidden operation by
  increments. Now a declared, gated budget.
- **§4** — previews could not be measured, yet step 3 required judging them.
  Resolved by a preview metric set that explicitly cannot certify.
- **§7** — Gaea's reproducibility was assumed, its version was an undeclared
  input, and there was no stated behaviour for a broken Proton stack.

---

## Gaea invocation traps (cost an hour; do not rediscover)

- **Filenames with spaces break argument parsing** — Swarm dumps usage and exits
  without building. **All three bundled examples with a `SaveDefinition` have
  spaces in their names.** Copy to a space-free path first.
- **A relative graph path** exits 0 in ~6 s writing nothing.
- **A real build is slow and looks exactly like a hang** — a heavy example at 512
  ran 15 min at ~1 core. The 29 s figure does not generalise.
- **Never trust a Swarm exit code.** Exit 0 with no files, exit 0 after a usage
  dump, and exit 1 were all observed. Check that output files appeared.

---

## Next

3. **Re-establish a reproducible headless build** (yours — two minutes at a real
   terminal). Then build 3–4 example graphs and fly them.
   - 3a. Build one graph twice, diff the digests: decides whether heightfields
     are derived or source artifacts (§6).
   - 3b. Measure radial monotonicity and symmetry residual on real Gaea output to
     calibrate the gates against erosion rather than synthetic noise.
4. **Author the WGE graph** in the GUI — basin macro-shape, symmetric composite
   *before* erosion, exposed variables, marked height export.
5. The driver, then symmetry (§5.1), then lane routing (§5.2) with the edit
   budget in place *before* the first edit.

**Reproduce tonight's previews:**

```bash
python3 pipeline/import_heightfield.py <heightfield.png> \
  --output <dir>/preview_name --world-m 800 --relief-m 300
python3 pipeline/preview_metrics.py <dir>/preview_name
world_core/target/release/codeweald-world-viewer \
  --batch <dir>/preview_name --worlds-root <dir>
```

The two synthetic previews used for verification are **scratchpad-only and will
not survive** — they were fractal noise, not Gaea, and deliberately not committed
so nothing mistakes them for evidence about Gaea's output.
