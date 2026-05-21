//! Iterative-deepening alpha-beta minimax search.
//!
//! X-positive eval. Per-stone granularity. PVS + aspiration windows +
//! LMR + threat-only quiescence + check extensions. Minimax form (not
//! negamax) — a single recursive `pvs_node` dispatches on
//! `board.to_move()` so X maximizes and O minimizes without sign flips.

#![allow(clippy::must_use_candidate)]
// Search arithmetic stays inside `[-INF, INF]` with `INF = i32::MAX / 2`,
// so `as` casts on `u8` / `u32` ply and node counts are bounded by
// construction. Pedantic clippy can't see those invariants.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use std::time::{Duration, Instant};

use crate::board::{Board, Player};
use crate::config::{
    ASPIRATION_START_DEPTH, ASP_WINDOW_INITIAL, ASP_WINDOW_WIDEN_FACTOR, DEADLINE_CHECK_NODES,
    DEFAULT_MAX_DEPTH,
    DEFAULT_MOVE_RADIUS, DEFAULT_TIME_MS, LMR_MIN_DEPTH, LMR_MIN_MOVE_INDEX,
    LMR_REDUCTION, MATE_SCORE, MAX_CHECK_EXTENSIONS, MAX_PLY, QSEARCH_MAX_PLIES, TIME_STONE1_PCT,
};
use crate::coords::{Coord, ORIGIN};
use crate::moves::{self, MOVE_GEN_CAP_INLINE};
use crate::ordering::{self, OrderingContext, OrderingState, order_moves_with_buckets};
use crate::tt::{TTFlag, TranspositionTable};
use smallvec::SmallVec;

/// Open window bound. Half of `i32::MAX` so `INF + x` never overflows
/// inside the search.
pub const INF: i32 = i32::MAX / 2;

/// Any |score| above this is treated as mate-class. The TT store/probe
/// path encodes mate distance relative to the current node so a
/// transposition at a different ply doesn't return an off-by-N mate.
const MATE_BOUND: i32 = MATE_SCORE - MAX_PLY as i32;

// ─────────────────────────────────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────────────────────────────────

/// Tunable search parameters. Defaults are sourced from `hexo.toml` via
/// `crate::config::*`.
#[derive(Copy, Clone, Debug)]
pub struct SearchConfig {
    /// Hard cap on iterative-deepening depth.
    pub max_depth: i8,
    /// Wall-clock budget for this `search_root` call, or `None` for
    /// depth-only termination.
    pub time_ms: Option<u64>,
    /// Nodes between deadline checks. Powers of two are cheapest for the
    /// modulo test.
    pub deadline_check_nodes: u32,
    /// Fraction of a per-turn budget allocated to stone 1. Stone 2 gets
    /// the remainder.
    pub stone1_time_pct: f32,
    /// Aspiration half-window for the first attempt.
    pub asp_window_initial: i32,
    /// Multiplicative widen factor between aspiration attempts.
    pub asp_window_widen_factor: u32,
    /// Minimum remaining depth for an LMR-eligible move.
    pub lmr_min_depth: i8,
    /// Minimum 0-indexed move number for an LMR-eligible move.
    pub lmr_min_move_index: u8,
    /// Plies to subtract on LMR reduction.
    pub lmr_reduction: i8,
    /// Hard cap on quiescence recursion depth (in plies inside qsearch).
    pub qsearch_max_plies: u8,
    /// Per-root-path budget for check extensions.
    pub max_check_extensions: u8,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_depth: i8::try_from(DEFAULT_MAX_DEPTH).expect("DEFAULT_MAX_DEPTH fits in i8"),
            time_ms: Some(DEFAULT_TIME_MS),
            deadline_check_nodes: DEADLINE_CHECK_NODES as u32,
            stone1_time_pct: TIME_STONE1_PCT,
            asp_window_initial: ASP_WINDOW_INITIAL,
            asp_window_widen_factor: ASP_WINDOW_WIDEN_FACTOR,
            lmr_min_depth: LMR_MIN_DEPTH,
            lmr_min_move_index: LMR_MIN_MOVE_INDEX,
            lmr_reduction: LMR_REDUCTION,
            qsearch_max_plies: QSEARCH_MAX_PLIES,
            max_check_extensions: MAX_CHECK_EXTENSIONS,
        }
    }
}

/// Result of one `search_root` call.
#[derive(Copy, Clone, Debug)]
pub struct SearchResult {
    /// Chosen move. Defaults to [`ORIGIN`] when no search completed.
    pub best_move: Coord,
    /// Minimax score from the X-positive perspective.
    pub score: i32,
    /// Depth of the deepest fully-completed iteration.
    pub depth_reached: i8,
    /// Total nodes visited (recursive + quiescence).
    pub nodes: u64,
    /// Wall-clock time consumed in milliseconds.
    pub time_ms: u64,
}

impl Default for SearchResult {
    fn default() -> Self {
        Self {
            best_move: ORIGIN,
            score: 0,
            depth_reached: 0,
            nodes: 0,
            time_ms: 0,
        }
    }
}

/// Reason an in-progress iteration aborted. Propagated up through
/// `Result`; the outer driver discards partial iterations on timeout.
#[derive(Copy, Clone, Debug)]
enum SearchError {
    Timeout,
}

type SearchScore = Result<i32, SearchError>;

// ─────────────────────────────────────────────────────────────────────────────
// Public driver
// ─────────────────────────────────────────────────────────────────────────────

/// Iterative-deepening search entry point. Soft-fails on `cfg.time_ms`
/// timeout, returning the last completed iteration's result.
///
/// Caller owns the [`Board`], [`TranspositionTable`], and
/// [`OrderingState`]. The function bumps TT generation and decays
/// ordering history exactly once per call.
pub fn search_root(
    board: &mut Board,
    tt: &mut TranspositionTable,
    ordering: &mut OrderingState,
    cfg: &SearchConfig,
) -> SearchResult {
    let start = Instant::now();
    let deadline = cfg.time_ms.map(|t| start + Duration::from_millis(t));
    tt.new_generation();
    ordering.decay_history();

    let mut result = SearchResult::default();
    // Prime the fallback so a depth-1 timeout still returns *some* legal
    // move instead of the ORIGIN sentinel — placing ORIGIN on a non-empty
    // board would raise from the Python boundary.
    let fallback_moves = moves::generate(board, DEFAULT_MOVE_RADIUS);
    if let Some(&m) = fallback_moves.first() {
        result.best_move = m;
    }
    let mut prev_score: Option<i32> = None;
    let mut node_count: u64 = 0;

    let max_depth = cfg.max_depth.max(1);
    for depth in 1..=max_depth {
        match iterate_at_depth(
            board,
            tt,
            ordering,
            cfg,
            depth,
            prev_score,
            deadline,
            &mut node_count,
        ) {
            Ok((score, best_move)) => {
                result = SearchResult {
                    best_move,
                    score,
                    depth_reached: depth,
                    nodes: node_count,
                    time_ms: elapsed_ms(start),
                };
                prev_score = Some(score);
            }
            Err(SearchError::Timeout) => break,
        }
        if deadline_reached(deadline) {
            break;
        }
    }
    result.time_ms = elapsed_ms(start);
    result.nodes = node_count;
    result
}

/// One full iterative-deepening iteration at fixed `depth`. Wraps PVS
/// with aspiration-window widening. Returns `(score, best_move)`.
#[allow(clippy::too_many_arguments)]
fn iterate_at_depth(
    board: &mut Board,
    tt: &mut TranspositionTable,
    ord: &mut OrderingState,
    cfg: &SearchConfig,
    depth: i8,
    prev_score: Option<i32>,
    deadline: Option<Instant>,
    node_count: &mut u64,
) -> Result<(i32, Coord), SearchError> {
    let root_hash = board.hash();
    let widen = i32::try_from(cfg.asp_window_widen_factor.max(2)).unwrap_or(2);
    let mut delta = cfg.asp_window_initial.max(1);

    let (mut alpha, mut beta) = match prev_score {
        Some(p) if depth >= ASPIRATION_START_DEPTH => (
            p.saturating_sub(delta).max(-INF),
            p.saturating_add(delta).min(INF),
        ),
        _ => (-INF, INF),
    };

    // Aspiration loop. Up to 2 narrow widens; on the third failure we
    // promote to full-window which always returns in-window and exits.
    let mut attempt = 0_u8;
    loop {
        let score = pvs_node(
            board,
            tt,
            ord,
            cfg,
            depth,
            alpha,
            beta,
            0,
            cfg.max_check_extensions,
            deadline,
            &[],
            node_count,
        )?;
        let fail_low = score <= alpha;
        let fail_high = score >= beta;
        let at_full_window = alpha == -INF && beta == INF;
        if (!fail_low && !fail_high) || at_full_window {
            let best = tt.probe(root_hash).map_or(ORIGIN, |entry| entry.best_move);
            return Ok((score, best));
        }
        attempt += 1;
        if attempt >= 2 {
            alpha = -INF;
            beta = INF;
            continue;
        }
        delta = delta.saturating_mul(widen);
        if fail_low {
            alpha = alpha.saturating_sub(delta).max(-INF);
        }
        if fail_high {
            beta = beta.saturating_add(delta).min(INF);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Recursive node
// ─────────────────────────────────────────────────────────────────────────────

/// Recursive alpha-beta node. X-positive scoring; the node maximizes when
/// `board.to_move() == X` and minimizes otherwise.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn pvs_node(
    board: &mut Board,
    tt: &mut TranspositionTable,
    ord: &mut OrderingState,
    cfg: &SearchConfig,
    depth: i8,
    mut alpha: i32,
    mut beta: i32,
    ply: u8,
    extensions_left: u8,
    deadline: Option<Instant>,
    stone1_defense: &[Coord],
    node_count: &mut u64,
) -> SearchScore {
    bump_and_check_deadline(node_count, cfg, deadline)?;

    if let Some(winner) = board.winner() {
        return Ok(terminal_score(winner, ply));
    }

    let side = board.to_move();
    let maximize = matches!(side, Player::X);
    let alpha_orig = alpha;
    let beta_orig = beta;

    // ── TT probe ────────────────────────────────────────────────────────────
    let mut tt_move: Option<Coord> = None;
    if let Some(entry) = tt.probe(board.hash()) {
        tt_move = Some(entry.best_move);
        if entry.depth >= depth {
            let adjusted = score_from_tt(entry.score, ply);
            match entry.flag {
                TTFlag::Exact => return Ok(adjusted),
                TTFlag::LowerBound if adjusted >= beta => return Ok(adjusted),
                TTFlag::UpperBound if adjusted <= alpha => return Ok(adjusted),
                _ => {}
            }
        }
    }

    if depth <= 0 {
        return quiescence_node(board, cfg, alpha, beta, ply, 0, deadline, node_count);
    }

    // ── Move generation + ordering ──────────────────────────────────────────
    let mut moves_list = moves::generate(board, DEFAULT_MOVE_RADIUS);
    if moves_list.is_empty() {
        return Ok(board.cached_eval());
    }

    let buckets: SmallVec<[u8; MOVE_GEN_CAP_INLINE]> = {
        let killers_idx = (ply as usize).min(MAX_PLY - 1);
        let killers_snap = ord.killers[killers_idx];
        let ctx = OrderingContext {
            board,
            side,
            tt_move,
            killers: &killers_snap,
            history: &ord.history,
            stone1_s0_defense: stone1_defense,
        };
        order_moves_with_buckets(&mut moves_list, &ctx)
    };

    let mut best_score = if maximize { -INF } else { INF };
    let mut best_move = moves_list.first().copied().unwrap_or(ORIGIN);

    // ── Move loop ───────────────────────────────────────────────────────────
    for (i, (&m, &bucket)) in moves_list.iter().zip(buckets.iter()).enumerate() {
        // Check extension: own move creates a fresh S0 against the
        // opponent and we still have budget.
        let extends_check = bucket == BUCKET_S0_CREATE && extensions_left > 0;
        let ext_amt: i8 = i8::from(extends_check);
        let new_extensions = if extends_check {
            extensions_left.saturating_sub(1)
        } else {
            extensions_left
        };
        let new_depth = (depth - 1 + ext_amt).max(0);

        // LMR.
        let lmr_excluded = is_lmr_excluded(bucket) || extends_check;
        let lmr_reduction =
            if !lmr_excluded && depth >= cfg.lmr_min_depth && (i as u8) >= cfg.lmr_min_move_index {
                cfg.lmr_reduction
            } else {
                0
            };
        let probe_depth = (new_depth - lmr_reduction).max(0);

        board
            .place(m)
            .expect("ordered move must be legal at search depth");
        let stone1_buf = collect_stone1_defense(board, m);

        let score_result = pvs_dance(
            board,
            tt,
            ord,
            cfg,
            new_depth,
            probe_depth,
            lmr_reduction,
            alpha,
            beta,
            maximize,
            ply,
            new_extensions,
            deadline,
            &stone1_buf,
            node_count,
            i == 0,
        );
        board
            .undo()
            .expect("undo own placement must succeed inside search");
        let score = score_result?;

        if maximize {
            if score > best_score {
                best_score = score;
                best_move = m;
            }
            if score > alpha {
                alpha = score;
            }
        } else {
            if score < best_score {
                best_score = score;
                best_move = m;
            }
            if score < beta {
                beta = score;
            }
        }
        if alpha >= beta {
            let cutoff_depth = depth.max(0);
            ord.record_cutoff(ply, m, side, cutoff_depth);
            break;
        }
    }

    // ── TT store ────────────────────────────────────────────────────────────
    let flag = if best_score <= alpha_orig {
        TTFlag::UpperBound
    } else if best_score >= beta_orig {
        TTFlag::LowerBound
    } else {
        TTFlag::Exact
    };
    tt.store(
        board.hash(),
        depth,
        score_to_tt(best_score, ply),
        flag,
        best_move,
    );

    Ok(best_score)
}

/// PVS dance after `place(m)` for move number `i`. Encapsulates the
/// null-window probe, optional LMR re-search, and full-window re-search.
/// Pulled out of [`pvs_node`] so `?` propagation doesn't bypass the
/// surrounding [`Board::undo`].
#[allow(clippy::too_many_arguments)]
fn pvs_dance(
    board: &mut Board,
    tt: &mut TranspositionTable,
    ord: &mut OrderingState,
    cfg: &SearchConfig,
    new_depth: i8,
    probe_depth: i8,
    lmr_reduction: i8,
    alpha: i32,
    beta: i32,
    maximize: bool,
    ply: u8,
    extensions_left: u8,
    deadline: Option<Instant>,
    stone1_defense: &[Coord],
    node_count: &mut u64,
    is_first: bool,
) -> SearchScore {
    if is_first {
        return pvs_node(
            board,
            tt,
            ord,
            cfg,
            new_depth,
            alpha,
            beta,
            ply + 1,
            extensions_left,
            deadline,
            stone1_defense,
            node_count,
        );
    }
    let (probe_a, probe_b) = if maximize {
        (alpha, alpha.saturating_add(1).min(INF))
    } else {
        (beta.saturating_sub(1).max(-INF), beta)
    };
    let probed = pvs_node(
        board,
        tt,
        ord,
        cfg,
        probe_depth,
        probe_a,
        probe_b,
        ply + 1,
        extensions_left,
        deadline,
        stone1_defense,
        node_count,
    )?;
    let beats_window = if maximize {
        probed > alpha
    } else {
        probed < beta
    };
    if !beats_window {
        return Ok(probed);
    }
    let after_lmr = if lmr_reduction > 0 && probe_depth < new_depth {
        pvs_node(
            board,
            tt,
            ord,
            cfg,
            new_depth,
            probe_a,
            probe_b,
            ply + 1,
            extensions_left,
            deadline,
            stone1_defense,
            node_count,
        )?
    } else {
        probed
    };
    let beats_window2 = if maximize {
        after_lmr > alpha
    } else {
        after_lmr < beta
    };
    let needs_full = beats_window2
        && if maximize {
            after_lmr < beta
        } else {
            after_lmr > alpha
        };
    if needs_full {
        pvs_node(
            board,
            tt,
            ord,
            cfg,
            new_depth,
            alpha,
            beta,
            ply + 1,
            extensions_left,
            deadline,
            stone1_defense,
            node_count,
        )
    } else {
        Ok(after_lmr)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Quiescence
// ─────────────────────────────────────────────────────────────────────────────

/// Threat-only quiescence. Stand-pat with the cached static eval; only
/// recurse on moves that create an own S0, block an opponent S0, or make
/// a 6-in-row. Hard-capped at `cfg.qsearch_max_plies`.
#[allow(clippy::too_many_arguments)]
fn quiescence_node(
    board: &mut Board,
    cfg: &SearchConfig,
    mut alpha: i32,
    mut beta: i32,
    ply: u8,
    q_ply: u8,
    deadline: Option<Instant>,
    node_count: &mut u64,
) -> SearchScore {
    bump_and_check_deadline(node_count, cfg, deadline)?;
    if let Some(winner) = board.winner() {
        return Ok(terminal_score(winner, ply));
    }

    let side = board.to_move();
    let maximize = matches!(side, Player::X);
    let static_eval = board.cached_eval();

    if maximize {
        if static_eval >= beta {
            return Ok(beta);
        }
        if static_eval > alpha {
            alpha = static_eval;
        }
    } else {
        if static_eval <= alpha {
            return Ok(alpha);
        }
        if static_eval < beta {
            beta = static_eval;
        }
    }

    if q_ply >= cfg.qsearch_max_plies {
        return Ok(if maximize { alpha } else { beta });
    }

    // Threat-only filter on the inner-radius candidates.
    let candidates = moves::generate(board, DEFAULT_MOVE_RADIUS);
    let mut threats: SmallVec<[Coord; MOVE_GEN_CAP_INLINE]> = SmallVec::new();
    for m in candidates.iter().copied() {
        if is_threat_move(board, m, side) {
            threats.push(m);
        }
    }
    if threats.is_empty() {
        return Ok(if maximize { alpha } else { beta });
    }

    let mut best = if maximize { alpha } else { beta };
    for m in threats {
        board
            .place(m)
            .expect("ordered threat must be legal in qsearch");
        let r = quiescence_node(
            board,
            cfg,
            alpha,
            beta,
            ply + 1,
            q_ply + 1,
            deadline,
            node_count,
        );
        board.undo().expect("undo own placement in qsearch");
        let score = r?;
        if maximize {
            if score >= beta {
                return Ok(beta);
            }
            if score > best {
                best = score;
            }
            if score > alpha {
                alpha = score;
            }
        } else {
            if score <= alpha {
                return Ok(alpha);
            }
            if score < best {
                best = score;
            }
            if score < beta {
                beta = score;
            }
        }
    }
    Ok(best)
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Encoding value emitted by `ordering::bucket_value` for "creates own
/// S0". Mirrored here for the check-extension predicate so search and
/// ordering share one source of truth.
const BUCKET_S0_CREATE: u8 = 6;

/// Ordering encoding values that are LMR-excluded: TT (10), own win (9),
/// block opponent win (8), stone-1 S0 defense (7), creates S0 (6), blocks
/// opponent S0 (5), killer (3). Anything else (S1 create / history / etc.)
/// is reducible.
#[inline]
fn is_lmr_excluded(bucket: u8) -> bool {
    matches!(bucket, 3 | 5..=10)
}

/// Mate score with mate-distance accounting. `ply` is the search ply at
/// which the terminal position was observed; closer mates score higher.
/// Cold: terminal positions are rare in the inner search loop.
#[cold]
#[inline]
fn terminal_score(winner: Player, ply: u8) -> i32 {
    let mag = MATE_SCORE - i32::from(ply);
    match winner {
        Player::X => mag,
        Player::O => -mag,
    }
}

/// Encode a node-relative score into a ply-agnostic TT value.
///
/// Mate-class scores carry their absolute search ply; storing them as-is
/// gives the wrong distance back to a transposition reached at a
/// different ply. We shift mate-class scores so the TT slot represents
/// "mate at distance `d` from the stored node" and the probe re-anchors
/// to the current ply.
#[inline]
fn score_to_tt(score: i32, ply: u8) -> i32 {
    if score >= MATE_BOUND {
        score + i32::from(ply)
    } else if score <= -MATE_BOUND {
        score - i32::from(ply)
    } else {
        score
    }
}

/// Inverse of [`score_to_tt`]. Apply to a probed entry before comparing
/// against the current node's alpha/beta.
#[inline]
fn score_from_tt(score: i32, ply: u8) -> i32 {
    if score >= MATE_BOUND {
        score - i32::from(ply)
    } else if score <= -MATE_BOUND {
        score + i32::from(ply)
    } else {
        score
    }
}

/// `is_threat_move` predicates for quiescence — share definitions with
/// the ordering module so the qsearch frontier matches bucket 5/6/9.
#[inline]
fn is_threat_move(board: &Board, m: Coord, side: Player) -> bool {
    ordering::would_make_six(board, m, side)
        || ordering::creates_s0(board, m, side)
        || ordering::blocks_opp_s0(board, m, side)
}

/// If the just-placed move at `m` left us halfway through a `HeXO` turn
/// AND it created an S0 for the same side, return that S0's defense
/// cells so the next node can bucket-7 prioritize completing it.
fn collect_stone1_defense(board: &Board, m: Coord) -> SmallVec<[Coord; 4]> {
    if board.halfmove() != 1 || board.winner().is_some() {
        return SmallVec::new();
    }
    // After `place`, `to_move()` is the same side that just played
    // (their stone-2 is next).
    let side = board.to_move();
    let threats = board.threats(side);
    threats
        .s0_instances
        .iter()
        .find(|inst| inst.pieces.contains(&m))
        .map(|inst| inst.defense_cells.clone())
        .unwrap_or_default()
}

#[inline]
fn elapsed_ms(start: Instant) -> u64 {
    let elapsed = start.elapsed();
    let millis = elapsed.as_millis();
    if millis > u128::from(u64::MAX) {
        u64::MAX
    } else {
        millis as u64
    }
}

#[inline]
fn deadline_reached(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|d| Instant::now() >= d)
}

#[inline]
fn bump_and_check_deadline(
    node_count: &mut u64,
    cfg: &SearchConfig,
    deadline: Option<Instant>,
) -> Result<(), SearchError> {
    *node_count = node_count.wrapping_add(1);
    let step = u64::from(cfg.deadline_check_nodes.max(1));
    if *node_count % step == 0 && deadline_reached(deadline) {
        return cold_timeout();
    }
    Ok(())
}

#[cold]
#[inline(never)]
fn cold_timeout() -> Result<(), SearchError> {
    Err(SearchError::Timeout)
}

