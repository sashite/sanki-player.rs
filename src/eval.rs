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
