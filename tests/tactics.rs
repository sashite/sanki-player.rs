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
