//! The search — iterative-deepening, fail-soft alpha-beta (negamax), move
//! ordering, quiescence on captures/promotions, bounded transposition table
//! (ADR-0015 §3), history-aware draw scoring (§4), deterministic seeded
//! tie-breaking (§6).
//!
//! The engine's public API is the only board representation: children come
//! from `engine::apply`, legality from `engine::legal_moves`, terminal
//! classification from `engine::status`, and the canonical FEEN string —
//! computed once per node — is shared by the repetition overlay and the
//! table key. Zero rule duplication, zero divergence surface with the
//! arbiter.

use std::collections::BTreeMap;

use sashite_sanki_engine::domain::half_move::Move;
use sashite_sanki_engine::domain::outcome::Verdict;
use sashite_sanki_engine::domain::side::Side;
use sashite_sanki_engine::domain::square::Square;
use sashite_sanki_engine::domain::status::Status;
use sashite_sanki_engine::domain::variant::Variant;
use sashite_sanki_engine::engine;
use sashite_sanki_engine::position::Position;

use crate::api::{Choice, Context, EvalWeights, Limits, Occurrences, Strength};
use crate::classify::{capture_victim, is_promotion, resets_halfmove_clock};
use crate::eval::{capture_gain, evaluate};
use crate::prng::SplitMix64;
use crate::tt::{Bound, Entry, Table};
use crate::values::piece_board_value;

/// The mate score anchor: a mate found `d` plies from the root scores
/// `MATE − d` (or its negation for the mated side), so faster mates rank
/// higher and longer resistance ranks less badly.
pub const MATE: i32 = 30_000;

/// The threefold-repetition threshold, initial position included — the
/// kernel's own convention.
const REPETITION_THRESHOLD: u32 = 3;

/// The 100-half-move limit — the kernel's own convention.
const MOVE_LIMIT: u32 = 100;

/// A canonical, total ordering key over the `[source, destination, actor]`
/// triple (ADR-0015 §6): the generated move list is sorted by it before any
/// heuristic ordering, so determinism holds by construction, independent of
/// engine iteration order. Doubles as the history-heuristic key.
fn move_key(mv: &Move) -> String {
    match mv {
        Move::Board { from, to, actor } => {
            let actor = actor.as_ref().map_or("", |name| name.as_str());
            format!(
                "b:{:02}:{:02}:{actor}",
                square_index(*from),
                square_index(*to)
            )
        }
        Move::Drop { piece, to } => format!("d:{}:{:02}", piece.as_str(), square_index(*to)),
    }
}

fn square_index(square: Square) -> u8 {
    square
        .rank()
        .saturating_mul(Square::FILE_COUNT)
        .saturating_add(square.file())
}

struct Searcher<'a> {
    weights: &'a EvalWeights,
    occurrences: &'a Occurrences,
    root_side: Side,
    contempt: i32,
    /// Dead-position probing is enabled only when neither side plays ōgi
    /// (pure ōgi has no dead-position rule, and a hand refutes insufficiency).
    dead_gate: bool,
    tt: Table,
    killers: Vec<[Option<Move>; 2]>,
    history: BTreeMap<String, u32>,
    path: BTreeMap<String, u32>,
    nodes: u64,
    max_nodes: Option<u64>,
    should_stop: Option<&'a dyn Fn() -> bool>,
    /// False during the depth-1 iteration (normative exemption, ADR-0015 §2).
    limits_armed: bool,
    aborted: bool,
}

impl<'a> Searcher<'a> {
    fn new(ctx: &Context<'a>, strength: &'a Strength, limits: &Limits<'a>) -> Self {
        let variants = ctx.position.variants();
        let dead_gate = variants.first != Variant::Ogi && variants.second != Variant::Ogi;
        Self {
            weights: &strength.weights,
            occurrences: ctx.occurrences,
            root_side: ctx.position.active_side(),
            contempt: strength.weights.contempt,
            dead_gate,
            tt: Table::new(strength.tt_capacity),
            killers: vec![[None, None]; usize::from(strength.max_depth).saturating_add(2)],
            history: BTreeMap::new(),
            path: BTreeMap::new(),
            nodes: 0,
            max_nodes: limits.max_nodes,
            should_stop: limits.should_stop,
            limits_armed: false,
            aborted: false,
        }
    }

    /// Count a node and poll the caller's limits (armed from depth 2 on).
    fn tick(&mut self) {
        self.nodes = self.nodes.saturating_add(1);
        if !self.limits_armed || self.aborted {
            return;
        }
        if self.max_nodes.is_some_and(|budget| self.nodes > budget) {
            self.aborted = true;
            return;
        }
        if self.should_stop.is_some_and(|stop| stop()) {
            self.aborted = true;
        }
    }

    /// The draw score from the node mover's viewpoint, anchored at the root
    /// side (ADR-0015 §4): `−contempt` for the root side, `+contempt` for
    /// its opponent — the same leaf must not look draw-attractive to both.
    fn draw_score(&self, node_mover: Side) -> i32 {
        if node_mover == self.root_side {
            self.contempt.saturating_neg()
        } else {
            self.contempt
        }
    }

    /// The total occurrence count of `feen`: game history plus search path.
    fn occurrence_count(&self, feen: &str) -> u32 {
        let game = self.occurrences.get(feen).copied().unwrap_or(0);
        let path = self.path.get(feen).copied().unwrap_or(0);
        game.saturating_add(path)
    }

    /// The intrinsic terminal score of a position **whose move list is
    /// empty**, from its mover's viewpoint: checkmate is `−(MATE − ply)`,
    /// every drawn status the contempt-anchored draw.
    fn terminal_score(&self, position: &Position, ply: u16) -> i32 {
        match engine::status(position) {
            Verdict::Terminated {
                status: Status::Checkmate,
                ..
            } => MATE.saturating_sub(i32::from(ply)).saturating_neg(),
            _ => self.draw_score(position.active_side()),
        }
    }

    /// Dead-position probe (ADR-0015 §3): a draw the arbiter calls while
    /// moves remain, so it is tested at **every** node — behind a cheap
    /// material gate (an over-approximation of every per-variant dead
    /// configuration; never fires in ōgi).
    fn dead_draw(&self, position: &Position, non_royal_count: u32) -> bool {
        if !self.dead_gate || non_royal_count > 2 {
            return false;
        }
        matches!(
            engine::status(position),
            Verdict::Terminated {
                status: Status::Insufficient,
                ..
            }
        )
    }

    /// Non-royal piece count (board only — the gate is disabled whenever a
    /// hand could exist).
    fn non_royal_count(position: &Position) -> u32 {
        let mut count = 0_u32;
        for square in Square::all() {
            if position.piece_at(square).is_some_and(|p| !p.is_royal()) {
                count = count.saturating_add(1);
            }
        }
        count
    }

    /// Heuristic ordering score for `mv` at `ply` (higher searches first).
    fn order_score(&self, position: &Position, mv: &Move, tt_move: Option<&Move>, ply: u16) -> i64 {
        const ORDER_TT: i64 = 1 << 40;
        const ORDER_CAPTURE: i64 = 1 << 30;
        const ORDER_PROMOTION: i64 = 1 << 29;
        const ORDER_KILLER_PRIMARY: i64 = (1 << 28) + 1;
        const ORDER_KILLER_SECONDARY: i64 = 1 << 28;
        const ORDER_QUIET_DROP_MALUS: i64 = 1 << 20;
        if tt_move == Some(mv) {
            return ORDER_TT;
        }
        if let Some((_, victim)) = capture_victim(position, mv) {
            let mover_side = position.active_side();
            let gain = i64::from(capture_gain(position, mover_side, victim));
            let attacker = attacker_value(position, mv);
            return ORDER_CAPTURE
                .saturating_add(gain.saturating_mul(16))
                .saturating_sub(attacker.wrapping_div(8));
        }
        if is_promotion(position, mv) {
            return ORDER_PROMOTION;
        }
        if let Some(slots) = self.killers.get(usize::from(ply)) {
            if slots.first().is_some_and(|k| k.as_ref() == Some(mv)) {
                return ORDER_KILLER_PRIMARY;
            }
            if slots.get(1).is_some_and(|k| k.as_ref() == Some(mv)) {
                return ORDER_KILLER_SECONDARY;
            }
        }
        let history = i64::from(self.history.get(&move_key(mv)).copied().unwrap_or(0));
        // Quiet drops last among quiets (ADR-0015 §3).
        if matches!(mv, Move::Drop { .. }) {
            history.saturating_sub(ORDER_QUIET_DROP_MALUS)
        } else {
            history
        }
    }

    /// The generated move list, canonically sorted then heuristically
    /// ordered (stable, so equal heuristic scores keep canonical order).
    fn ordered_moves(&self, position: &Position, tt_move: Option<&Move>, ply: u16) -> Vec<Move> {
        let mut moves = engine::legal_moves(position);
        moves.sort_by_key(move_key);
        let mut scored: Vec<(i64, Move)> = moves
            .into_iter()
            .map(|mv| (self.order_score(position, &mv, tt_move, ply), mv))
            .collect();
        scored.sort_by_key(|entry| core::cmp::Reverse(entry.0));
        scored.into_iter().map(|(_, mv)| mv).collect()
    }

    /// Record a quiet move that produced a beta cutoff (killers + history).
    fn record_cutoff(&mut self, position: &Position, mv: &Move, ply: u16, depth: u8) {
        if capture_victim(position, mv).is_some() || is_promotion(position, mv) {
            return;
        }
        if let Some(slots) = self.killers.get_mut(usize::from(ply)) {
            if slots.first().map(Option::as_ref) != Some(Some(mv)) {
                *slots = [Some(mv.clone()), slots.first().cloned().flatten()];
            }
        }
        let bonus = u32::from(depth).saturating_mul(u32::from(depth));
        let entry = self.history.entry(move_key(mv)).or_insert(0);
        *entry = entry.saturating_add(bonus);
    }

    /// Quiescence search: captures and promotions only (a drop never
    /// captures, so hand material cannot explode the tree). Path overlay is
    /// unnecessary here — every searched move resets the half-move clock, so
    /// no repetition can occur along a quiescence line.
    fn quiescence(&mut self, position: &Position, ply: u16, mut alpha: i32, beta: i32) -> i32 {
        self.tick();
        if self.aborted {
            return 0;
        }
        if self.dead_draw(position, Self::non_royal_count(position)) {
            return self.draw_score(position.active_side());
        }
        let mut moves = engine::legal_moves(position);
        if moves.is_empty() {
            return self.terminal_score(position, ply);
        }
        let stand_pat = evaluate(position, self.weights, moves.len());
        if stand_pat >= beta {
            return stand_pat;
        }
        alpha = alpha.max(stand_pat);
        let mut best = stand_pat;

        moves.sort_by_key(move_key);
        let mut tactical: Vec<(i64, Move)> = moves
            .into_iter()
            .filter_map(|mv| {
                if let Some((_, victim)) = capture_victim(position, &mv) {
                    let gain = i64::from(capture_gain(position, position.active_side(), victim));
                    Some((gain.saturating_mul(16), mv))
                } else if is_promotion(position, &mv) {
                    Some((0, mv))
                } else {
                    None
                }
            })
            .collect();
        tactical.sort_by_key(|entry| core::cmp::Reverse(entry.0));

        for (_, mv) in tactical {
            let Ok(child) = engine::apply(position, &mv) else {
                continue;
            };
            let score = self
                .quiescence(
                    &child,
                    ply.saturating_add(1),
                    beta.saturating_neg(),
                    alpha.saturating_neg(),
                )
                .saturating_neg();
            if self.aborted {
                return 0;
            }
            best = best.max(score);
            alpha = alpha.max(score);
            if alpha >= beta {
                break;
            }
        }
        best
    }

    /// Fail-soft negamax. Returns `(score, tainted)` — `tainted` marks a
    /// score derived from a path-dependent draw, which must not enter the
    /// table (ADR-0015 §4).
    #[allow(clippy::too_many_arguments)]
    fn negamax(
        &mut self,
        position: &Position,
        feen: &str,
        depth: u8,
        ply: u16,
        mut alpha: i32,
        beta: i32,
        clock: u32,
    ) -> (i32, bool) {
        self.tick();
        if self.aborted {
            return (0, true);
        }

        // History draws first: repetition (game + path), then the move limit.
        if self.occurrence_count(feen) >= REPETITION_THRESHOLD {
            return (self.draw_score(position.active_side()), true);
        }
        if clock >= MOVE_LIMIT {
            return (self.draw_score(position.active_side()), true);
        }
        // Dead position: a draw called while moves remain — every node.
        if self.dead_draw(position, Self::non_royal_count(position)) {
            return (self.draw_score(position.active_side()), false);
        }

        if depth == 0 {
            return (self.quiescence(position, ply, alpha, beta), false);
        }

        // Transposition probe.
        let alpha_in = alpha;
        let mut beta_bound = beta;
        let mut tt_move: Option<Move> = None;
        if let Some(entry) = self.tt.probe(feen) {
            tt_move = entry.best.clone();
            if entry.depth >= depth {
                let score = Table::probe_score(entry, ply);
                match entry.bound {
                    Bound::Exact => return (score, false),
                    Bound::Lower => alpha = alpha.max(score),
                    Bound::Upper => beta_bound = beta_bound.min(score),
                }
                if alpha >= beta_bound {
                    return (score, false);
                }
            }
        }

        let moves = self.ordered_moves(position, tt_move.as_ref(), ply);
        if moves.is_empty() {
            return (self.terminal_score(position, ply), false);
        }

        let mut best = i32::MIN.saturating_add(1);
        let mut best_move: Option<Move> = None;
        let mut best_taint = false;

        for mv in moves {
            let Ok(child) = engine::apply(position, &mv) else {
                continue; // unreachable on a legal move; defensive skip
            };
            let child_feen = child.to_feen();
            let child_clock = if resets_halfmove_clock(position, &mv) {
                0
            } else {
                clock.saturating_add(1)
            };
            increment(&mut self.path, &child_feen);
            let (child_score, child_taint) = self.negamax(
                &child,
                &child_feen,
                depth.saturating_sub(1),
                ply.saturating_add(1),
                beta_bound.saturating_neg(),
                alpha.saturating_neg(),
                child_clock,
            );
            decrement(&mut self.path, &child_feen);
            if self.aborted {
                return (0, true);
            }
            let score = child_score.saturating_neg();
            if score > best {
                best = score;
                best_move = Some(mv.clone());
                best_taint = child_taint;
            }
            alpha = alpha.max(score);
            if alpha >= beta_bound {
                self.record_cutoff(position, &mv, ply, depth);
                break;
            }
        }

        let bound = if best <= alpha_in {
            Bound::Upper
        } else if best >= beta_bound {
            Bound::Lower
        } else {
            Bound::Exact
        };
        self.tt.store(
            feen,
            ply,
            Entry {
                depth,
                score: best,
                bound,
                best: best_move,
            },
            best_taint,
        );
        (best, best_taint)
    }
}

fn increment(path: &mut BTreeMap<String, u32>, key: &str) {
    let count = path.entry(key.to_owned()).or_insert(0);
    *count = count.saturating_add(1);
}

fn decrement(path: &mut BTreeMap<String, u32>, key: &str) {
    if let Some(count) = path.get_mut(key) {
        if *count <= 1 {
            path.remove(key);
        } else {
            *count = count.saturating_sub(1);
        }
    }
}

fn attacker_value(position: &Position, mv: &Move) -> i64 {
    let Move::Board { from, .. } = mv else {
        return 0;
    };
    position.piece_at(*from).map_or(0, |piece| {
        i64::from(piece_board_value(piece, position.variant_of(piece.side())))
    })
}

/// A root move's standing after an iteration.
#[derive(Debug, Clone)]
struct RootMove {
    mv: Move,
    score: i32,
}

/// The best move, or `None` iff the position has no legal move
/// (ADR-0015 §2). See [`crate::choose`] for the full contract.
#[must_use]
pub fn choose(ctx: &Context<'_>, strength: &Strength, limits: &Limits<'_>) -> Option<Choice> {
    let max_depth = strength.max_depth.max(1);
    let mut searcher = Searcher::new(ctx, strength, limits);

    let mut root_moves: Vec<RootMove> = {
        let mut moves = engine::legal_moves(ctx.position);
        moves.sort_by_key(move_key);
        moves
            .into_iter()
            .map(|mv| RootMove { mv, score: 0 })
            .collect()
    };
    if root_moves.is_empty() {
        return None;
    }

    let mut completed_depth = 0_u8;

    for depth in 1..=max_depth {
        // The depth-1 iteration is exempt from every limit (normative):
        // running it to completion is what guarantees a move.
        searcher.limits_armed = depth > 1;

        let mut iteration: Vec<RootMove> = Vec::with_capacity(root_moves.len());
        let mut alpha = i32::MIN.saturating_add(1);
        let mut aborted = false;

        for root in &root_moves {
            let Ok(child) = engine::apply(ctx.position, &root.mv) else {
                continue;
            };
            let child_feen = child.to_feen();
            let child_clock = if resets_halfmove_clock(ctx.position, &root.mv) {
                0
            } else {
                ctx.halfmove_clock.saturating_add(1)
            };
            increment(&mut searcher.path, &child_feen);
            let (child_score, _) = searcher.negamax(
                &child,
                &child_feen,
                depth.saturating_sub(1),
                1,
                i32::MIN.saturating_add(1),
                alpha.saturating_neg(),
                child_clock,
            );
            decrement(&mut searcher.path, &child_feen);
            if searcher.aborted {
                aborted = true;
                break;
            }
            let score = child_score.saturating_neg();
            alpha = alpha.max(score);
            iteration.push(RootMove {
                mv: root.mv.clone(),
                score,
            });
        }

        if aborted {
            break;
        }
        // Order the next iteration best-first (stable: canonical order among
        // equals is preserved from the initial sort).
        iteration.sort_by_key(|entry| core::cmp::Reverse(entry.score));
        root_moves = iteration;
        completed_depth = depth;
    }

    // Seeded tie-break among equal-best root moves, in canonical order so the
    // pick is independent of iteration ordering (ADR-0015 §6).
    let best_score = root_moves.iter().map(|r| r.score).max()?;
    let mut equal_best: Vec<&RootMove> = root_moves
        .iter()
        .filter(|r| r.score == best_score)
        .collect();
    equal_best.sort_by_key(|r| move_key(&r.mv));
    let mut rng = SplitMix64::new(strength.seed);
    let chosen = equal_best.get(rng.next_index(equal_best.len()))?;

    let pv = principal_variation(ctx.position, &chosen.mv, &searcher.tt, completed_depth);
    Some(Choice {
        mv: chosen.mv.clone(),
        eval_cp: best_score,
        depth: completed_depth,
        pv,
        nodes: searcher.nodes,
    })
}

/// Bounded assessment of the position (ADR-0015 §2): the root score of the
/// same search, without the move obligation. A position with no legal move
/// scores its terminal value (`−MATE` for a mated mover, the contempt-
/// anchored draw otherwise).
#[must_use]
pub fn assess(ctx: &Context<'_>, strength: &Strength, limits: &Limits<'_>) -> i32 {
    match choose(ctx, strength, limits) {
        Some(choice) => choice.eval_cp,
        None => {
            let searcher = Searcher::new(ctx, strength, limits);
            searcher.terminal_score(ctx.position, 0)
        }
    }
}

/// Walk the table's best moves from the chosen root move, bounded by the
/// completed depth (and by legality — every hop is re-applied).
fn principal_variation(position: &Position, first: &Move, tt: &Table, depth: u8) -> Vec<Move> {
    let mut pv = vec![first.clone()];
    let Ok(mut current) = engine::apply(position, first) else {
        return pv;
    };
    for _ in 1..depth {
        let feen = current.to_feen();
        let Some(next) = tt.probe(&feen).and_then(|entry| entry.best.clone()) else {
            break;
        };
        let Ok(applied) = engine::apply(&current, &next) else {
            break;
        };
        pv.push(next);
        current = applied;
    }
    pv
}
