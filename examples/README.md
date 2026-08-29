# Measured balance — the nine pairings

Chess, Ōgi and Xiongqi each field their own army on one shared 8×8 board,
and any tradition can face any other — nine pairings, mirrors included.
Whether those pairings are *fair* deserves numbers rather than opinions.
This document is the numbers: measured, disclosed in full, and reproducible
bit for bit. The code that produced every figure is
[`balance.rs`](./balance.rs), in this directory.

## Method

Two studies over the nine pairings (first seat × second seat), played by
`sashite-sanki-engine` and `sashite-sanki-player` — the same rule kernel and
move-decision code that ships:

1. **Uniformly random play** (250 games per cell) — a structural control:
   under zero skill, no army should collapse.
2. **Self-play at depth 3** (100 games per cell), default evaluation
   weights, after **four random opening half-moves**. The players are
   deterministic, so the random openings are what makes games distinct;
   every game's transcript is hashed and the **Distinct** column counts the
   games that actually differ. A collapsed count would invalidate its row —
   here, every cell is 100 % distinct (3 150 games, all different).

Games run through the engine kernel, so every termination is the full rule
system's: checkmate, stalemate, no-move, dead position, threefold
repetition, the 100-half-move limit, the 600-half-move cap. Clocks are
neutralized (no timeouts). The first seat scores 1 per win and ½ per draw;
SE is the standard error of the per-game score.

Everything derives from the master seed `0xB920260829`: the study reproduces
**bit for bit** — verified across platforms (Apple Silicon/macOS and
x86-64/Linux produce identical tables, down to each cell's win–draw–loss
split and termination counts; the engine and player are integer arithmetic
throughout). An independent replication of the random study at 500 games per
cell agrees (all cells within 44.4–56.0 %).

## Results

## Uniformly random play

| First seat | Second seat | Games | Distinct | First score | ± SE | W–D–L | Mean plies | Terminations |
| --- | --- | ---: | ---: | ---: | ---: | :--- | ---: | :--- |
| chess | chess | 250 | 250 | 50.0 % | 1.2 | 18–214–18 | 342 | checkmate 36, insufficient 139, move_cap 2, move_limit 48, repetition 8, stalemate 17 |
| chess | ogi | 250 | 250 | 53.6 % | 1.2 | 28–212–10 | 505 | checkmate 38, insufficient 9, move_cap 116, move_limit 79, repetition 4, stalemate 4 |
| chess | xiongqi | 250 | 250 | 56.6 % | 1.7 | 56–171–23 | 290 | checkmate 79, insufficient 95, move_limit 50, repetition 5, stalemate 21 |
| ogi | chess | 250 | 250 | 46.6 % | 1.2 | 11–211–28 | 507 | checkmate 39, insufficient 6, move_cap 108, move_limit 89, repetition 5, stalemate 3 |
| ogi | ogi | 250 | 250 | 55.4 % | 2.6 | 100–77–73 | 393 | checkmate 173, move_cap 77 |
| ogi | xiongqi | 250 | 250 | 51.8 % | 1.9 | 50–159–41 | 378 | checkmate 91, insufficient 91, move_cap 11, move_limit 33, repetition 18, stalemate 6 |
| xiongqi | chess | 250 | 250 | 44.8 % | 1.8 | 27–170–53 | 290 | checkmate 80, insufficient 91, move_cap 1, move_limit 48, repetition 7, stalemate 23 |
| xiongqi | ogi | 250 | 250 | 47.2 % | 1.7 | 30–176–44 | 382 | checkmate 74, insufficient 110, move_cap 13, move_limit 31, repetition 10, stalemate 12 |
| xiongqi | xiongqi | 250 | 250 | 51.2 % | 1.7 | 38–180–32 | 248 | checkmate 70, insufficient 117, move_limit 14, repetition 8, stalemate 41 |



## Honest reading

**Under random play there is no structural imbalance.** Every cell sits
between 44.8 % and 56.6 %, and draws dominate everywhere except the ōgi
mirror. Three small but real signals: the chess army keeps a 3–6 point edge
whichever seat it takes; ōgi–xiongqi is nearly symmetric; and the ōgi mirror
shows a first-move edge (55.4 ± 2.6) — consistent with its nature, as the
only pairing with no insufficient-material draws (hands refill the board)
and 69 % checkmates even under random play.

**At depth 3 the chess army dominates its crossings** (68.0 and 76.5 % as
first seat, 75.5 and 75.0 % seen from the second), while **ōgi–xiongqi
stays balanced in both directions** (49.5 / 50.5) and the mirrors show a
mild first-move edge (51.5–56.0). What these figures do **not** establish:
that the domination is a property of the games rather than of the evaluation
function, which is chess-inspired. The evidence leans toward the evaluator:
under random play the chess edge is a few points; it is search guided by
that same evaluation that widens it.

These figures are published so that nobody sits down at an asymmetric
pairing unknowingly.

## Limits

Depth 3, one evaluation function, default weights, 100 self-play games per
cell (SE ≈ 4–5 points). The study measures engines, not humans. Stronger
claims await stronger engines — or real games; if yours contradict these
numbers, that is exactly the feedback this study exists to collect.

## Reproduce

```console
cargo run --release --example balance
```

Per-cell counts are env-overridable (`N_RANDOM`, `N_D3`; `0` skips a study).
The tables land in `out/b9-results.md`, with a per-cell checkpoint log in
`out/partial.log`. Same seed, same tables — on any machine.
