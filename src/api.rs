//! The crate's public types — the ADR-0015 §2 API surface.
//!
//! Everything the caller supplies is grouped in three values: the game
//! [`Context`] (position + the history the rules make relevant), the persona
//! [`Strength`] (search effort and style), and the caller-owned [`Limits`]
//! (interruption). The result is a [`Choice`]. The crate never reads a clock:
//! all timing lives behind the `should_stop` hook.

use std::collections::BTreeMap;

use sashite_sanki_engine::position::Position;

/// Canonical-FEEN occurrence counts for every position reached so far in the
/// game — the same bookkeeping as the engine's `kernel::SessionState`
/// (`to_feen` → count, the initial position included). A [`BTreeMap`] so
/// lookup and iteration are deterministic.
pub type Occurrences = BTreeMap<String, u32>;

/// Everything the caller knows about the game that the rules make relevant.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The current canonical position (side to move encoded within).
    pub position: &'a Position,
    /// Canonical-FEEN occurrence counts for every position reached so far.
    pub occurrences: &'a Occurrences,
    /// Plies since the last capture or unpromoted foot-soldier move.
    pub halfmove_clock: u32,
}

/// The seven evaluation-term scales (ADR-0015 §5) — style profile material.
///
/// Each weight is a percentage: `100` applies the term's built-in table
/// as-is, `0` disables it, `150` overweights it half again. Personas
/// differentiate style by re-weighting, never by injected blunders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvalWeights {
    /// Board material (per-variant value tables).
    pub material: i32,
    /// Ōgi hand material (droppable potential; inert trays count zero).
    pub hand: i32,
    /// Advancement / placement (per-variant piece-square terms).
    pub psq: i32,
    /// Royal safety (shelter, in-check nudge).
    pub royal_safety: i32,
    /// Foot-soldier structure (doubled penalty, connected bonus).
    pub structure: i32,
    /// The mover's legal-move count (paid for by the search anyway).
    pub mobility: i32,
    /// Draw-leaf score in centipoints, from the root side's viewpoint:
    /// positive avoids draws the persona could press, negative steers toward
    /// them (ADR-0015 §4). Unlike the six others this is a direct score, not
    /// a percentage. Internally clamped well short of [`Choice::eval_cp`]'s
    /// mate range, so no magnitude ever lets a draw outrank, or be confused
    /// with, a genuine checkmate.
    pub contempt: i32,
}

impl Default for EvalWeights {
    fn default() -> Self {
        Self {
            material: 100,
            hand: 100,
            psq: 100,
            royal_safety: 100,
            structure: 100,
            mobility: 100,
            contempt: 0,
        }
    }
}

/// Search-effort and style parameters — the persona's "strength".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Strength {
    /// Iterative-deepening ceiling (≥ 1; `0` is treated as `1`).
    pub max_depth: u8,
    /// Transposition-table capacity in entries; `0` disables the table.
    pub tt_capacity: usize,
    /// Style profile (ADR-0015 §5).
    pub weights: EvalWeights,
    /// Tie-breaking seed among equal-best root moves — the fleet's
    /// between-sessions variety mechanism (ADR-0015 §6).
    pub seed: u64,
}

impl Default for Strength {
    fn default() -> Self {
        Self {
            max_depth: 4,
            tt_capacity: 100_000,
            weights: EvalWeights::default(),
            seed: 0,
        }
    }
}

/// Caller-owned interruption. The crate never reads a clock.
///
/// The **depth-1 iteration is exempt** from both limits (normative,
/// ADR-0015 §2): it is cheap and bounded, and running it to completion
/// unconditionally is what guarantees a legal position always yields a move,
/// however late the stop fires.
#[derive(Default, Clone, Copy)]
pub struct Limits<'a> {
    /// Deterministic node budget (`None` = unbounded).
    pub max_nodes: Option<u64>,
    /// Wall-clock hook, polled at node granularity from depth 2 on. On stop,
    /// `choose` returns the best move of the last completed iteration.
    pub should_stop: Option<&'a dyn Fn() -> bool>,
}

impl core::fmt::Debug for Limits<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Limits")
            .field("max_nodes", &self.max_nodes)
            .field("should_stop", &self.should_stop.map(|_| "<fn>"))
            .finish()
    }
}

/// The outcome of a [`crate::choose`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// The selected move.
    pub mv: sashite_sanki_engine::domain::half_move::Move,
    /// Score from the mover's viewpoint, in centipoints. Mate scores are
    /// offset by distance (`MATE − plies`), so larger is strictly better.
    pub eval_cp: i32,
    /// The last fully completed iteration's depth.
    pub depth: u8,
    /// Principal variation, best-first (never empty; starts with `mv`).
    pub pv: Vec<sashite_sanki_engine::domain::half_move::Move>,
    /// Nodes visited across all iterations (quiescence included).
    pub nodes: u64,
}
