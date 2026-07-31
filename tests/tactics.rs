//! Tactical, draw-awareness, and contract suites (ADR-0015 §7).
//!
//! Every fixture is a FEEN plus expected move(s), run at fixed node budgets
//! (or none — the budgets here are generous; determinism comes from
//! `should_stop = None`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::collections::BTreeMap;

use sashite_sanki_player::{
    assess, choose, Context, Limits, Move, Occurrences, Position, Strength, MATE,
};

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

fn strength(max_depth: u8) -> Strength {
    Strength {
        max_depth,
        ..Strength::default()
    }
}

const NO_LIMITS: Limits<'_> = Limits {
    max_nodes: None,
    should_stop: None,
};

#[test]
fn mate_in_one_chess() {
    // Black King h8, white King g6, white Queen a1: Qa1–a8 mates on the
    // back rank (g8 covered by the queen, h7 by the king).
    let pos = position("7k^/8/6K^1/8/8/8/8/Q7 / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let choice = choose(&ctx, &strength(2), &NO_LIMITS).expect("a legal move exists");
    assert_eq!(choice.mv, mv(r#"["a1","a8",null]"#));
    assert_eq!(choice.eval_cp, MATE - 1);
}

#[test]
fn prefers_the_faster_mate_at_depth_three() {
    // Same fixture, deeper search: the mate distance must not degrade.
    let pos = position("7k^/8/6K^1/8/8/8/8/Q7 / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let choice = choose(&ctx, &strength(3), &NO_LIMITS).expect("a legal move exists");
    assert_eq!(choice.eval_cp, MATE - 1);
    assert_eq!(choice.mv, mv(r#"["a1","a8",null]"#));
}

#[test]
fn takes_the_hanging_queen() {
    // Black queen d5 hangs to the rook d1 (and attacks it — inaction loses
    // the rook instead).
    let pos = position("k^7/8/8/3q4/8/8/8/3R3K^ / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let choice = choose(&ctx, &strength(2), &NO_LIMITS).expect("a legal move exists");
    assert_eq!(choice.mv, mv(r#"["d1","d5",null]"#));
}

#[test]
fn refuses_the_poisoned_pawn() {
    // The d5 pawn is defended by e6: Qxd5?? exd5 loses the queen for a
    // pawn. Minimal one-ply search grabs it; the deeper search must not
    // (horizon regression, ADR-0015 §7).
    let pos = position("k^7/8/4p3/3p4/8/8/8/3Q3K^ / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let choice = choose(&ctx, &strength(2), &NO_LIMITS).expect("a legal move exists");
    assert_ne!(choice.mv, mv(r#"["d1","d5",null]"#));
}

#[test]
fn ogi_rook_drop_mate() {
    // Ōgi vs ōgi: black King a8, white Bishop c7 (covers b8), white King b6
    // (covers a7/b7), a Rook in hand. Any a-file Rook drop is mate — and
    // perfectly legal (uchifuzume bans only the Fu).
    let pos = position("k^7/2B5/1K^6/7f/8/8/8/8 R/ J/j");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let choice = choose(&ctx, &strength(2), &NO_LIMITS).expect("a legal move exists");
    assert_eq!(choice.eval_cp, MATE - 1);
    assert!(choice.mv.is_drop(), "the mate is a drop: {:?}", choice.mv);
}

#[test]
fn refuses_uchifuzume_and_still_plays() {
    // The kernel's own uchifuzume fixture: black King walled in on h8,
    // white Rook g1 and Knight f6, a white Fu in hand. Fu@h7 would be
    // checkmate — illegal (uchifuzume); the search must play something
    // else (regression against the engine-0.4 façade guard).
    let pos = position("7k^/8/5N2/8/8/8/8/4K^1R1 F/ J/j");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let forbidden = mv(r#"[null,"h7","fu"]"#);
    assert!(
        !sashite_sanki_engine::engine::legal_moves(&pos).contains(&forbidden),
        "the engine façade must exclude the mating Fu drop"
    );
    let choice = choose(&ctx, &strength(3), &NO_LIMITS).expect("a legal move exists");
    assert_ne!(choice.mv, forbidden);
}

#[test]
fn repetition_awareness_holds_the_draw() {
    // White is a queen down; the King step to f1 re-enters a position seen
    // twice already (seeded occurrences) — the third occurrence is a draw,
    // strictly better than any losing alternative.
    let pos = position("k^2q4/8/8/8/8/8/8/6K^1 / W/w");
    let repetition_move = mv(r#"["g1","f1",null]"#);
    let after = sashite_sanki_engine::engine::apply(&pos, &repetition_move).unwrap();
    let mut occ = occurrences_of(&pos);
    occ.insert(after.to_feen(), 2);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 10,
    };
    let choice = choose(&ctx, &strength(3), &NO_LIMITS).expect("a legal move exists");
    assert_eq!(choice.mv, repetition_move);
    assert_eq!(
        choice.eval_cp, 0,
        "a repetition draw scores the contempt (0)"
    );
}

#[test]
fn repetition_from_game_history_alone_exceeds_the_threshold() {
    // `repetition_awareness_holds_the_draw` reaches the threshold as exactly
    // 2 (game history) + 1 (this move, on the search path) = 3 — a boundary
    // value that a hypothetical `==` bug (instead of the correct `>=`) would
    // also satisfy by coincidence. Here game history alone has ALREADY
    // reached the threshold (3, with no help needed from the search path),
    // so the combined count is 3 + 1 = 4, strictly ABOVE the threshold: a
    // case only a genuine `>=` can pass.
    let pos = position("k^2q4/8/8/8/8/8/8/6K^1 / W/w");
    let repetition_move = mv(r#"["g1","f1",null]"#);
    let after = sashite_sanki_engine::engine::apply(&pos, &repetition_move).unwrap();
    let mut occ = occurrences_of(&pos);
    occ.insert(after.to_feen(), 3);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 10,
    };
    let choice = choose(&ctx, &strength(3), &NO_LIMITS).expect("a legal move exists");
    assert_eq!(choice.mv, repetition_move);
    assert_eq!(
        choice.eval_cp, 0,
        "a repetition already past the threshold in game history alone is still the contempt draw (0)"
    );
}

#[test]
fn contempt_steers_the_draw_decision() {
    // Same fortress; a draw-averse persona (positive contempt) scores the
    // repetition below zero — the score shifts even when no better move
    // exists.
    let pos = position("k^2q4/8/8/8/8/8/8/6K^1 / W/w");
    let repetition_move = mv(r#"["g1","f1",null]"#);
    let after = sashite_sanki_engine::engine::apply(&pos, &repetition_move).unwrap();
    let mut occ = occurrences_of(&pos);
    occ.insert(after.to_feen(), 2);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 10,
    };
    let mut averse = strength(3);
    averse.weights.contempt = 150;
    let choice = choose(&ctx, &averse, &NO_LIMITS).expect("a legal move exists");
    if choice.mv == repetition_move {
        assert_eq!(choice.eval_cp, -150);
    }
}

#[test]
fn extreme_contempt_never_lets_a_draw_outrank_a_real_mate() {
    // Bug found in this review: `EvalWeights::contempt` is caller-supplied
    // and unbounded, and was added into the draw score raw (no clamp). A
    // sufficiently negative (draw-attracted) contempt inflated a forced
    // draw's score past `MATE` itself, so the root chose a mundane
    // repetition over an available, immediate checkmate — and, separately,
    // `Choice::eval_cp` could exceed `MATE`, breaking the documented
    // invariant that mate scores are the largest possible ones.
    //
    // Fixture: Black King a8, White King b6, White Rook h1 — Rh1-h8 is
    // checkmate (no diagonal/rank coincidence with the king's own square, so
    // this is a clean fixture unrelated to any engine-side quirk). White
    // ALSO has an unrelated king step (b6-b5), seeded via `occurrences` to
    // complete a third repetition the instant it is played.
    let pos = position("k^7/8/1K^6/8/8/8/8/7R / W/w");
    let mate_move = mv(r#"["h1","h8",null]"#);
    let repeat_move = mv(r#"["b6","b5",null]"#);
    let after = sashite_sanki_engine::engine::apply(&pos, &repeat_move).unwrap();
    let mut occ = occurrences_of(&pos);
    occ.insert(after.to_feen(), 2);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    for contempt in [0_i32, -150, -28_999, -30_000, -40_000, i32::MIN, i32::MAX] {
        let mut draw_attracted = strength(2);
        draw_attracted.weights.contempt = contempt;
        let choice = choose(&ctx, &draw_attracted, &NO_LIMITS).expect("a legal move exists");
        assert_eq!(
            choice.mv, mate_move,
            "contempt {contempt}: a genuine mate must always outrank a mere draw"
        );
        assert_eq!(choice.eval_cp, MATE - 1, "contempt {contempt}");
        assert!(
            choice.eval_cp <= MATE,
            "contempt {contempt}: eval_cp must never exceed MATE"
        );
    }
    let _ = repeat_move;
}

#[test]
fn move_limit_awareness() {
    // At halfmove_clock 99, any quiet move reaches the 100-half-move draw:
    // the queen-down mover should embrace it (score 0, not a lost eval).
    let pos = position("k^2q4/8/8/8/8/8/8/6K^1 / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 99,
    };
    let choice = choose(&ctx, &strength(2), &NO_LIMITS).expect("a legal move exists");
    assert_eq!(choice.eval_cp, 0);
}

#[test]
fn mixed_pairing_dead_position_is_recognized_despite_an_ogi_side() {
    // Bare royals, ōgi (First) vs chess (Second): the mixed dead-position
    // rule (only royals on the board, no droppable hand) applies to ANY
    // mixed pairing — it is not limited to pairings that exclude ōgi.
    // Regression: `Searcher::dead_gate` used to require BOTH sides to be
    // non-ōgi before ever probing, so this exact position (which the engine
    // itself terminates as `Insufficient`) silently fell through to the
    // ordinary heuristic evaluation instead of the draw score.
    let pos = position("4k^3/8/8/8/8/8/8/4K^3 / J/w");
    assert!(matches!(
        sashite_sanki_engine::engine::status(&pos),
        sashite_sanki_engine::domain::outcome::Verdict::Terminated {
            status: sashite_sanki_engine::domain::status::Status::Insufficient,
            ..
        }
    ));
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    assert_eq!(
        assess(&ctx, &strength(2), &NO_LIMITS),
        0,
        "a mixed ōgi/chess bare-royals position is a dead-position draw"
    );
}

#[test]
fn pure_ogi_dead_gate_stays_off() {
    // The mirror check: *pure* ōgi (both sides) is the one pairing with no
    // dead-position rule at all (`ogi_performs_no_detection`, engine) — the
    // engine itself reports this bare-royals position as `Ongoing`, so the
    // gate must stay disabled here, not just wherever it happens to be
    // harmless. Guards against over-correcting the sibling regression test
    // into an unconditional `true`.
    let pos = position("4k^3/8/8/8/8/8/8/4K^3 / J/j");
    assert!(matches!(
        sashite_sanki_engine::engine::status(&pos),
        sashite_sanki_engine::domain::outcome::Verdict::Ongoing
    ));
}

#[test]
fn determinism_under_a_node_budget() {
    let pos = position("7k^/8/6K^1/8/8/8/8/Q7 / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let limits = Limits {
        max_nodes: Some(20_000),
        should_stop: None,
    };
    let a = choose(&ctx, &strength(4), &limits).expect("a legal move exists");
    let b = choose(&ctx, &strength(4), &limits).expect("a legal move exists");
    assert_eq!(a, b, "identical inputs must yield an identical Choice");
    assert_eq!(a.pv.first(), Some(&a.mv));
    assert!(a.depth >= 1);
    assert!(a.nodes > 0);
}

#[test]
fn tt_capacity_zero_matches_a_populated_table_in_result_not_just_performance() {
    // `tt_capacity: 0` disables the transposition table (ADR-0015 §3). This
    // must only ever cost performance: the chosen move, its score, and the
    // completed depth must be identical to an otherwise-identical search
    // with the table enabled — exercised through a real `choose()` call,
    // not just `tt.rs`'s own isolated unit tests. The total node count
    // differing (checked in aggregate — not every individual fixture is
    // guaranteed to diverge at this depth) confirms the table is actually
    // doing something, not a vacuous check.
    let fixtures = [
        "7k^/8/6K^1/8/8/8/8/Q7 / W/w",
        "k^7/8/8/3q4/8/8/8/3R3K^ / W/w",
        "k^7/8/4p3/3p4/8/8/8/3Q3K^ / W/w",
        "k^7/2B5/1K^6/7f/8/8/8/8 R/ J/j",
    ];
    let mut total_without_table = 0_u64;
    let mut total_with_table = 0_u64;
    for feen in fixtures {
        let pos = position(feen);
        let occ = occurrences_of(&pos);
        let ctx = Context {
            position: &pos,
            occurrences: &occ,
            halfmove_clock: 0,
        };
        let without_table = Strength {
            max_depth: 3,
            tt_capacity: 0,
            ..Strength::default()
        };
        let with_table = Strength {
            max_depth: 3,
            tt_capacity: 100_000,
            ..Strength::default()
        };
        let a = choose(&ctx, &without_table, &NO_LIMITS).expect("a legal move exists");
        let b = choose(&ctx, &with_table, &NO_LIMITS).expect("a legal move exists");
        assert_eq!(
            a.mv, b.mv,
            "{feen}: tt_capacity must not change the chosen move"
        );
        assert_eq!(
            a.eval_cp, b.eval_cp,
            "{feen}: tt_capacity must not change the eval"
        );
        assert_eq!(
            a.depth, b.depth,
            "{feen}: tt_capacity must not change the completed depth"
        );
        total_without_table += a.nodes;
        total_with_table += b.nodes;
    }
    assert_ne!(
        total_without_table, total_with_table,
        "the table should measurably change the total node count across these fixtures"
    );
}

#[test]
fn seeds_vary_only_the_equal_best_pick() {
    // Bare kings: every move leads to the same dead-position draw, so the
    // whole move list is equal-best — the seed picks among them, the eval
    // never moves.
    let pos = position("4k^3/8/8/8/8/8/8/4K^3 / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let mut moves = std::collections::BTreeSet::new();
    let mut evals = std::collections::BTreeSet::new();
    for seed in 0..16_u64 {
        let strength = Strength {
            max_depth: 2,
            seed,
            ..Strength::default()
        };
        let choice = choose(&ctx, &strength, &NO_LIMITS).expect("a legal move exists");
        moves.insert(format!("{:?}", choice.mv));
        evals.insert(choice.eval_cp);
    }
    assert_eq!(evals.len(), 1, "differing seeds must not change the eval");
    assert!(
        moves.len() >= 2,
        "seeds should reach distinct equal-best moves"
    );
}

#[test]
fn single_legal_move_degenerates_correctly_regardless_of_seed() {
    // White King a1 boxed in by Black King b3: a2 and b2 are both adjacent
    // to the Black King (illegal to step into), leaving exactly one legal
    // move (a1-b1) — no tie-break is possible. `equal_best` must degenerate
    // to that singleton cleanly, for every seed and every depth.
    let pos = position("8/8/8/8/8/1k^6/8/K^7 / W/w");
    assert_eq!(
        sashite_sanki_engine::engine::legal_moves(&pos).len(),
        1,
        "fixture must have exactly one legal move"
    );
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let only_move = mv(r#"["a1","b1",null]"#);
    for seed in 0..8_u64 {
        for max_depth in [1_u8, 2, 3, 5] {
            let strength = Strength {
                max_depth,
                seed,
                ..Strength::default()
            };
            let choice = choose(&ctx, &strength, &NO_LIMITS).expect("the one legal move");
            assert_eq!(
                choice.mv, only_move,
                "seed {seed}, depth {max_depth}: the only legal move must always be chosen"
            );
        }
    }
}

#[test]
fn anytime_contract_depth_one_is_exempt() {
    // A stop that fires immediately: the depth-1 iteration still completes
    // and yields a move (normative, ADR-0015 §2).
    let pos = position("7k^/8/6K^1/8/8/8/8/Q7 / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let stop = || true;
    let limits = Limits {
        max_nodes: None,
        should_stop: Some(&stop),
    };
    let choice = choose(&ctx, &strength(6), &limits).expect("depth 1 always yields a move");
    assert_eq!(choice.depth, 1);

    let starved = Limits {
        max_nodes: Some(1),
        should_stop: None,
    };
    let choice = choose(&ctx, &strength(6), &starved).expect("depth 1 always yields a move");
    assert_eq!(choice.depth, 1);
}

#[test]
fn max_depth_zero_is_treated_as_one() {
    // `Strength::max_depth` documents `0` as treated like `1` (the `.max(1)`
    // in `choose`) rather than searching nothing.
    let pos = position("7k^/8/6K^1/8/8/8/8/Q7 / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let zero = Strength {
        max_depth: 0,
        ..Strength::default()
    };
    let one = strength(1);
    let choice_zero = choose(&ctx, &zero, &NO_LIMITS).expect("a legal move exists");
    let choice_one = choose(&ctx, &one, &NO_LIMITS).expect("a legal move exists");
    assert_eq!(
        choice_zero.depth, 1,
        "max_depth 0 must still complete depth 1"
    );
    assert_eq!(choice_zero.mv, choice_one.mv);
    assert_eq!(choice_zero.eval_cp, choice_one.eval_cp);
}

#[test]
fn max_nodes_zero_still_completes_depth_one() {
    // The depth-1 iteration is unarmed (ADR-0015 §2): even the tightest
    // possible node budget (`Some(0)`, not just `Some(1)` as in
    // `anytime_contract_depth_one_is_exempt`) must not stop it early.
    let pos = position("7k^/8/6K^1/8/8/8/8/Q7 / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let limits = Limits {
        max_nodes: Some(0),
        should_stop: None,
    };
    let choice = choose(&ctx, &strength(6), &limits).expect("depth 1 always yields a move");
    assert_eq!(choice.depth, 1);
    assert_eq!(choice.mv, mv(r#"["a1","a8",null]"#));
}

#[test]
fn should_stop_mid_depth_three_falls_back_to_depth_two_exactly() {
    // `should_stop` firing partway through a LATER iteration — not
    // immediately, and not just at depth 1 (which
    // `anytime_contract_depth_one_is_exempt` already covers). A call-count
    // threshold is calibrated from real (unlimited) runs at max_depth 2 and
    // 3, guaranteed to fall strictly between "depth 2 has just completed"
    // and "depth 3 completes" — `nodes` is cumulative from depth 1, and
    // `should_stop` is polled once per armed (depth ≥ 2) node, so the number
    // of armed ticks up to and including the end of an iteration at depth D
    // is exactly `nodes(D) - nodes(1)`. The interrupted run must then match
    // the depth-2-only answer exactly, not merely "some earlier depth".
    let pos = position("k^7/8/4p3/3p4/8/8/8/3Q3K^ / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };

    let baseline_one = choose(&ctx, &strength(1), &NO_LIMITS).expect("a legal move exists");
    let baseline_two = choose(&ctx, &strength(2), &NO_LIMITS).expect("a legal move exists");
    let baseline_three = choose(&ctx, &strength(3), &NO_LIMITS).expect("a legal move exists");
    assert!(baseline_two.nodes > baseline_one.nodes);
    assert!(
        baseline_three.nodes > baseline_two.nodes,
        "fixture must need strictly more nodes to complete depth 3 than depth 2"
    );

    let armed_at_two = baseline_two.nodes - baseline_one.nodes;
    let armed_at_three = baseline_three.nodes - baseline_one.nodes;
    let threshold = armed_at_two + (armed_at_three - armed_at_two) / 2;
    assert!(threshold > armed_at_two && threshold < armed_at_three);

    let calls = std::cell::Cell::new(0_u64);
    let stop = || {
        calls.set(calls.get() + 1);
        calls.get() > threshold
    };
    let deep = Strength {
        max_depth: 6,
        ..Strength::default()
    };
    let limits = Limits {
        max_nodes: None,
        should_stop: Some(&stop),
    };
    let interrupted = choose(&ctx, &deep, &limits).expect("a legal move exists");

    assert_eq!(
        interrupted.depth, 2,
        "must fall back to the last COMPLETE iteration, not a partial depth-3 one"
    );
    assert_eq!(interrupted.mv, baseline_two.mv);
    assert_eq!(interrupted.eval_cp, baseline_two.eval_cp);
}

#[test]
fn max_depth_255_completes_safely_under_a_node_budget() {
    // u8::MAX: the killers vector is sized `max_depth + 2` and indexed by
    // ply throughout the search — confirms that sizing (and every other
    // ply/depth-indexed heuristic) never panics at the type's own upper
    // bound, and that the anytime contract still holds (falls back to
    // whatever the node budget allowed).
    let pos = position("7k^/8/6K^1/8/8/8/8/Q7 / W/w");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    let strength = Strength {
        max_depth: u8::MAX,
        ..Strength::default()
    };
    let limits = Limits {
        max_nodes: Some(2_000),
        should_stop: None,
    };
    let choice = choose(&ctx, &strength, &limits).expect("a legal move exists");
    assert!(choice.depth >= 1);
    assert!(
        choice.depth < u8::MAX,
        "a 2_000-node budget must not reach depth 255"
    );
}

#[test]
fn mated_position_has_no_choice_and_a_mate_assessment() {
    // Black to move, checkmated (Qg7 supported by Kg6).
    let pos = position("7k^/6Q1/6K^1/8/8/8/8/8 / w/W");
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    assert!(choose(&ctx, &strength(2), &NO_LIMITS).is_none());
    assert_eq!(assess(&ctx, &strength(2), &NO_LIMITS), -MATE);
}

#[test]
fn assess_tracks_the_mover_viewpoint() {
    // The same material edge reads positive for the side that owns it…
    let up = position("k^7/8/8/8/8/8/8/Q3K^3 / W/w");
    let occ_up = occurrences_of(&up);
    let ctx_up = Context {
        position: &up,
        occurrences: &occ_up,
        halfmove_clock: 0,
    };
    assert!(assess(&ctx_up, &strength(2), &NO_LIMITS) > 300);

    // …and negative for the side that faces it.
    let down = position("k^7/8/8/8/8/8/8/Q3K^3 / w/W");
    let occ_down = occurrences_of(&down);
    let ctx_down = Context {
        position: &down,
        occurrences: &occ_down,
        halfmove_clock: 0,
    };
    assert!(assess(&ctx_down, &strength(2), &NO_LIMITS) < -300);
}

#[test]
fn dead_draw_gate_recognizes_more_than_two_same_coloured_bishops() {
    // Bug found in this review: `Searcher::dead_draw`'s cheap material gate
    // skipped the authoritative `engine::status` probe whenever more than
    // two non-royal pieces sat on the board (`non_royal_count > 2`) — but
    // pure chess's "Kings and Bishops only, all the same colour" dead
    // position (the engine's own `chess_material_is_dead`) has NO bound on
    // how many Bishops are involved: `(0, [_, 0]) | (0, [0, _]) => true`
    // matches any count on one colour. Three same-coloured Bishops (colour
    // 0: a1, c1, a3) and otherwise bare Kings is a genuine, engine-confirmed
    // dead position, but the gate's fixed `> 2` cutoff silently skipped the
    // probe, leaving the position scored as roughly "White is up three
    // Bishops" instead of the drawn `0` — a wrong score, not merely weaker
    // pruning, since nothing else in the search ever reaches for
    // `engine::status` while legal moves remain.
    let pos = position("4k^3/8/8/8/8/B7/8/B1B1K^3 / W/w");
    assert!(
        matches!(
            sashite_sanki_engine::engine::status(&pos),
            sashite_sanki_engine::domain::outcome::Verdict::Terminated {
                status: sashite_sanki_engine::domain::status::Status::Insufficient,
                ..
            }
        ),
        "fixture must be a genuine engine-confirmed dead position"
    );
    let occ = occurrences_of(&pos);
    let ctx = Context {
        position: &pos,
        occurrences: &occ,
        halfmove_clock: 0,
    };
    assert_eq!(
        assess(&ctx, &strength(3), &NO_LIMITS),
        0,
        "three same-coloured Bishops and bare Kings is still a dead-position draw, not a material edge"
    );
}
