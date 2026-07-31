//! Static evaluation — one function, integer centipoints, from the
//! side-to-move's viewpoint (ADR-0015 §5).
//!
//! Seven terms, each scaled by its [`EvalWeights`] percentage: board
//! material (per-variant tables), ōgi hand material (droppable potential;
//! inert trays count zero), advancement/placement, royal safety,
//! foot-soldier structure, the mover's mobility (its move list is already
//! paid for by the search), and contempt (a draw-leaf score handled in the
//! search, not here). The cross-variant capture asymmetry of *Core Playing
//! Principles* §4 emerges from the material terms alone.

use sashite_sanki_engine::domain::piece::Piece;
use sashite_sanki_engine::domain::side::Side;
use sashite_sanki_engine::domain::square::Square;
use sashite_sanki_engine::domain::variant::Variant;
use sashite_sanki_engine::legality::check::in_check;
use sashite_sanki_engine::position::Position;

use crate::api::EvalWeights;
use crate::values::{hand_value, piece_board_value};

/// Apply a percentage weight to a raw term.
fn scaled(raw: i32, weight: i32) -> i32 {
    raw.saturating_mul(weight).wrapping_div(100)
}

/// A signed sum helper: adds for `side == plus`, subtracts otherwise.
fn signed(total: i32, amount: i32, side: Side, plus: Side) -> i32 {
    if side == plus {
        total.saturating_add(amount)
    } else {
        total.saturating_sub(amount)
    }
}

/// The rank progress of `square` from `side`'s home toward promotion
/// (`0..=7` on the 8×8 board).
fn advancement(square: Square, side: Side) -> i32 {
    let rank = i32::from(square.rank());
    match side {
        Side::First => rank,
        Side::Second => i32::from(Square::RANK_COUNT)
            .saturating_sub(1)
            .saturating_sub(rank),
    }
}

/// Distance-to-center bonus base: `3 − min(dist_file, 3)` where `dist_file`
/// is the file's distance to the d/e files (mirrored for ranks).
fn centrality(square: Square) -> i32 {
    let file = i32::from(square.file());
    let rank = i32::from(square.rank());
    let df = (file.saturating_mul(2).saturating_sub(7))
        .abs()
        .wrapping_div(2);
    let dr = (rank.saturating_mul(2).saturating_sub(7))
        .abs()
        .wrapping_div(2);
    3_i32.saturating_sub(df.max(dr).min(3))
}

/// Static evaluation of `position` from the **side to move's** viewpoint.
/// `mover_moves` is the mover's legal-move count at this node — the search
/// already generated the list, so the mobility term costs nothing extra.
#[must_use]
pub fn evaluate(position: &Position, weights: &EvalWeights, mover_moves: usize) -> i32 {
    let mover = position.active_side();

    let mut material = 0_i32;
    let mut psq = 0_i32;
    let mut structure_seen: [[u8; 8]; 2] = [[0; 8]; 2]; // foot-soldiers per (side, file)
    let mut royal_square: [Option<Square>; 2] = [None, None];

    for square in Square::all() {
        let Some(piece) = position.piece_at(square) else {
            continue;
        };
        let side = piece.side();
        let owner_variant = position.variant_of(side);
        material = signed(
            material,
            piece_board_value(piece, owner_variant),
            side,
            mover,
        );

        if piece.is_royal() {
            if let Some(slot) = royal_square.get_mut(side_index(side)) {
                *slot = Some(square);
            }
            continue;
        }

        // Placement: centre bias for every piece, advancement for the
        // foot-soldier class (promotion-zone pull).
        let mut placement = centrality(square).saturating_mul(4);
        if piece.is_foot_soldier() {
            placement = placement.saturating_add(advancement(square, side).saturating_mul(6));
            if let Some(row) = structure_seen.get_mut(side_index(side)) {
                if let Some(count) = row.get_mut(usize::from(square.file())) {
                    *count = count.saturating_add(1);
                }
            }
        }
        psq = signed(psq, placement, side, mover);
    }

    // Hand material: droppable potential for ōgi sides; inert trays are
    // simply not represented in `Position::hand`, and a non-ōgi side's hand
    // is empty by construction — the loop is a no-op there.
    let mut hand = 0_i32;
    for side in [Side::First, Side::Second] {
        if position.variant_of(side) != Variant::Ogi {
            continue;
        }
        let mut total = 0_i32;
        for (piece, count) in position.hand(side) {
            let count = i32::try_from(count).unwrap_or(i32::MAX);
            total = total.saturating_add(hand_value(piece.kind_letter()).saturating_mul(count));
        }
        hand = signed(hand, total, side, mover);
    }

    // Foot-soldier structure: doubled penalty, connected (adjacent-file) bonus.
    let mut structure = 0_i32;
    for side in [Side::First, Side::Second] {
        let Some(row) = structure_seen.get(side_index(side)) else {
            continue;
        };
        let mut term = 0_i32;
        for file in 0..8_usize {
            let here = i32::from(row.get(file).copied().unwrap_or(0));
            if here >= 2 {
                term = term.saturating_sub(here.saturating_sub(1).saturating_mul(12));
            }
            if here > 0 {
                let left = file
                    .checked_sub(1)
                    .and_then(|f| row.get(f))
                    .copied()
                    .unwrap_or(0);
                let right = row.get(file.saturating_add(1)).copied().unwrap_or(0);
                if left > 0 || right > 0 {
                    term = term.saturating_add(6);
                }
            }
        }
        structure = signed(structure, term, side, mover);
    }

    // Royal safety: shelter (friendly neighbours around the royal) and an
    // in-check nudge against the mover.
    let mut royal_safety = 0_i32;
    for side in [Side::First, Side::Second] {
        let Some(Some(royal)) = royal_square.get(side_index(side)).copied() else {
            continue;
        };
        let mut shelter = 0_i32;
        for df in -1_i8..=1 {
            for dr in -1_i8..=1 {
                if df == 0 && dr == 0 {
                    continue;
                }
                let Some(neighbour) = royal.offset(df, dr) else {
                    continue;
                };
                if position
                    .piece_at(neighbour)
                    .is_some_and(|p| p.belongs_to(side))
                {
                    shelter = shelter.saturating_add(8);
                }
            }
        }
        royal_safety = signed(royal_safety, shelter, side, mover);
    }
    let opponent_variant = position.variant_of(mover.flip());
    if in_check(mover, opponent_variant, |sq| position.piece_at(sq)) {
        royal_safety = royal_safety.saturating_sub(25);
    }

    let mobility = i32::try_from(mover_moves)
        .unwrap_or(i32::MAX)
        .saturating_mul(2);

    scaled(material, weights.material)
        .saturating_add(scaled(hand, weights.hand))
        .saturating_add(scaled(psq, weights.psq))
        .saturating_add(scaled(royal_safety, weights.royal_safety))
        .saturating_add(scaled(structure, weights.structure))
        .saturating_add(scaled(mobility, weights.mobility))
}

const fn side_index(side: Side) -> usize {
    match side {
        Side::First => 0,
        Side::Second => 1,
    }
}

/// Exchange-ordering value of capturing `victim`, under the **capturing
/// side's** variant economics (ADR-0015 §3): board removal — valued under
/// the *victim owner's* tables, what leaves the opponent — plus the hand
/// gain when the capturer plays ōgi (the captured piece reappears droppable:
/// an ōgi victim demotes/flips, a foreign one collapses to a Fu).
#[must_use]
pub fn capture_gain(position: &Position, capturer_side: Side, victim: Piece) -> i32 {
    let victim_variant = position.variant_of(victim.side());
    let removal = piece_board_value(victim, victim_variant);
    if position.variant_of(capturer_side) != Variant::Ogi {
        return removal;
    }
    let held_letter = if victim_variant == Variant::Ogi {
        // An ōgi piece flips colour into the hand; a Tokin demotes to a Fu.
        match victim.kind_letter() {
            'T' => 'F',
            letter => letter,
        }
    } else {
        // A foreign capture collapses to an ōgi Fu (*Core Playing
        // Principles* §4).
        'F'
    };
    removal.saturating_add(hand_value(held_letter))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::classify::capture_victim;
    use sashite_sanki_engine::domain::half_move::Move;
    use sashite_sanki_engine::engine;

    fn pos(feen: &str) -> Position {
        Position::parse(feen).expect("fixture FEEN must parse")
    }

    // --- in_check parameter order (reliability review, 2026-07-31) -----
    //
    // `in_check(side, opponent_variant, piece_at)` dispatches the ATTACKING
    // side's pieces through `opponent_variant`'s table. Empirically, most
    // piece letters (K, G, Q, R, B, N, I, E, T) are dispatched purely by
    // their own letter in `sashite_sanki_engine::movement::attack` -- the
    // variant argument never actually changes their geometry there, because
    // each letter already belongs to exactly one variant by construction
    // (only xiongqi ever has `G`/`E`, only ōgi `I`/`T`, only chess `Q`).
    // The one letter class where `variant` really does change the geometry
    // is the foot soldier (`P`/`F`/`S` share one dispatch function that
    // switches on `variant`): a xiongqi Soldier attacks sideways past the
    // river, a chess Pawn never does. That makes the foot-soldier case the
    // genuine test of parameter order below (confirmed by direct
    // experiment: swapping the variant on the flying-general fixture that
    // follows does NOT change its outcome, so it could not have caught a
    // backwards parameter order by itself).

    #[test]
    fn in_check_nudge_fires_only_for_the_real_variant_of_the_attacker() {
        // Second's xiongqi Soldier on d4 has crossed the river (Second's
        // gate: `rank <= 3`) and so attacks sideways onto c4/e4 -- but ONLY
        // under xiongqi rules; the same letter read as a chess Pawn only
        // attacks diagonally forward (c3/e3), never sideways.
        let checked = pos("g^7/8/8/8/3sK^3/8/8/8 / W/c");
        assert_eq!(checked.active_side(), Side::First);
        let opponent_variant = checked.variant_of(Side::First.flip());
        assert_eq!(opponent_variant, Variant::Xiongqi);
        assert!(in_check(Side::First, opponent_variant, |s| checked.piece_at(s)));
        // Proof the fixture is discriminating: the swapped (mover's own)
        // variant must miss this exact check.
        assert!(!in_check(Side::First, Variant::Chess, |s| checked.piece_at(s)));

        let weights = EvalWeights::default();
        // `mover_moves` pinned identically for both calls below: isolates
        // the in-check term from the mobility term, which would otherwise
        // also shift (the King has strictly fewer safe squares once
        // adjacent to the Soldier).
        let score_checked = evaluate(&checked, &weights, 0);

        // Safe: the King moved out of the Soldier's reach (different file
        // and rank from the soldier's sideways/straight attack squares).
        let safe = pos("g^7/8/8/8/3s3K^/8/8/8 / W/c");
        assert!(!in_check(Side::First, opponent_variant, |s| safe.piece_at(s)));
        let score_safe = evaluate(&safe, &weights, 0);

        // The only other per-position terms (material, psq, structure) are
        // unaffected by the King's own square (royals score 0 material and
        // are excluded from psq/structure), and shelter is 0 for both (no
        // friendly neighbours in either fixture) -- so the gap is exactly
        // the flat 25-centipoint in-check nudge at the default 100% weight.
        assert_eq!(
            score_safe.saturating_sub(score_checked),
            25,
            "checked={score_checked} safe={score_safe}"
        );
    }

    #[test]
    fn in_check_nudge_fires_for_a_mixed_pairing_flying_general() {
        // Chess (First) King on the open e-file from a xiongqi (Second)
        // General: a mixed-pairing check the crate must also get right,
        // even though (per the comment above) it does not by itself
        // discriminate the `in_check` parameter order.
        let checked = pos("4g^3/8/8/8/8/8/8/4K^3 / W/c");
        let opponent_variant = checked.variant_of(Side::Second);
        assert!(in_check(Side::First, opponent_variant, |s| checked.piece_at(s)));
        let weights = EvalWeights::default();
        let moves_checked = engine::legal_moves(&checked);
        let score_checked = evaluate(&checked, &weights, moves_checked.len());

        // Safe: King off both the General's file and rank.
        let safe = pos("4g^3/8/8/8/8/8/8/K^7 / W/c");
        assert!(!in_check(Side::First, opponent_variant, |s| safe.piece_at(s)));
        let moves_safe = engine::legal_moves(&safe);
        let score_safe = evaluate(&safe, &weights, moves_safe.len());

        assert!(
            score_checked < score_safe,
            "checked={score_checked} safe={score_safe}"
        );
    }

    // --- capture_gain (mirrors sashite-sanki-engine's `capture_transform`
    // exactly) -------------------------------------------------------------

    #[test]
    fn capture_gain_ogi_capturing_ogi_tokin_demotes_then_flips() {
        let position = pos("4k^3/8/8/3t4/8/8/8/3RK^3 / J/j");
        let m = Move::parse(r#"["d1","d5",null]"#).expect("valid move");
        let (_, victim) = capture_victim(&position, &m).expect("capture");
        assert_eq!(victim.kind_letter(), 'T');

        let predicted = capture_gain(&position, Side::First, victim);
        assert_eq!(
            predicted,
            piece_board_value(victim, Variant::Ogi) + hand_value('F')
        );

        let next = engine::apply(&position, &m).expect("legal capture");
        let held: Vec<(Piece, usize)> = next.hand(Side::First).collect();
        assert_eq!(held.len(), 1);
        let (held_piece, count) = held.first().copied().expect("one entry");
        assert_eq!(held_piece.kind_letter(), 'F', "the Tokin demotes to a Fu");
        assert_eq!(held_piece.side(), Side::First, "droppable by the capturer");
        assert_eq!(count, 1);
    }

    #[test]
    fn capture_gain_ogi_capturing_a_foreign_piece_becomes_a_fu() {
        let position = pos("4k^3/8/8/3s4/8/8/8/3RK^3 / J/c");
        let m = Move::parse(r#"["d1","d5",null]"#).expect("valid move");
        let (_, victim) = capture_victim(&position, &m).expect("capture");
        assert_eq!(victim.kind_letter(), 'S');

        let predicted = capture_gain(&position, Side::First, victim);
        assert_eq!(
            predicted,
            piece_board_value(victim, Variant::Xiongqi) + hand_value('F')
        );

        let next = engine::apply(&position, &m).expect("legal capture");
        let held: Vec<(Piece, usize)> = next.hand(Side::First).collect();
        assert_eq!(held.len(), 1);
        let (held_piece, _count) = held.first().copied().expect("one entry");
        assert_eq!(held_piece.kind_letter(), 'F');
        assert_eq!(held_piece.side(), Side::First);
    }

    #[test]
    fn capture_gain_non_ogi_capturer_is_inert_board_value_only() {
        // Chess (capturer) taking a xiongqi piece: identity, no hand term --
        // the capture_transform doc's "chess or xiongqi capturer: identity
        // (the opponent's case is kept, hence an inert hand)".
        let position = pos("4k^3/8/8/3s4/8/8/8/3RK^3 / W/c");
        let m = Move::parse(r#"["d1","d5",null]"#).expect("valid move");
        let (_, victim) = capture_victim(&position, &m).expect("capture");

        let predicted = capture_gain(&position, Side::First, victim);
        assert_eq!(
            predicted,
            piece_board_value(victim, Variant::Xiongqi),
            "no hand gain: the capturer is not ōgi"
        );
    }

    // --- hand material term ------------------------------------------------

    #[test]
    fn hand_material_term_is_exactly_additive() {
        let weights = EvalWeights::default();
        let baseline = pos("7k^/8/8/8/8/8/8/K^7 / J/j");
        let with_rook_in_hand = pos("7k^/8/8/8/8/8/8/K^7 R/ J/j");
        // `mover_moves` pinned identically for both calls: a droppable Rook
        // also changes the real legal-move count, which would otherwise
        // confound the comparison with the mobility term.
        let score_base = evaluate(&baseline, &weights, 0);
        let score_hand = evaluate(&with_rook_in_hand, &weights, 0);
        assert_eq!(
            score_hand.saturating_sub(score_base),
            hand_value('R'),
            "the hand term must add exactly hand_value('R')"
        );
    }

    // --- EvalWeights coverage gap (reliability review, 2026-07-31) --------
    //
    // Every existing weight-related test above uses `EvalWeights::default`
    // (every term at 100%); none confirms that a weight of exactly `0`
    // *fully* disables its term, as the type's own doc comment promises,
    // rather than merely diminishing it (a `wrapping_div` slip, e.g., could
    // leave a residual instead of an exact zero).

    #[test]
    fn material_weight_zero_fully_disables_the_term() {
        // Two positions differing only by one extra Black pawn on d5. Every
        // OTHER weight is pinned at 0 too, so with material alone active the
        // gap must be exactly one pawn's board value, and with material ALSO
        // at 0 the gap must collapse to exactly zero -- not merely shrink.
        let baseline = pos("k^7/8/8/8/8/8/8/Q3K^3 / W/w");
        let extra_pawn = pos("k^7/8/8/3p4/8/8/8/Q3K^3 / W/w");

        let material_only = EvalWeights {
            material: 100,
            hand: 0,
            psq: 0,
            royal_safety: 0,
            structure: 0,
            mobility: 0,
            contempt: 0,
        };
        let gap = evaluate(&baseline, &material_only, 0).saturating_sub(evaluate(
            &extra_pawn,
            &material_only,
            0,
        ));
        assert_eq!(
            gap,
            crate::values::board_value(Variant::Chess, 'P'),
            "with every other term off, the gap must be exactly one pawn's value"
        );

        let material_off_too = EvalWeights {
            material: 0,
            ..material_only
        };
        assert_eq!(
            evaluate(&baseline, &material_off_too, 0),
            evaluate(&extra_pawn, &material_off_too, 0),
            "material weight 0 must fully disable the term, not merely diminish it"
        );
    }
}
