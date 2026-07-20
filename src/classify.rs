//! Move classification from position + geometry (ADR-0015 §3).
//!
//! A [`Move`] carries no capture flag and an ōgi promotion is invisible in
//! the move (`actor` is absent), so ordering and quiescence derive both
//! facts here: destination occupancy for ordinary captures, the en-passant
//! patterns of *Core Playing Principles* §4 for captures onto an empty
//! square, and foot-soldier-reaches-last-rank for the forced ōgi Tokin.
//!
//! The en-passant read is the **marker-based** one the engine applies
//! (deciders' confirmation, 2026-07-19): the victim beside/behind the
//! destination must carry the `-` capturability flag — an unmarked
//! neighbour means the step is quiet.

use sashite_sanki_engine::domain::half_move::Move;
use sashite_sanki_engine::domain::piece::Piece;
use sashite_sanki_engine::domain::side::Side;
use sashite_sanki_engine::domain::square::Square;
use sashite_sanki_engine::domain::variant::Variant;
use sashite_sanki_engine::movement::forward;
use sashite_sanki_engine::position::Position;

/// The victim of `mv` on `position`, if the move captures: the occupied
/// destination, or the marker-attested en-passant victim square. `None` for
/// a quiet move — and always for a drop (*Core Playing Principles* §3: a
/// drop never captures).
#[must_use]
pub fn capture_victim(position: &Position, mv: &Move) -> Option<(Square, Piece)> {
    let Move::Board { from, to, .. } = mv else {
        return None;
    };
    let mover = position.piece_at(*from)?;
    let side = mover.side();
    let opponent: Side = side.flip();

    // Ordinary capture: the destination holds an opponent piece.
    if let Some(occupant) = position.piece_at(*to) {
        if occupant.belongs_to(opponent) {
            return Some((*to, occupant));
        }
        return None;
    }

    // En passant onto an empty square, chess pattern: a pawn stepping one
    // square diagonally forward; the victim sits beside the source, on the
    // destination file.
    let mover_variant = position.variant_of(side);
    if !mover.is_foot_soldier() {
        return None;
    }
    let df = i16::from(to.file()).saturating_sub(i16::from(from.file()));
    let dr = i16::from(to.rank()).saturating_sub(i16::from(from.rank()));
    let fwd = i16::from(forward(side));
    let victim_square = match mover_variant {
        Variant::Chess if df.abs() == 1 && dr == fwd => Square::new(to.file(), from.rank()),
        // Xiongqi pattern: a sideways step; the victim sits one rank behind
        // the destination (from the mover's perspective).
        Variant::Xiongqi if df.abs() == 1 && dr == 0 => {
            let behind = i16::from(to.rank()).saturating_sub(fwd);
            u8::try_from(behind)
                .ok()
                .and_then(|rank| Square::new(to.file(), rank))
        }
        _ => None,
    }?;
    let victim = position.piece_at(victim_square)?;
    // The `-` marker is the single capturability signal (frontend-logic
    // §En passant): unmarked ⇒ the sideways/diagonal step is quiet.
    if victim.belongs_to(opponent) && victim.is_foot_soldier() && victim.is_diminished() {
        return Some((victim_square, victim));
    }
    None
}

/// Whether `mv` promotes on `position`: an explicit `actor` on a board move
/// (chess / xiongqi choice), or an ōgi foot-soldier reaching its last rank
/// (forced Tokin, invisible in the move).
#[must_use]
pub fn is_promotion(position: &Position, mv: &Move) -> bool {
    let Move::Board { from, to, actor } = mv else {
        return false;
    };
    if actor.is_some() {
        return true;
    }
    let Some(mover) = position.piece_at(*from) else {
        return false;
    };
    if !mover.is_foot_soldier() || position.variant_of(mover.side()) != Variant::Ogi {
        return false;
    }
    let last_rank: u8 = match mover.side() {
        Side::First => Square::RANK_COUNT.saturating_sub(1),
        Side::Second => 0,
    };
    to.rank() == last_rank
}

/// Whether `mv` resets the 100-half-move clock: a capture, or a board move
/// of a foot-soldier (tested on the **source** piece, so a promoting push
/// still resets; the promoted forms — Tokin included — are not in the
/// `P`/`F`/`S` class). A drop never resets. The predicate mirrors the
/// kernel's `terminal::move_limit::clock_resets` exactly (ADR-0015 §4).
#[must_use]
pub fn resets_halfmove_clock(position: &Position, mv: &Move) -> bool {
    if capture_victim(position, mv).is_some() {
        return true;
    }
    let Move::Board { from, .. } = mv else {
        return false;
    };
    position.piece_at(*from).is_some_and(Piece::is_foot_soldier)
}
