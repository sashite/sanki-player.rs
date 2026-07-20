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
const MATE_WINDOW: i32 = 1_000;

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
}
