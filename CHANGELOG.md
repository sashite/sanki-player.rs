# Changelog

All notable changes to this crate are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.2.0] — 2026-07-22

### Changed

- **`sashite-sanki-engine` bumped to 0.6.** The engine's status vocabulary
  gains `Status::MoveCap` (the absolute 300-move / 600-half-move cap). The crate
  reaches the engine only through `engine::status`/`apply`/`legal_moves`, and
  its two `status` matches each carry a catch-all, so the new variant needs no
  code change and search behaviour is unchanged. The cap is a history-dependent
  terminal that the position-only `engine::status` never reports; modelling it
  in the search's draw scoring (alongside the 100-half-move limit) is left to a
  future evaluation pass.

## [0.1.0] — 2026-07-20

### Added

- **Initial release** (ADR-0015): a pure, single-threaded, anytime
  move-decision crate for the Sanki suite.
  - `choose` / `assess` over a `Context` (position + canonical-FEEN
    occurrence counts + half-move clock), a `Strength` persona
    (depth, transposition capacity, `EvalWeights`, tie-break seed) and
    caller-owned `Limits` (node budget, `should_stop` hook — the depth-1
    iteration is exempt, so a legal position always yields a move).
  - Iterative-deepening fail-soft alpha-beta (negamax) with quiescence on
    captures and promotions, killer moves, history heuristic, and a bounded
    transposition table keyed by the engine's canonical FEEN (mate scores
    ply-adjusted; path-dependent draw scores never stored).
  - One evaluation, weight-parameterized: per-variant board material, ōgi
    hand material (droppable potential — the cross-variant capture asymmetry
    of *Core Playing Principles* §4 emerges from the material terms),
    placement, royal safety, foot-soldier structure, mobility, contempt.
  - History-aware draw scoring: threefold repetition (game + search path)
    and the 100-half-move limit, both at the kernel's own thresholds; dead
    positions probed at every node behind a material gate.
  - Determinism by construction: canonical move-list sorting, `BTreeMap`
    state, no ambient randomness; the seed varies only the pick among
    equal-best root moves.
