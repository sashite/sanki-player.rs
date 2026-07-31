//! Bounded transposition table, keyed by the engine's canonical FEEN
//! (ADR-0015: zero new invariants — the engine's own canonicalization is
//! the key; a Zobrist scheme is future work with its own ADR).
//!
//! Mate scores are stored and probed **ply-adjusted** (distance from the
//! root converted to distance from the node), so the PV reflects true
//! distance-to-mate through the table. Scores derived from path-dependent
//! draws (repetition, half-move limit) are **never stored** — the same
//! position reached by another path is not drawn (normative, ADR-0015 §4).

use std::collections::BTreeMap;

use sashite_sanki_engine::domain::half_move::Move;

use crate::search::MATE;

/// The bound kind a stored score carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// The score is exact (a PV node).
    Exact,
    /// The score is a lower bound (a fail-high node).
    Lower,
    /// The score is an upper bound (a fail-low node).
    Upper,
}

/// A table entry.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Search depth the score was established at.
    pub depth: u8,
    /// Ply-adjusted score (see module doc).
    pub score: i32,
    /// The score's bound kind.
    pub bound: Bound,
    /// The best move found at this node, if any (ordering hint).
    pub best: Option<Move>,
}

/// A capacity-bounded map. When full, absent keys are simply not inserted
/// (no eviction in v1); present keys always update. A `BTreeMap` keeps the
/// crate free of ambient hasher randomness.
#[derive(Debug)]
pub struct Table {
    entries: BTreeMap<String, Entry>,
    capacity: usize,
}

/// The threshold above which a score is "a mate score" (ply-adjustable).
///
/// `pub(crate)` so [`crate::search`] can keep every non-mate score (draws,
/// the quiescence stand-pat) strictly outside this window, regardless of how
/// extreme a caller's `EvalWeights` is (see `search::NON_MATE_BOUND`) — the
/// table's magnitude-based mate/non-mate distinction only holds if nothing
/// else ever produces a score in this range.
pub(crate) const MATE_WINDOW: i32 = 1_000;

impl Table {
    /// An empty table with room for `capacity` entries (0 disables storage).
    #[must_use]
    pub const fn new(capacity: usize) -> Self {
        Self {
            entries: BTreeMap::new(),
            capacity,
        }
    }

    /// Probe `key`, returning the raw entry (mate scores still node-relative).
    #[must_use]
    pub fn probe(&self, key: &str) -> Option<&Entry> {
        self.entries.get(key)
    }

    /// Store an entry under `key`, converting a root-relative mate score to a
    /// node-relative one. `tainted` marks a path-dependent-draw score, which
    /// is never stored. Replaces an existing entry only at greater or equal
    /// depth.
    pub fn store(&mut self, key: &str, ply: u16, mut entry: Entry, tainted: bool) {
        if tainted || self.capacity == 0 {
            return;
        }
        if let Some(held) = self.entries.get(key) {
            if held.depth > entry.depth {
                return;
            }
        } else if self.entries.len() >= self.capacity {
            return;
        }
        entry.score = to_stored(entry.score, ply);
        self.entries.insert(key.to_owned(), entry);
    }

    /// A probed score converted back to root-relative form at `ply`.
    #[must_use]
    pub fn probe_score(entry: &Entry, ply: u16) -> i32 {
        from_stored(entry.score, ply)
    }
}

/// Root-relative → node-relative (store-side mate adjustment).
fn to_stored(score: i32, ply: u16) -> i32 {
    let ply = i32::from(ply);
    if score >= MATE.saturating_sub(MATE_WINDOW) {
        score.saturating_add(ply)
    } else if score <= MATE.saturating_sub(MATE_WINDOW).saturating_neg() {
        score.saturating_sub(ply)
    } else {
        score
    }
}

/// Node-relative → root-relative (probe-side mate adjustment).
fn from_stored(score: i32, ply: u16) -> i32 {
    let ply = i32::from(ply);
    if score >= MATE.saturating_sub(MATE_WINDOW) {
        score.saturating_sub(ply)
    } else if score <= MATE.saturating_sub(MATE_WINDOW).saturating_neg() {
        score.saturating_add(ply)
    } else {
        score
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    fn entry(depth: u8, score: i32) -> Entry {
        Entry {
            depth,
            score,
            bound: Bound::Exact,
            best: None,
        }
    }

    #[test]
    fn capacity_bounds_insertions_but_updates_pass() {
        let mut table = Table::new(1);
        table.store("a", 0, entry(1, 10), false);
        table.store("b", 0, entry(1, 20), false); // full: not inserted
        assert!(table.probe("a").is_some());
        assert!(table.probe("b").is_none());
        table.store("a", 0, entry(2, 30), false); // update passes
        assert_eq!(table.probe("a").unwrap().score, 30);
        // A shallower result never overwrites a deeper one.
        table.store("a", 0, entry(1, 40), false);
        assert_eq!(table.probe("a").unwrap().score, 30);
    }

    #[test]
    fn store_replaces_a_same_depth_exact_entry_with_a_weaker_bound() {
        // Pins the table's actual replacement policy (reliability review,
        // task 2): a same-or-greater-depth entry always replaces whatever is
        // held, with **no** preference for keeping a stronger `Exact` bound
        // over a `Lower`/`Upper` one at equal depth. This is deliberate, not
        // an oversight -- see `Table::store`'s doc and `search::negamax`'s
        // probe block (`match entry.bound { .. }`), which treats *every*
        // bound variant soundly regardless of provenance: `Exact` short-
        // circuits, `Lower`/`Upper` only ever narrow the window for a real,
        // independently-correct re-search. Losing the `Exact` tag here can
        // only cost a future re-search (search-quality), never produce a
        // wrong score (correctness) -- this test exists to make that
        // replacement concrete, not to demand it change.
        let mut table = Table::new(8);
        table.store(
            "a",
            0,
            Entry {
                depth: 5,
                score: 42,
                bound: Bound::Exact,
                best: None,
            },
            false,
        );
        assert_eq!(table.probe("a").unwrap().bound, Bound::Exact);

        table.store(
            "a",
            0,
            Entry {
                depth: 5,
                score: 7,
                bound: Bound::Lower,
                best: None,
            },
            false,
        );
        let held = table.probe("a").unwrap();
        assert_eq!(
            held.bound,
            Bound::Lower,
            "equal depth replaces even a stronger Exact bound with a weaker one"
        );
        assert_eq!(held.score, 7);
    }

    #[test]
    fn tainted_scores_are_never_stored() {
        let mut table = Table::new(8);
        table.store("a", 0, entry(3, 0), true);
        assert!(table.probe("a").is_none());
    }

    #[test]
    fn mate_scores_round_trip_through_ply_adjustment() {
        let mut table = Table::new(8);
        // A mate found 3 plies from the root, stored at ply 3.
        let root_relative = MATE.saturating_sub(3);
        table.store("a", 3, entry(5, root_relative), false);
        // Probed from a different ply, the distance re-anchors.
        let held = table.probe("a").unwrap();
        assert_eq!(Table::probe_score(held, 1), MATE.saturating_sub(1));
        assert_eq!(Table::probe_score(held, 3), root_relative);
    }

    // --- extended coverage (reliability review, 2026-07-31) ---------------
    //
    // The original test above only ever exercises one winning, positive mate
    // score stored and probed a couple of plies apart. The scenarios below
    // add: the losing (negative) side of the same mirror-symmetric formula,
    // a store at ply 0 re-probed from a much larger ply (the shape a real
    // transposition takes -- the same node reached shallow in one branch and
    // deep in another), and the exact `MATE_WINDOW` boundary on both sides
    // (`search::NON_MATE_BOUND` is defined as one *below* this threshold
    // specifically so a clamped eval score can never cross into ply-adjusted
    // territory -- these tests pin that the threshold itself behaves as that
    // reasoning assumes). A real end-to-end `choose()` run reproducing this
    // same arithmetic (not just the isolated formula) is covered separately
    // -- against an independently oracle-verified mate-in-2 fixture -- by
    // `tests/root_tiebreak.rs`'s:
    //   mate_in_two_scores_exactly_mate_minus_three_plies_through_a_real_tt

    #[test]
    fn losing_mate_scores_round_trip_through_ply_adjustment() {
        let mut table = Table::new(8);
        // Mirror of the winning case: *we* are mated 3 plies from the root.
        let root_relative = MATE.saturating_sub(3).saturating_neg();
        table.store("a", 3, entry(5, root_relative), false);
        let held = table.probe("a").unwrap();
        assert_eq!(
            Table::probe_score(held, 1),
            MATE.saturating_sub(1).saturating_neg()
        );
        assert_eq!(Table::probe_score(held, 3), root_relative);
    }

    #[test]
    fn mate_found_at_the_root_reprobes_correctly_from_a_much_deeper_ply() {
        // A mate in 1, found at the root itself (ply 0) -- the shallowest
        // possible store. Node-relative and root-relative coincide here
        // (`to_stored` adds `ply == 0`, a no-op), so this alone would not
        // catch an adjustment bug; the deep re-probe below is the real check.
        let mut table = Table::new(8);
        let root_relative = MATE.saturating_sub(1);
        table.store("a", 0, entry(7, root_relative), false);
        let held = table.probe("a").unwrap();
        assert_eq!(Table::probe_score(held, 0), root_relative);

        // The same stored node, now imagined reached transposed 50 plies
        // deep in a different branch: the node-relative "mate in 1" is
        // unchanged, but re-anchored to that deeper root it is 51 plies away.
        assert_eq!(Table::probe_score(held, 50), MATE.saturating_sub(51));
    }

    #[test]
    fn mate_window_boundary_is_inclusive_at_the_threshold_and_excludes_one_below() {
        // `MATE_WINDOW` draws the line at `MATE - MATE_WINDOW == 29_000`:
        // `to_stored`/`from_stored` treat a score as mate-distance-dependent
        // via `>=`/`<=`, so 29_000 itself must be ply-adjusted and 28_999
        // (== `search::NON_MATE_BOUND`, the exact value an extreme
        // `EvalWeights` clamps to) must sail through completely untouched --
        // otherwise a legitimately huge (but non-mate) evaluation could get
        // spuriously shifted as though it carried mate-distance information.
        let mut table = Table::new(8);

        table.store("edge_mate", 5, entry(3, 29_000), false);
        let edge_mate = table.probe("edge_mate").unwrap();
        assert_eq!(
            edge_mate.score, 29_005,
            "29_000 must be ply-adjusted on store"
        );
        assert_eq!(
            Table::probe_score(edge_mate, 5),
            29_000,
            "round trip at the same ply"
        );
        assert_eq!(
            Table::probe_score(edge_mate, 0),
            29_005,
            "re-anchored to a shallower ply"
        );

        table.store("edge_non_mate", 5, entry(3, 28_999), false);
        let edge_non_mate = table.probe("edge_non_mate").unwrap();
        assert_eq!(
            edge_non_mate.score, 28_999,
            "28_999 must NOT be ply-adjusted on store"
        );
        assert_eq!(Table::probe_score(edge_non_mate, 0), 28_999);
        assert_eq!(
            Table::probe_score(edge_non_mate, 200),
            28_999,
            "a non-mate score must be stable under probing from any ply"
        );

        // Mirror on the losing side: -29_000 is adjusted, -28_999 is not.
        table.store("edge_mate_neg", 5, entry(3, -29_000), false);
        let edge_mate_neg = table.probe("edge_mate_neg").unwrap();
        assert_eq!(
            edge_mate_neg.score, -29_005,
            "-29_000 must be ply-adjusted on store"
        );
        assert_eq!(Table::probe_score(edge_mate_neg, 5), -29_000);
        assert_eq!(Table::probe_score(edge_mate_neg, 0), -29_005);

        table.store("edge_non_mate_neg", 5, entry(3, -28_999), false);
        let edge_non_mate_neg = table.probe("edge_non_mate_neg").unwrap();
        assert_eq!(
            edge_non_mate_neg.score, -28_999,
            "-28_999 must NOT be ply-adjusted on store"
        );
        assert_eq!(Table::probe_score(edge_non_mate_neg, 0), -28_999);
        assert_eq!(Table::probe_score(edge_non_mate_neg, 200), -28_999);
    }
}
