# sashite-sanki-player

Pure, anytime **move-decision** crate for the Sanki game suite (chess, ōgi and
xiongqi on a shared 8×8 board), built for [Sashité](https://sashite.com/) —
the "brain" behind the player-bot fleet, and the third pure crate of the
family after `sashite-sanki-engine` (the rules) and `sashite-sanki-arbiter`
(the protocol semantics).

A classic, complete v1 search: iterative deepening, fail-soft alpha-beta
(negamax), move ordering (transposition move, captures by asymmetric exchange
value, promotions, killer moves, history heuristic), quiescence on captures
and promotions, and a bounded transposition table keyed by the engine's own
canonical FEEN. The engine's public API is the **only** board representation —
children come from `engine::apply`, legality from `engine::legal_moves` — so
there is zero rule duplication and zero divergence surface with the arbiter.

The crate is **pure**: no I/O, no clock, no ambient randomness. All timing
lives behind the caller's `should_stop` hook (anytime search); with a node
budget instead, the result is a deterministic function of its inputs. The
`seed` breaks ties among equal-best root moves — the fleet's between-sessions
variety mechanism, reproducible game by game.

```rust
use std::collections::BTreeMap;
use sashite_sanki_player::{choose, Context, Limits, Position, Strength};

// A bare-kings-plus-rook endgame; first player (chess) to move.
let position = Position::parse("4k^3/8/8/8/8/8/8/R3K^3 / W/w")?;

// The game history the rules make relevant: canonical-FEEN occurrence
// counts (threefold repetition) and the half-move clock — the same
// bookkeeping as the engine's `kernel::SessionState`.
let mut occurrences = BTreeMap::new();
occurrences.insert(position.to_feen(), 1);

let ctx = Context {
    position: &position,
    occurrences: &occurrences,
    halfmove_clock: 0,
};
let strength = Strength { max_depth: 3, ..Strength::default() };
let limits = Limits { max_nodes: Some(50_000), should_stop: None };

let choice = choose(&ctx, &strength, &limits).ok_or("no legal move")?;
assert!(choice.depth >= 1);
assert_eq!(choice.pv.first(), Some(&choice.mv));
# Ok::<(), Box<dyn std::error::Error>>(())
```

## Contract highlights

- `choose` returns `None` **iff** the position has no legal move. The
  depth-1 iteration is exempt from `should_stop` and `max_nodes` (normative):
  it is cheap and bounded, and running it to completion is what guarantees a
  legal position always yields a move, however late the stop fires.
- With `should_stop = None`, the output is a pure function of
  `(Context, Strength, max_nodes)`. Wall-clock interruption is inherently
  non-deterministic and documented as such; tests use node budgets.
- Draw offers and resignation are **not** decided here: `assess` returns a
  score; thresholds and temperament live in the calling bot's persona
  (ADR-0014). The crate stays a game oracle, not a policy engine.
- History-aware draw scoring: the search overlays its path on the caller's
  occurrence counts and scores repetition and move-limit lines as draws,
  `contempt`-anchored at the root side. Path-dependent draw scores are never
  stored in the transposition table.
- Evaluation is one function parameterized by `EvalWeights` (styles are
  weight profiles, never injected blunders), with per-variant value tables:
  the cross-variant capture asymmetry of the *Core Playing Principles* —
  ōgi captures gain droppable hand material, chess/xiongqi trays are inert —
  emerges from the material terms.

## License

Apache-2.0 — see `LICENSE` and `NOTICE`.
