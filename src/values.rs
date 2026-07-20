//! Material value tables — per-(variant, kind-letter), in centipoints.
//!
//! Each side's pieces are valued under **that side's** variant tables
//! (ADR-0015 §5); the cross-variant capture asymmetry of *Core Playing
//! Principles* §4 then emerges from the sums: material captured by an
//! inert-tray side (chess, xiongqi) simply vanishes, while material captured
//! by an ōgi side reappears in its hand term. The tables are compile-time
//! defaults — tunable data, scaled by [`crate::EvalWeights`].

use sashite_sanki_engine::domain::piece::Piece;
use sashite_sanki_engine::domain::variant::Variant;

/// Board value of a piece kind under `variant`, in centipoints. The royal is
/// `0` (it is never exchanged — royal safety is its own term). Unknown
/// letters (malformed positions) are `0` rather than a panic.
#[must_use]
pub fn board_value(variant: Variant, kind_letter: char) -> i32 {
    match variant {
        Variant::Chess => match kind_letter {
            'P' => 100,
            'N' => 320,
            'B' => 330,
            'R' => 500,
            'Q' => 900,
            _ => 0, // 'K' and unknown letters
        },
        Variant::Ogi => match kind_letter {
            'F' => 100,
            'T' => 250, // the promoted Fu (gold-like reach)
            'N' => 320,
            'B' => 330,
            'R' => 500,
            'I' => 800, // Princess
            _ => 0,     // 'K' and unknown letters
        },
        Variant::Xiongqi => match kind_letter {
            'S' => 100,
            'N' => 320,
            'B' => 330,
            'R' => 500,
            'E' => 900, // Empress
            _ => 0,     // 'G' and unknown letters
        },
    }
}

/// Hand value of a held piece kind (ōgi only — the caller never asks for an
/// inert tray): droppable potential, a large fraction of the board value. A
/// hand only ever holds the five demoted droppables (`F`, `R`, `B`, `N`,
/// `I` — a captured Tokin demotes to a Fu), so the table has exactly five
/// entries; anything else is `0`.
#[must_use]
pub fn hand_value(kind_letter: char) -> i32 {
    match kind_letter {
        'F' => 85,
        'N' => 270,
        'B' => 280,
        'R' => 425,
        'I' => 680,
        _ => 0,
    }
}

/// The value of `piece` on the board, under its **owner's** variant.
#[must_use]
pub fn piece_board_value(piece: Piece, owner_variant: Variant) -> i32 {
    board_value(owner_variant, piece.kind_letter())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn royals_are_never_countable_material() {
        assert_eq!(board_value(Variant::Chess, 'K'), 0);
        assert_eq!(board_value(Variant::Ogi, 'K'), 0);
        assert_eq!(board_value(Variant::Xiongqi, 'G'), 0);
    }

    #[test]
    fn hand_table_has_exactly_the_five_droppables() {
        for letter in ['F', 'R', 'B', 'N', 'I'] {
            assert!(hand_value(letter) > 0);
        }
        for letter in ['T', 'K', 'Q', 'P', 'S', 'E', 'G', 'X'] {
            assert_eq!(hand_value(letter), 0);
        }
    }

    #[test]
    fn hand_value_is_a_fraction_of_board_value() {
        for letter in ['F', 'R', 'B', 'N', 'I'] {
            assert!(hand_value(letter) < board_value(Variant::Ogi, letter));
        }
    }
}
