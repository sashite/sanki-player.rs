# Changelog

All notable changes to this crate are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [0.4.1] — 2026-07-31

### Fixed

- **`Searcher::dead_gate` was inverted for mixed pairings.** The gate that
  arms the dead-position probe required *both* sides to be non-ōgi
  (`variants.first != Ogi && variants.second != Ogi`) before ever calling
  `engine::status`, when the only pairing with no dead-position rule at all
  is *pure* ōgi (`ogi_performs_no_detection`, engine). Any mixed pairing
  with exactly one ōgi side — e.g. ōgi versus chess on bare royals — fell
  through to the ordinary material evaluation instead of the drawn `0`,
  silently reporting a dead position as a material edge. Fixed by widening
  the condition to `||`: the gate now stays armed for pure chess, pure
  xiongqi, and every mixed pairing, and disarms only for pure ōgi.
- **`dead_draw`'s material gate missed the one unbounded dead-position
  rule.** The cheap pre-filter skipped the authoritative `engine::status`
  probe whenever more than two non-royal pieces remained on the board — but
  pure chess's "Kings and same-coloured Bishops only" rule
  (`chess_material_is_dead`, engine) has no bound on how many Bishops are
  involved. Three or more same-coloured Bishops and otherwise bare Kings —
  a genuine, engine-confirmed dead position — were scored as a material
  edge instead of the drawn `0`. Fixed by adding a `DeadGateMaterial` shape
  (non-royal count plus an "all Bishops" flag) and exempting it from the
  `> 2` cutoff.
- **Unbounded `EvalWeights::contempt` could let a draw outrank a real
  mate.** `contempt` is caller-supplied and was folded into the draw-leaf
  score with no clamp; a sufficiently draw-attracted persona could inflate
  a forced draw's score past `MATE` itself, so the root could choose a
  mundane repetition over an available, immediate checkmate — and
  separately, `Choice::eval_cp` could exceed `MATE`, breaking the
  documented invariant that mate scores are the largest possible ones.
  Fixed with a `NON_MATE_BOUND` clamp (one below the smallest score
  `tt.rs`'s `MATE_WINDOW` treats as mate-distance-dependent) applied to
  every non-mate leaf: the draw score and quiescence's stand-pat.

### Added

- Regression tests for the three fixes above:
  `mixed_pairing_dead_position_is_recognized_despite_an_ogi_side`,
  `pure_ogi_dead_gate_stays_off`,
  `dead_draw_gate_recognizes_more_than_two_same_coloured_bishops`, and
  `extreme_contempt_never_lets_a_draw_outrank_a_real_mate`.
- Coverage-closure tests found while auditing for this release: xiongqi en
  passant against the engine's own capture (both mover sides), `EvalWeights`
  per-term isolation including the `0`-fully-disables-a-term case,
  transposition-table mate-score ply-adjustment round-trips at the
  `MATE_WINDOW` boundary, the anytime/limits contract at its edges
  (`max_depth: 0`, `max_nodes: Some(0)`, a stop firing mid-iteration,
  `max_depth: u8::MAX`), `tt_capacity: 0` matching a populated table in
  result and not just performance, and three further `root_tiebreak`
  fixtures (two independent three-way ties, an exact mate-in-2 score
  through a real transposition table).

## [0.4.0] — 2026-07-31

### Changed

- **`sashite-sanki-engine` bumped to 0.8.** Fixes a checkmate that could be
  misreported as `Ongoing` when a cross-variant capture leaves an inert,
  opposite-cased token sitting in the capturer's hand tray (full root cause
  in the engine's own 0.8.0 changelog). The crate reaches the engine only
  through `engine::status`/`apply`/`legal_moves`, so `choose` now sees the
  corrected terminal status at every node it visits, without any code
  change here — behaviour can differ on positions that hit this pattern.
- **`tests/root_tiebreak.rs`'s chess-versus-ōgi fixture now accepts two
  keys.** The same engine bug that could hide a real checkmate had, on this
  fixture's position, the opposite symptom: it suppressed recognition of a
  second winning line (`c2→c3`), making the position merely look uniquely
  solved by `c2→b1`. Under the corrected engine both moves are confirmed
  mates in 2 (a brute-force AND/OR search agrees); the fixture was never
  wrong to expect a mate, only wrong about which move(s) qualify.

## [0.3.1] — 2026-07-31

### Fixed

- **Root move tie-break could return a move that does not mate.** `choose`'s
  per-iteration root loop searched every move through a window narrowed by
  the running `alpha`, so a move that failed high in its child came back with
  an **upper bound**, not a value — and that bound could land exactly on the
  eventual best score. The seeded tie-break (ADR-0015 §6) trusted every
  equal-best score as a value, so a losing move could tie a real mate and be
  picked in its place. Observed on a chess-versus-ōgi position with a unique
  mate in 2: three of four seeds threw it away for a non-mating move scored
  identically.
- Root moves are now searched in two passes: the usual narrowed window first,
  tracking which scores are exact (raised `alpha`) versus a bound; then a
  second, open-window re-search of only the entries tied at the best score
  and not yet exact. Only genuine ties pay for the re-search — a position
  with no ambiguous bound costs exactly what it did before.

### Added

- **`tests/root_tiebreak.rs`** — three fixtures, each a position with exactly
  one mating move, checked across 12 seeds × 2 transposition-table capacities
  (24 searches per fixture). `#[ignore]`d: run with
  `cargo test --release -- --ignored`.

## [0.3.0] — 2026-07-27

### Changed

- **`sashite-sanki-engine` bumped to 0.7** — castling extended to ōgi and
  xiongqi (deciders' ruling, 2026-07-27; FIDE mechanics, the xiongqi General
  `G^` in the King's role; canonical initial FEENs gain the `-R` corner
  markers). The crate reaches the engine only through
  `engine::status`/`apply`/`legal_moves`, so the search now explores the new
  castlings without any code change here.

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
