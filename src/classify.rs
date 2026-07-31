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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use sashite_sanki_engine::engine;

    fn pos(feen: &str) -> Position {
        Position::parse(feen).expect("fixture FEEN must parse")
    }

    fn mv(content: &str) -> Move {
        Move::parse(content).expect("fixture move must parse")
    }

    fn sq(s: &str) -> Square {
        Square::parse(s).expect("valid square")
    }

    // --- Xiongqi en passant (reliability review, 2026-07-31) ------------
    //
    // The sideways step's victim-square arithmetic had no direct test at
    // all. Each test below runs the SAME position and move through both
    // `engine::apply` (ground truth: what the real engine actually removes)
    // and `capture_victim` (what this crate believes is being captured, for
    // MVV-LVA ordering) and asserts they agree.

    #[test]
    fn xiongqi_en_passant_matches_the_engines_own_capture() {
        // The engine's own fixture (sashite-sanki-engine's
        // `engine::tests::xiongqi_en_passant_captures_end_to_end`): First's
        // Soldier g6 takes Second's just-double-stepped `-s` on f5 by
        // stepping sideways onto the skipped square f6.
        let position = pos("7g^/8/6S1/5-s2/8/8/8/G^7 / C/c");
        let m = mv(r#"["g6","f6",null]"#);
        assert!(engine::legal_moves(&position).contains(&m));

        let next = engine::apply(&position, &m).expect("legal en passant");
        assert!(
            next.piece_at(sq("f5")).is_none(),
            "the engine must remove the victim from f5: {}",
            next.to_feen()
        );
        assert_eq!(
            next.piece_at(sq("f6")).map(Piece::kind_letter),
            Some('S'),
            "the capturer stands on the skipped square"
        );

        let (square, victim) = capture_victim(&position, &m).expect("must classify as a capture");
        assert_eq!(
            square,
            sq("f5"),
            "classify must agree with the engine on the victim square"
        );
        assert_eq!(victim.kind_letter(), 'S');
        assert_eq!(victim.side(), Side::Second);
    }

    #[test]
    fn xiongqi_en_passant_matches_the_engine_mirrored_second_side() {
        // Mirror of the above: Second's Soldier captures First's `-S`, so
        // `forward()` flips sign too (Second's forward is -1, not +1) --
        // the direction most likely to expose a sign error in the rank
        // arithmetic. Generals are kept off the shared file/rank (a8 vs h1)
        // to avoid an incidental mutual flying-general self-check.
        let position = pos("g^7/8/8/8/5-S2/6s1/8/7G^ / c/C");
        let m = mv(r#"["g3","f3",null]"#);
        assert!(engine::legal_moves(&position).contains(&m));

        let next = engine::apply(&position, &m).expect("legal en passant");
        assert!(
            next.piece_at(sq("f4")).is_none(),
            "the engine must remove the victim from f4: {}",
            next.to_feen()
        );

        let (square, victim) = capture_victim(&position, &m).expect("must classify as a capture");
        assert_eq!(square, sq("f4"));
        assert_eq!(victim.kind_letter(), 'S');
        assert_eq!(victim.side(), Side::First);
    }

    #[test]
    fn chess_en_passant_still_matches_the_engine() {
        // Adjacent coverage: the function's chess branch, end-to-end
        // against the engine, alongside the xiongqi branch above.
        let position = pos("7k^/8/8/3-pP3/8/8/8/7K^ / W/w");
        let m = mv(r#"["e5","d6",null]"#);
        assert!(engine::legal_moves(&position).contains(&m));

        let next = engine::apply(&position, &m).expect("legal en passant");
        assert!(next.piece_at(sq("d5")).is_none());

        let (square, victim) = capture_victim(&position, &m).expect("must classify as a capture");
        assert_eq!(square, sq("d5"));
        assert_eq!(victim.side(), Side::Second);
    }

    // --- is_promotion -----------------------------------------------------

    #[test]
    fn xiongqi_promotion_is_actor_based_like_chess() {
        // Confirmed against the real engine rather than assumed: xiongqi
        // really does offer a choice of four promotion targets, exactly
        // like chess, each expanded by `engine::legal_moves` into its own
        // move carrying an actor.
        let position = pos("7g^/3S4/8/8/8/8/8/G^7 / C/c");
        let moves = engine::legal_moves(&position);
        let promotions: Vec<&Move> = moves
            .iter()
            .filter(|m| matches!(m, Move::Board { to, actor: Some(_), .. } if *to == sq("d8")))
            .collect();
        assert_eq!(
            promotions.len(),
            4,
            "chariot/knight/bear/empress: {moves:?}"
        );
        for m in promotions {
            assert!(is_promotion(&position, m), "{m:?} must be a promotion");
        }
        // A non-promoting sibling in the same legal-move list must not.
        let quiet = mv(r#"["a1","a2",null]"#);
        assert!(moves.contains(&quiet));
        assert!(!is_promotion(&position, &quiet));
    }

    #[test]
    fn ogi_promotion_is_automatic_and_actor_free() {
        let position = pos("7k^/3F4/8/8/8/8/8/4K^3 / J/j");
        let promo = mv(r#"["d7","d8",null]"#);
        assert!(engine::legal_moves(&position).contains(&promo));
        assert!(is_promotion(&position, &promo));
    }

    // --- resets_halfmove_clock ---------------------------------------------

    #[test]
    fn resets_halfmove_clock_matches_engine_cases() {
        // Capture.
        let capturing = pos("k^7/8/8/3q4/8/8/8/3R3K^ / W/w");
        let capture_mv = mv(r#"["d1","d5",null]"#);
        assert!(engine::legal_moves(&capturing).contains(&capture_mv));
        assert!(resets_halfmove_clock(&capturing, &capture_mv));

        // Foot-soldier quiet push.
        let pushing = pos("7k^/8/8/8/8/8/4P3/4K^3 / W/w");
        let push_mv = mv(r#"["e2","e3",null]"#);
        assert!(engine::legal_moves(&pushing).contains(&push_mv));
        assert!(resets_halfmove_clock(&pushing, &push_mv));

        // Non-foot-soldier quiet move.
        let quiet = pos("7k^/8/8/8/8/8/8/R3K^3 / W/w");
        let quiet_mv = mv(r#"["a1","a4",null]"#);
        assert!(engine::legal_moves(&quiet).contains(&quiet_mv));
        assert!(!resets_halfmove_clock(&quiet, &quiet_mv));

        // Drop (ōgi): never resets.
        let dropping = pos("7k^/8/8/8/8/8/8/4K^3 F/ J/j");
        let drop_mv = mv(r#"[null,"d4","fu"]"#);
        assert!(engine::legal_moves(&dropping).contains(&drop_mv));
        assert!(!resets_halfmove_clock(&dropping, &drop_mv));
    }
}
