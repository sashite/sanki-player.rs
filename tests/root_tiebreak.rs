//! Regression guard for the root-move tie-break bug (0.3.0 and earlier):
//! `choose`'s per-iteration root loop narrowed the search window by the
//! running `alpha`, so a move that failed high in its child came back with
//! an **upper bound**, not a value. That bound could land exactly on the
//! eventual best score, and the seeded tie-break (ADR-0015 §6) trusted every
//! equal-best score as a value — so a losing move could tie a real mate and
//! get picked in its place.
//!
//! Each fixture below has one or more genuinely mating moves; the assertion
//! is that `choose` always lands on one of them, regardless of the tie-break
//! seed or the transposition table's capacity (`0` disables it; a real
//! capacity exercises early store/probe) — never on a move that merely tied
//! their score without truly winning.
//!
//! The chess-versus-ōgi fixture accepts **two** keys, not one: under
//! `sashite-sanki-engine` 0.7 this position had a single key (`c2→b1`), but
//! 0.8 — which fixed a checkmate hidden behind a cross-variant inert tray —
//! revealed that `c2→c3` forces mate in 2 just as well. The same engine bug
//! that once hid a *real* checkmate from this crate's own `choose` had, on
//! this position, the opposite symptom: it hid a *second* winning line,
//! making the position look more uniquely solved than it truly is. Both moves
//! are confirmed mates now (a brute-force AND/OR check over
//! `sashite-sanki-engine` 0.8 agrees); a seed landing on either is correct —
//! this fixture is exactly what it should be, proof that a genuine tie
//! between winners never lets a loser in.
//!
//! `#[ignore]`: the fixtures are middlegame-sized and the sweep is 24
//! searches per position — too slow for an unoptimized default `cargo test`.
//! Run with `cargo test --release -- --ignored`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeMap;

use sashite_sanki_player::{
    assess, choose, Context, EvalWeights, Limits, Move, Occurrences, Position, Strength, MATE,
};

/// As [`assert_mate_survives_every_seed_and_tt`], but also sweeping
/// `max_depth` over `depths` (the original fixtures pin only depth 4).
fn assert_mate_survives_every_seed_tt_and_depth(feen: &str, keys_content: &[&str], depths: &[u8]) {
    let position = Position::parse(feen).expect("fixture FEEN must parse");
    let mut occurrences: Occurrences = BTreeMap::new();
    occurrences.insert(position.to_feen(), 1);
    let ctx = Context {
        position: &position,
        occurrences: &occurrences,
        halfmove_clock: 0,
    };
    let keys: Vec<Move> = keys_content
        .iter()
        .map(|content| Move::parse(content).expect("fixture move must parse"))
        .collect();

    for seed in 0..12_u64 {
        for tt_capacity in [0_usize, 1 << 18] {
            for &max_depth in depths {
                let strength = Strength {
                    max_depth,
                    tt_capacity,
                    seed,
                    ..Strength::default()
                };
                let limits = Limits {
                    max_nodes: Some(5_000_000),
                    should_stop: None,
                };
                let choice = choose(&ctx, &strength, &limits).expect("a legal move exists");
                assert!(
                    choice.eval_cp > MATE - 1000,
                    "seed {seed}, tt {tt_capacity}, depth {max_depth}: eval {} is not a mate score",
                    choice.eval_cp
                );
                assert!(
                    keys.contains(&choice.mv),
                    "seed {seed}, tt {tt_capacity}, depth {max_depth}: picked {:?}, not one of the confirmed mates {:?}",
                    choice.mv,
                    keys
                );
            }
        }
    }
}

fn assert_mate_survives_every_seed_and_tt(feen: &str, keys_content: &[&str]) {
    assert_mate_survives_every_seed_tt_and_depth(feen, keys_content, &[4]);
}

#[test]
#[ignore]
fn mate_survives_chess_vs_ogi() {
    assert_mate_survives_every_seed_and_tt(
        "k^5r1/2Q1R3/f7/3b4/8/1PfB4/+PB-K^N1+P+P+P/7R 10f2nbir/ W/j",
        &[r#"["c2","b1",null]"#, r#"["c2","c3",null]"#],
    );
}

#[test]
#[ignore]
fn mate_survives_chess_vs_chess() {
    assert_mate_survives_every_seed_and_tt(
        "r1b2bnr/+p2+p3+p/p3k^p2/q1P1p2Q/3PP2N/P1P5/1B3+P+P+P/-RN2K^2+R 2pn/B W/w",
        &[r#"["h5","e8",null]"#],
    );
}

#[test]
#[ignore]
fn mate_survives_xiongqi_vs_chess() {
    assert_mate_survives_every_seed_and_tt(
        "4k^3/5r1+p/7S/5N2/2p1qS2/B7/8/5G^1R 5p2b2nqr/6SBENR w/C",
        &[r#"["e4","e2",null]"#],
    );
}

// --- extended coverage (reliability review, 2026-07-31) -------------------
//
// The four fixtures above each have exactly one (or, since the 0.4.0 engine
// bump, two) mating moves in a middlegame-sized position, always at max_depth
// 4. The tests below deliberately vary structure the originals do not:
// endgame-sized (near-bare-board) positions instead of middlegame ones,
// THREE-way tied keys instead of one or two, a genuine tie at a non-mate
// score (the historical bug was mate-specific; the fix is not), and a sweep
// of max_depth far outside the pinned value of 4 (both the degenerate
// max_depth=1 case and depths past 4). Every mate fixture's key set was
// independently confirmed offline with a small from-scratch AND/OR search
// over `sashite-sanki-engine` directly (not this crate, and not a search --
// exhaustive enumeration of every reply at every ply), reporting every move
// that forces mate at the position's minimal depth rather than stopping once
// uniqueness is settled (unlike `sashite-sanki-puzzle-composer`'s prover,
// which exists to prove uniqueness and stops as soon as it finds a second
// key) -- so, unlike the fixtures above, these were composed by hand rather
// than mined from self-play, specifically to get more than two tied keys.

#[test]
#[ignore]
fn mate_survives_three_way_tie_queen_and_rook_backrank() {
    // Black King h8 alone; White King g6 (covers g7/h7); White Queen a2,
    // Rook b1. Three independent mate-in-1 keys: Qa2-a8 and Rb1-b8 both
    // deliver the back-rank check with g8 covered by the mating piece's own
    // rank reach; Rb1-h1 is a different mechanism entirely -- mate along the
    // open h-file, with g8 covered instead by the *unmoved* Queen's a2-g8
    // diagonal. A genuine three-way tie via two different mating patterns,
    // not three cosmetic variations of one idea.
    assert_mate_survives_every_seed_tt_and_depth(
        "7k^/8/6K^1/8/8/8/Q7/1R6 / W/w",
        &[
            r#"["a2","a8",null]"#,
            r#"["b1","b8",null]"#,
            r#"["b1","h1",null]"#,
        ],
        // Depth 6 was tried here first but, with a Queen and Rook both
        // still fully mobile against a bare King, its branching factor made
        // this one fixture dominate the whole file's runtime; depth 5
        // already exercises "past the pinned depth of 4" just as well.
        &[1, 2, 4, 5],
    );
}

#[test]
#[ignore]
fn mate_survives_three_way_tie_rook_ladder() {
    // Black King h8 alone; White King g6; White Rooks a3, b2, c1 -- a
    // "ladder" of three rooks any one of which alone covers the entire 8th
    // rank once it arrives there, with g7/h7 covered by the king. Unlike the
    // previous fixture, all three keys are the *same* mating idea from three
    // different files: the sharpest test of whether the root loop's ordering
    // (which key is searched, and in what position in the list, varies by
    // file) ever privileges one tied key over the others.
    assert_mate_survives_every_seed_tt_and_depth(
        "7k^/8/6K^1/8/8/R7/1R6/2R5 / W/w",
        &[
            r#"["a3","a8",null]"#,
            r#"["b2","b8",null]"#,
            r#"["c1","c8",null]"#,
        ],
        &[1, 2, 3, 5],
    );
}

#[test]
#[ignore] // slow in an unoptimized debug build (unbounded node budget, depth
          // up to 6); run with `cargo test --release -- --ignored` like the
          // rest of this file.
fn non_mate_tie_two_mirrored_quiet_knight_moves_score_identically() {
    // The documented bug (0.3.1) was specifically about a MATE score's
    // fail-soft upper bound coinciding with the best; the fix's reasoning
    // (only an *exact* tie at the max score is trusted) does not mention
    // mate at all, so it should generalize to an ordinary score too. Two
    // Knights, mirror-placed (b3/g3) and far from both bare Kings; under
    // material-only weights (every positional term zeroed) a quiet
    // (non-capturing) move never changes material, and this position has no
    // tactics reachable within a shallow horizon (nothing is attackable), so
    // b3-a5 and g3-h5 must score IDENTICALLY at every depth. Checked via a
    // direct, independent `assess` on each resulting child rather than
    // inferred from which move `choose` happens to land on: other quiet
    // moves (King steps) tie at the same score too, so the *pick* alone
    // would not pin this cleanly (see the companion test below for that
    // angle instead).
    let pos = position("4k^3/8/8/8/8/1N4N1/8/4K^3 / W/w");
    let left = mv(r#"["b3","a5",null]"#);
    let right = mv(r#"["g3","h5",null]"#);
    assert!(sashite_sanki_engine::engine::legal_moves(&pos).contains(&left));
    assert!(sashite_sanki_engine::engine::legal_moves(&pos).contains(&right));

    let material_only = EvalWeights {
        material: 100,
        hand: 0,
        psq: 0,
        royal_safety: 0,
        structure: 0,
        mobility: 0,
        contempt: 0,
    };

    for max_depth in [1_u8, 2, 3, 4, 5, 6] {
        for tt_capacity in [0_usize, 1 << 14] {
            let child_left = sashite_sanki_engine::engine::apply(&pos, &left).expect("legal");
            let child_right = sashite_sanki_engine::engine::apply(&pos, &right).expect("legal");
            let occ_left = occurrences_of(&child_left);
            let occ_right = occurrences_of(&child_right);
            let ctx_left = Context {
                position: &child_left,
                occurrences: &occ_left,
                halfmove_clock: 1,
            };
            let ctx_right = Context {
                position: &child_right,
                occurrences: &occ_right,
                halfmove_clock: 1,
            };
            let strength = Strength {
                max_depth,
                tt_capacity,
                seed: 0,
                weights: material_only,
            };
            let limits = Limits {
                max_nodes: None,
                should_stop: None,
            };
            let score_left = assess(&ctx_left, &strength, &limits);
            let score_right = assess(&ctx_right, &strength, &limits);
            assert_eq!(
                score_left, score_right,
                "depth {max_depth}, tt {tt_capacity}: mirrored quiet Knight moves must score identically"
            );
        }
    }
}

#[test]
#[ignore] // slow in an unoptimized debug build; see the previous test.
fn non_mate_tie_seed_only_varies_the_pick_not_the_eval() {
    // Companion to `tests/tactics.rs`'s `seeds_vary_only_the_equal_best_pick`
    // (which uses a dead-position draw, where literally every move ties at
    // 0): here material is unbalanced (White is up two Knights) and the
    // position is very much alive, yet every *quiet* root move still ties
    // under material-only weights. If the rescue pass ever let a non-tied
    // bound through, some seed/tt/depth combination would report a different
    // `eval_cp` than the others.
    let pos = position("4k^3/8/8/8/8/1N4N1/8/4K^3 / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let material_only = EvalWeights {
        material: 100,
        hand: 0,
        psq: 0,
        royal_safety: 0,
        structure: 0,
        mobility: 0,
        contempt: 0,
    };
    let mut evals = std::collections::BTreeSet::new();
    let mut moves = std::collections::BTreeSet::new();
    for seed in 0..24_u64 {
        for tt_capacity in [0_usize, 1 << 14] {
            let strength = Strength {
                max_depth: 4,
                tt_capacity,
                seed,
                weights: material_only,
            };
            let limits = Limits {
                max_nodes: None,
                should_stop: None,
            };
            let choice = choose(&ctx, &strength, &limits).expect("a legal move exists");
            evals.insert(choice.eval_cp);
            moves.insert(format!("{:?}", choice.mv));
        }
    }
    assert_eq!(
        evals.len(),
        1,
        "every quiet root move ties under material-only weights: {evals:?}"
    );
    assert!(
        moves.len() >= 2,
        "seeds should reach distinct equal-best moves: {moves:?}"
    );
}

#[test]
#[ignore]
fn mate_in_two_scores_exactly_mate_minus_three_plies_through_a_real_tt() {
    // Task 2 (tt.rs mate-score ply-adjustment review): the fixtures above
    // only check that `choose` lands on a *confirmed* mating move and that
    // the score is loosely "some mate score" (`eval_cp > MATE - 1000`). This
    // test pins the exact value, which is what actually exercises
    // `tt::to_stored`/`from_stored` rather than merely their sign.
    //
    // `mate_survives_chess_vs_ogi`'s position (reused here) is a genuine
    // mate in 2 (3 plies) -- independently confirmed offline with the same
    // from-scratch AND/OR oracle mentioned in this file's other comments,
    // not assumed from the fixture's name. A real `choose()` run stores and
    // probes real `tt::Entry` values at real plies across iterative
    // deepening; if the ply-adjustment arithmetic had any drift, the
    // tt-enabled runs below would disagree with the `tt_capacity: 0` run
    // (which never ply-adjusts anything, since nothing is ever stored) or
    // with the independently-known mate distance, at some seed or depth.
    let feen = "k^5r1/2Q1R3/f7/3b4/8/1PfB4/+PB-K^N1+P+P+P/7R 10f2nbir/ W/j";
    let expected = MATE - 3;
    let position = Position::parse(feen).expect("fixture FEEN must parse");
    let mut occurrences: Occurrences = BTreeMap::new();
    occurrences.insert(position.to_feen(), 1);
    let ctx = Context {
        position: &position,
        occurrences: &occurrences,
        halfmove_clock: 0,
    };
    // The score is a pure function of the two-pass alpha-beta search and does
    // not depend on the tie-break seed at all (the seed only ever chooses
    // among `equal_best` moves) -- two seeds is enough to also catch any
    // accidental seed-coupling into the score itself, without paying for a
    // wider sweep that would not exercise anything the formula tests above
    // do not already cover in isolation.
    for seed in 0..2_u64 {
        for tt_capacity in [0_usize, 1 << 16] {
            // 3 plies is the minimal depth the mate can be seen at all; 5
            // forces at least one extra iterative-deepening round so the
            // table is probed against entries stored by a shallower pass.
            for max_depth in [3_u8, 5] {
                let strength = Strength {
                    max_depth,
                    tt_capacity,
                    seed,
                    ..Strength::default()
                };
                let limits = Limits {
                    max_nodes: Some(5_000_000),
                    should_stop: None,
                };
                let choice = choose(&ctx, &strength, &limits).expect("a legal move exists");
                assert_eq!(
                    choice.eval_cp, expected,
                    "seed {seed}, tt {tt_capacity}, depth {max_depth}: mate-in-2 must score exactly MATE - 3, not just \"a mate score\""
                );
            }
        }
    }
}

fn position(feen: &str) -> Position {
    Position::parse(feen).expect("fixture FEEN must parse")
}

fn occurrences_of(position: &Position) -> Occurrences {
    let mut occurrences = BTreeMap::new();
    occurrences.insert(position.to_feen(), 1);
    occurrences
}

fn mv(content: &str) -> Move {
    Move::parse(content).expect("fixture move must parse")
}
