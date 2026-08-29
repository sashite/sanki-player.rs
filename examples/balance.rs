//! balance — self-play win rates for the nine pairings of Chess, Ōgi and
//! Xiongqi on the shared 8×8 board. The full study, its tables and its
//! honest reading live in `examples/README.md`; this file is the code that
//! produces them.
//!
//! Two studies per pairing (first seat × second seat): uniformly RANDOM play
//! (a structural control — under zero skill, no army should collapse), and
//! DEPTH-3 self-play with default evaluation weights after four random
//! opening half-moves. The players are deterministic, so the random openings
//! are what makes games distinct; every game's transcript is hashed and the
//! DISTINCT count per cell is reported — a collapsed count invalidates its
//! row.
//!
//! Games run through the engine kernel (`kernel::step`), so every
//! termination is the full rule system's: checkmate, stalemate, no-move,
//! dead position, threefold repetition, the 100-half-move limit, the
//! 600-half-move cap. The clock is neutralized (every ply attests at the
//! session anchor: elapsed 0), so Timeout cannot occur.
//!
//! Everything derives from MASTER_SEED: the study reproduces bit for bit,
//! whatever the thread interleaving — verified identical across x86-64/Linux
//! and Apple Silicon/macOS (integer arithmetic throughout).
//!
//! Run: `cargo run --release --example balance`
//! (env: `N_RANDOM`, `N_D3` override the per-cell game counts, `0` skips a
//! study; `OUT_DIR_B9` sets the output directory, default `out/`).

// A measurement harness, not production code: the crate-level denies on
// integer arithmetic and indexing keep the PLAYER panic-free under
// adversarial input, while this example runs on its own trusted data — its
// counters cannot overflow within a study, and a panic here would fail a
// measurement, not a game. Below this header, the code is byte-identical to
// the bench run that produced the published tables.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]

use rayon::prelude::*;
use sashite_sanki_engine::domain::status::{Outcome3, Status};
use sashite_sanki_engine::domain::time::{Duration, Timestamp};
use sashite_sanki_engine::domain::time_control::{Period, TimeControl};
use sashite_sanki_engine::kernel::state::SessionState;
use sashite_sanki_engine::kernel::step::{step, StepResult};
use sashite_sanki_engine::prelude::*;
use sashite_sanki_player::{choose, Context, Limits, Occurrences, Strength};
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::fs;

/// Master seed — the whole study derives from it. Changing it is a NEW study.
const MASTER_SEED: u64 = 0x00B9_2026_0829;

/// Random opening half-moves before the depth-3 players take over.
const OPENING_PLIES: u32 = 4;

/// Per-cell game counts (env-overridable). 9 × (250 + 100) = 3 150 games.
const DEFAULT_N_RANDOM: usize = 250;
const DEFAULT_N_D3: usize = 100;

// ── deterministic PRNG (splitmix64) ─────────────────────────────────────────

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform index in `0..len` (len > 0). Modulo bias is < 2⁻⁵⁰ for any
    /// move-list length and is irrelevant to this study.
    fn index(&mut self, len: usize) -> usize {
        (self.next() % (len as u64)) as usize
    }
}

// ── the nine cells ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum V {
    Chess,
    Ogi,
    Xiongqi,
}

const VARIANTS: [V; 3] = [V::Chess, V::Ogi, V::Xiongqi];

impl V {
    const fn name(self) -> &'static str {
        match self {
            V::Chess => "chess",
            V::Ogi => "ogi",
            V::Xiongqi => "xiongqi",
        }
    }
    /// Back rank + pawn letter + SIN style letter, uppercase (first seat).
    const fn army(self) -> (&'static str, char, char) {
        match self {
            V::Chess => ("-RNBQK^BN-R", 'P', 'W'),
            V::Ogi => ("-RNBIK^BN-R", 'F', 'J'),
            V::Xiongqi => ("-RNBEG^BN-R", 'S', 'C'),
        }
    }
}

/// The initial FEEN of a pairing — the two shells' halves, exactly as the
/// arbiter founds a session (verified against `game-page/shells.ts` and the
/// canonical session of game c8ed1742: `-rnbik^bn-r/+f…/8/8/8/8/+S…/-RNBEG^BN-R / C/j`).
fn initial_feen(first: V, second: V) -> String {
    let (fb, fp, fs) = first.army();
    let (sb, sp, ss) = second.army();
    let pawns = |c: char| format!("+{c}").repeat(8);
    format!(
        "{}/{}/8/8/8/8/{}/{} / {}/{}",
        sb.to_lowercase(),
        pawns(sp.to_ascii_lowercase()),
        pawns(fp),
        fb,
        fs,
        ss.to_ascii_lowercase(),
    )
}

// ── one game ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum Mode {
    Random,
    Depth3,
}

struct GameRecord {
    outcome: Outcome3,
    status: Status,
    plies: u32,
    transcript: u64,
}

/// FNV-1a over the sequence of canonical FEENs — the game's identity.
fn fnv(acc: u64, s: &str) -> u64 {
    let mut h = acc;
    for b in s.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn play(first: V, second: V, mode: Mode, seed: u64) -> GameRecord {
    let feen = initial_feen(first, second);
    let position = match Position::parse(&feen) {
        Ok(p) => p,
        Err(e) => unreachable!(
            "initial FEEN of {}-{} rejected: {e:?}",
            first.name(),
            second.name()
        ),
    };
    let anchor = Timestamp::from_unix(1_756_400_000);
    let period = match Period::new(Duration::from_secs(1_000_000), None, None) {
        Ok(p) => p,
        Err(e) => unreachable!("period: {e:?}"),
    };
    let mut state = SessionState::start(position, TimeControl::new(period, Vec::new()), anchor);

    let mut rng = Rng(seed);
    let mut occurrences: Occurrences = BTreeMap::new();
    occurrences.insert(feen.clone(), 1);
    let mut transcript = fnv(0xCBF2_9CE4_8422_2325, &feen);
    let mut plies: u32 = 0;

    loop {
        let moves = engine::legal_moves(state.position());
        // The kernel's verdict terminates every game one ply earlier; an empty
        // legal set here would mean a rules bug. Flag it as a drawn anomaly
        // rather than crash the bench.
        if moves.is_empty() {
            return GameRecord {
                outcome: Outcome3::Draw,
                status: Status::NoMove,
                plies,
                transcript: !transcript,
            };
        }

        let random_ply = matches!(mode, Mode::Random) || plies < OPENING_PLIES;
        let mv: Move = if random_ply {
            moves[rng.index(moves.len())].clone()
        } else {
            let ctx = Context {
                position: state.position(),
                occurrences: &occurrences,
                halfmove_clock: state.halfmove_clock(),
            };
            let strength = Strength {
                max_depth: 3,
                tt_capacity: 100_000,
                seed: rng.next(),
                ..Strength::default()
            };
            match choose(&ctx, &strength, &Limits::default()) {
                Some(c) => c.mv,
                None => moves[rng.index(moves.len())].clone(),
            }
        };

        match step(state, &mv, anchor) {
            StepResult::Illegal { state: s, reason } => {
                // choose() and legal_moves() both promise legality; reaching
                // this arm is a player/engine disagreement worth surfacing.
                eprintln!(
                    "ILLEGAL in {}-{} ply {plies}: {reason:?}",
                    first.name(),
                    second.name()
                );
                state = s;
            }
            StepResult::Advanced { outcome, next } => {
                plies += 1;
                transcript = fnv(transcript, &outcome.position);
                *occurrences.entry(outcome.position.clone()).or_insert(0) += 1;
                match outcome.verdict {
                    Verdict::Ongoing => match next {
                        Some(n) => state = n,
                        None => unreachable!("ongoing verdict without next state"),
                    },
                    Verdict::Terminated { status, result } => {
                        return GameRecord {
                            outcome: result,
                            status,
                            plies,
                            transcript,
                        };
                    }
                }
            }
        }
    }
}

// ── the studies ─────────────────────────────────────────────────────────────

struct CellReport {
    first: V,
    second: V,
    n: usize,
    distinct: usize,
    wins: usize,
    draws: usize,
    losses: usize,
    mean_plies: f64,
    statuses: BTreeMap<&'static str, usize>,
}

impl CellReport {
    /// First-seat score in percent (win = 1, draw = ½).
    fn score(&self) -> f64 {
        100.0 * (self.wins as f64 + 0.5 * self.draws as f64) / (self.n as f64)
    }
    /// Standard error of the score, in points (Bernoulli-style on the mean of
    /// per-game points ∈ {0, ½, 1}; conservative and honest at these n).
    fn se(&self) -> f64 {
        let n = self.n as f64;
        let mean = self.score() / 100.0;
        let var = (self.wins as f64 * (1.0 - mean).powi(2)
            + self.draws as f64 * (0.5 - mean).powi(2)
            + self.losses as f64 * (0.0 - mean).powi(2))
            / (n - 1.0);
        100.0 * (var / n).sqrt()
    }
}

fn status_key(s: Status) -> &'static str {
    match s {
        Status::Checkmate => "checkmate",
        Status::Stalemate => "stalemate",
        Status::NoMove => "nomove",
        Status::Insufficient => "insufficient",
        Status::Repetition => "repetition",
        Status::MoveLimit => "move_limit",
        Status::MoveCap => "move_cap",
        _ => "other",
    }
}

fn run_study(mode: Mode, n_per_cell: usize, study_tag: u64) -> Vec<CellReport> {
    let cells: Vec<(V, V)> = VARIANTS
        .iter()
        .flat_map(|f| VARIANTS.iter().map(move |s| (*f, *s)))
        .collect();

    cells
        .iter()
        .map(|(first, second)| {
            let cell_tag = ((first.army().2 as u64) << 8) | (second.army().2 as u64);
            let records: Vec<GameRecord> = (0..n_per_cell)
                .into_par_iter()
                .map(|g| {
                    let mut d = Rng(MASTER_SEED ^ (study_tag << 48) ^ (cell_tag << 32) ^ g as u64);
                    let seed = d.next();
                    play(*first, *second, mode, seed)
                })
                .collect();

            let mut wins = 0;
            let mut draws = 0;
            let mut losses = 0;
            let mut plies_sum: u64 = 0;
            let mut distinct = HashSet::new();
            let mut statuses: BTreeMap<&'static str, usize> = BTreeMap::new();
            for r in &records {
                match r.outcome {
                    Outcome3::FirstWins => wins += 1,
                    Outcome3::Draw => draws += 1,
                    Outcome3::SecondWins => losses += 1,
                }
                plies_sum += u64::from(r.plies);
                distinct.insert(r.transcript);
                *statuses.entry(status_key(r.status)).or_insert(0) += 1;
            }
            let report = CellReport {
                first: *first,
                second: *second,
                n: records.len(),
                distinct: distinct.len(),
                wins,
                draws,
                losses,
                mean_plies: plies_sum as f64 / records.len() as f64,
                statuses,
            };
            // Per-cell checkpoint: a 3-hour run must not lose everything to a
            // container reclaim — each finished cell lands on disk at once.
            let out_dir = std::env::var("OUT_DIR_B9").unwrap_or_else(|_| "out".to_string());
            let line = format!(
                "study={study_tag} {}-{} n={} distinct={} score={:.1} se={:.1} wdl={}-{}-{} plies={:.0} {:?}\n",
                report.first.name(),
                report.second.name(),
                report.n,
                report.distinct,
                report.score(),
                report.se(),
                report.wins,
                report.draws,
                report.losses,
                report.mean_plies,
                report.statuses
            );
            eprintln!("  cell done: {}", line.trim_end());
            use std::io::Write as _;
            if let Ok(mut f) = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(format!("{out_dir}/partial.log"))
            {
                let _ = f.write_all(line.as_bytes());
            }
            report
        })
        .collect()
}

fn table(title: &str, reports: &[CellReport]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "## {title}\n");
    let _ = writeln!(
        out,
        "| First seat | Second seat | Games | Distinct | First score | ± SE | W–D–L | Mean plies | Terminations |"
    );
    let _ = writeln!(
        out,
        "| --- | --- | ---: | ---: | ---: | ---: | :--- | ---: | :--- |"
    );
    for r in reports {
        let statuses = r
            .statuses
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {:.1} % | {:.1} | {}–{}–{} | {:.0} | {} |",
            r.first.name(),
            r.second.name(),
            r.n,
            r.distinct,
            r.score(),
            r.se(),
            r.wins,
            r.draws,
            r.losses,
            r.mean_plies,
            statuses
        );
    }
    out
}

fn main() {
    let n_random = std::env::var("N_RANDOM")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_N_RANDOM);
    let n_d3 = std::env::var("N_D3")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_N_D3);
    let out_dir = std::env::var("OUT_DIR_B9").unwrap_or_else(|_| "out".to_string());
    let _ = fs::create_dir_all(&out_dir);

    // A study with a zero count is SKIPPED (its table is omitted): this is
    // what lets the two studies run in separate invocations — e.g.
    // `N_RANDOM=0 cargo run --release` replays only the depth-3 study.
    let random = if n_random > 0 {
        let t0 = std::time::Instant::now();
        eprintln!("random study: 9 × {n_random} games…");
        let r = run_study(Mode::Random, n_random, 1);
        eprintln!("  done in {:.1?}", t0.elapsed());
        r
    } else {
        Vec::new()
    };

    let d3 = if n_d3 > 0 {
        let t1 = std::time::Instant::now();
        eprintln!("depth-3 study: 9 × {n_d3} games…");
        let r = run_study(Mode::Depth3, n_d3, 2);
        eprintln!("  done in {:.1?}", t1.elapsed());
        r
    } else {
        Vec::new()
    };

    let mut md = String::new();
    let _ = writeln!(
        md,
        "# B9 — measured balance across the nine pairings\n\n\
         Master seed `{MASTER_SEED:#X}`; every figure below is reproducible\n\
         bit for bit by `cargo run --release` in this bench. Engine\n\
         `sashite-sanki-engine 0.9.0`, player `sashite-sanki-player 0.5.0`\n\
         (crates.io). Terminations are the kernel's — the arbiter's own rule\n\
         system. Depth-3 games open with {OPENING_PLIES} random half-moves;\n\
         the **Distinct** column is the number of distinct games actually\n\
         played in the cell, and a collapsed count invalidates its row.\n"
    );
    if !random.is_empty() {
        md.push_str(&table("Uniformly random play", &random));
        md.push('\n');
    }
    if !d3.is_empty() {
        md.push_str(&table("Self-play, depth 3 (default weights)", &d3));
    }

    let path = format!("{out_dir}/b9-results.md");
    match fs::write(&path, &md) {
        Ok(()) => eprintln!("written: {path}"),
        Err(e) => eprintln!("cannot write {path}: {e}"),
    }
    println!("{md}");
}
