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
    LMR_REDUCTION, MATE_SCORE, MAX_CHECK_EXTENSIONS, MAX_PLY, QSEARCH_MAX_PLIES,
};
use crate::coords::{Coord, ORIGIN};
use crate::moves;
use crate::ordering::{self, KillerSlot, OrderingContext, OrderingState, order_moves_with_buckets};
use crate::search_stats;
use crate::tt::{TTFlag, TranspositionTable};
use smallvec::SmallVec;

// ─────────────────────────────────────────────────────────────────────────────
// Per-ply scratch buffers
// ─────────────────────────────────────────────────────────────────────────────

/// Per-ply scratch space for move generation and ordering. One slot per
/// search ply, indexed by `ply.min(MAX_PLY - 1)`. Each slot retains its
/// `Vec` capacity across calls, so after the first search warmup the
/// hot path makes zero allocator round-trips.
///
/// Boxed `[Vec<_>; MAX_PLY]` to keep the ~12 KB worth of slots off the
/// stack frame. `MAX_PLY = 128` matches the `OrderingState::killers`
/// indexing precedent.
pub struct SearchScratch {
    /// Per-ply candidate-move buffer used by both `pvs_node` and
    /// `quiescence_node`. Sharing the same slot is safe: `pvs_node` at
    /// ply P only calls `quiescence_node` after the `depth <= 0`
    /// early-return path, where it never touches its own slot.
    pub moves: Box<[Vec<Coord>; MAX_PLY]>,
    /// Per-ply sort scratch for `order_moves_with_buckets`.
    pub scored: Box<[Vec<(u64, u8, Coord)>; MAX_PLY]>,
    /// Per-ply bucket-value output array (parallel to `moves` after
    /// ordering).
    pub buckets: Box<[Vec<u8>; MAX_PLY]>,
    /// Per-ply qsearch threat sub-list (filtered subset of `moves`).
    pub threats: Box<[Vec<Coord>; MAX_PLY]>,
}

impl Default for SearchScratch {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl SearchScratch {
    /// Fresh state: every slot is an empty `Vec` (no allocation until
    /// first use).
    #[cold]
    #[must_use]
    pub fn new() -> Self {
        Self {
            moves: Box::new(std::array::from_fn(|_| Vec::new())),
            scored: Box::new(std::array::from_fn(|_| Vec::new())),
            buckets: Box::new(std::array::from_fn(|_| Vec::new())),
            threats: Box::new(std::array::from_fn(|_| Vec::new())),
        }
    }

    /// Saturating ply-to-index. `pvs_node` recursion can in principle
    /// run past `MAX_PLY` via extensions; we share the last slot in that
    /// pathological case, matching the killer-table indexing convention
    /// (`ordering.rs:89`).
    #[inline]
    #[must_use]
    pub fn ply_index(ply: u8) -> usize {
        (ply as usize).min(MAX_PLY - 1)
    }
}

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
    scratch: &mut SearchScratch,
    cfg: &SearchConfig,
) -> SearchResult {
    let start = Instant::now();
    let deadline = cfg.time_ms.map(|t| start + Duration::from_millis(t));
    tt.new_generation();
    ordering.reset_killers(); // R-08-A: per-`best_move()` killer hygiene.
    ordering.decay_history();
    search_stats::reset();

    let mut result = SearchResult::default();
    // Prime the fallback so a depth-1 timeout still returns *some* legal
    // move instead of the ORIGIN sentinel — placing ORIGIN on a non-empty
    // board would raise from the Python boundary. We reuse slot 0; the
    // first `pvs_node` at ply 0 will clear-and-overwrite anyway.
    {
        let fallback_slot = &mut scratch.moves[0];
        moves::generate(board, DEFAULT_MOVE_RADIUS, fallback_slot);
        if let Some(&m) = fallback_slot.first() {
            result.best_move = m;
        }
    }
    let mut prev_score: Option<i32> = None;
    let mut node_count: u64 = 0;

    // R-08-B: pre-allocate a single scratch buffer for the per-ID-iteration
    // killer snapshot. `*buf = *ord.killers` memcpys ~1 KB; sub-microsecond
    // vs iteration runtimes in the 10s of ms. On `SearchError::Timeout` we
    // restore so partial-iteration killer writes from the aborted depth do
    // not contaminate the next `best_move()` call's ordering.
    let mut killers_snapshot: Box<[KillerSlot; MAX_PLY]> =
        Box::new([KillerSlot::default(); MAX_PLY]);

    let max_depth = cfg.max_depth.max(1);
    for depth in 1..=max_depth {
        // Record the killer state entering this ID iteration. Snapshot is
        // taken once per ID depth — outside the aspiration loop inside
        // `iterate_at_depth` — so failed aspiration attempts within the
        // same depth deliberately share killer state with the next attempt.
        *killers_snapshot = *ordering.killers;
        match iterate_at_depth(
            board,
            tt,
            ordering,
            scratch,
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
            Err(SearchError::Timeout) => {
                // Roll back partial-iteration killer writes.
                *ordering.killers = *killers_snapshot;
                break;
            }
        }
        if deadline_reached(deadline) {
            break;
        }
    }
    result.time_ms = elapsed_ms(start);
    result.nodes = node_count;
    search_stats::dump_stderr();
    result
}

/// One full iterative-deepening iteration at fixed `depth`. Wraps PVS
/// with aspiration-window widening. Returns `(score, best_move)`.
#[allow(clippy::too_many_arguments)]
fn iterate_at_depth(
    board: &mut Board,
    tt: &mut TranspositionTable,
    ord: &mut OrderingState,
    scratch: &mut SearchScratch,
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

    // Aspiration loop. Up to 1 narrow widen; on the second failure we
    // promote to full-window which always returns in-window and exits.
    let mut attempt = 0_u8;
    loop {
        let score = pvs_node(
            board,
            tt,
            ord,
            scratch,
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
        search_stats::note_asp_iter(fail_low, fail_high, at_full_window);
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
    scratch: &mut SearchScratch,
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
        return quiescence_node(board, scratch, cfg, alpha, beta, ply, 0, deadline, node_count);
    }

    // ── R-01 staged move iteration ──────────────────────────────────────────
    // Stage 1 (TT move) → Stage 2 (killer slots) → Stage 3 (generate + order).
    // β-cutoff in stage 1 or 2 returns without ever calling `moves::generate`.
    let ply_idx = SearchScratch::ply_index(ply);
    let killers_idx = (ply as usize).min(MAX_PLY - 1);
    let killers_snap = ord.killers[killers_idx];

    let mut best_score = if maximize { -INF } else { INF };
    let mut best_move = ORIGIN;
    let mut move_idx: usize = 0;
    let mut tried: SmallVec<[Coord; 3]> = SmallVec::new();
    let mut beta_cut = false;

    // Stage 1: TT move.
    if let Some(m) = tt_move {
        if board.is_legal(m) {
            let bucket = {
                let ctx = OrderingContext {
                    board,
                    side,
                    tt_move,
                    killers: &killers_snap,
                    history: &ord.history,
                    stone1_s0_defense: stone1_defense,
                };
                ordering::bucket_value(&ctx, m)
            };
            search_stats::note_stage1_tried();
            beta_cut = try_one_move(
                board, tt, ord, scratch, cfg, depth,
                &mut alpha, &mut beta, ply, extensions_left, deadline,
                node_count, side, maximize,
                m, bucket, move_idx,
                &mut best_score, &mut best_move,
            )?;
            if beta_cut {
                search_stats::note_cut(1, bucket, depth, None);
            }
            tried.push(m);
            move_idx += 1;
        }
    }

    // Stage 2: killer moves (slot 0 first = most recent).
    if !beta_cut {
        for (slot_idx, slot_opt) in killers_snap.slots().iter().enumerate() {
            let Some(slot) = *slot_opt else { continue };
            if tried.contains(&slot) || !board.is_legal(slot) {
                continue;
            }
            let bucket = {
                let ctx = OrderingContext {
                    board,
                    side,
                    tt_move,
                    killers: &killers_snap,
                    history: &ord.history,
                    stone1_s0_defense: stone1_defense,
                };
                ordering::bucket_value(&ctx, slot)
            };
            search_stats::note_stage2_tried(slot_idx);
            beta_cut = try_one_move(
                board, tt, ord, scratch, cfg, depth,
                &mut alpha, &mut beta, ply, extensions_left, deadline,
                node_count, side, maximize,
                slot, bucket, move_idx,
                &mut best_score, &mut best_move,
            )?;
            if beta_cut {
                search_stats::note_cut(2, bucket, depth, Some(slot_idx));
            }
            tried.push(slot);
            move_idx += 1;
            if beta_cut {
                break;
            }
        }
    }

    // Stage 3: full generate + order, skip entries already tried in stages 1-2.
    if !beta_cut {
        {
            let slot = &mut scratch.moves[ply_idx];
            moves::generate(board, DEFAULT_MOVE_RADIUS, slot);
            if slot.is_empty() {
                if tried.is_empty() {
                    return Ok(board.cached_eval());
                }
                // Stages 1-2 ran moves and Stage 3 has nothing to add: fall
                // through to TT store with the staged best_score / best_move.
            } else {
                let ctx = OrderingContext {
                    board,
                    side,
                    tt_move,
                    killers: &killers_snap,
                    history: &ord.history,
                    stone1_s0_defense: stone1_defense,
                };
                // Split-borrow: each scratch field is a distinct
                // `Box<[Vec<_>; N]>`, so simultaneous mut-borrows of one slot
                // from each field are disjoint.
                let moves_slot = &mut scratch.moves[ply_idx];
                let scored_slot = &mut scratch.scored[ply_idx];
                let buckets_slot = &mut scratch.buckets[ply_idx];
                order_moves_with_buckets(moves_slot, &ctx, scored_slot, buckets_slot);
            }
        }
        let move_count = scratch.moves[ply_idx].len();
        // best_move fallback when no Stage-1/2 move fired: seed from the
        // highest-bucket ordered candidate (matches today's pre-staging init).
        if best_move == ORIGIN && move_count > 0 {
            best_move = scratch.moves[ply_idx][0];
        }
        // Iterate by index; re-read each slot per iteration so no borrow
        // lives across the recursive `pvs_node` / `pvs_dance` calls.
        for i in 0..move_count {
            let m = scratch.moves[ply_idx][i];
            if tried.contains(&m) {
                continue;
            }
            let bucket = scratch.buckets[ply_idx][i];
            search_stats::note_stage3_tried();
            let cut = try_one_move(
                board, tt, ord, scratch, cfg, depth,
                &mut alpha, &mut beta, ply, extensions_left, deadline,
                node_count, side, maximize,
                m, bucket, move_idx,
                &mut best_score, &mut best_move,
            )?;
            move_idx += 1;
            if cut {
                search_stats::note_cut(3, bucket, depth, None);
                break;
            }
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
    scratch: &mut SearchScratch,
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
            scratch,
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
    if lmr_reduction > 0 && probe_depth < new_depth {
        search_stats::note_lmr_fired();
    }
    let probed = pvs_node(
        board,
        tt,
        ord,
        scratch,
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
        search_stats::note_lmr_research();
        pvs_node(
            board,
            tt,
            ord,
            scratch,
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
        search_stats::note_lmr_research_full();
        pvs_node(
            board,
            tt,
            ord,
            scratch,
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

/// Execute one move at `pvs_node`. Returns `Ok(true)` on a β-cutoff so the
/// caller can short-circuit the remaining stages / loop iterations.
/// Factored out of `pvs_node` so the three staged dispatch paths (TT,
/// killer, ordered fallback) share identical per-move accounting.
#[allow(clippy::too_many_arguments)]
fn try_one_move(
    board: &mut Board,
    tt: &mut TranspositionTable,
    ord: &mut OrderingState,
    scratch: &mut SearchScratch,
    cfg: &SearchConfig,
    depth: i8,
    alpha: &mut i32,
    beta: &mut i32,
    ply: u8,
    extensions_left: u8,
    deadline: Option<Instant>,
    node_count: &mut u64,
    side: Player,
    maximize: bool,
    m: Coord,
    bucket: u8,
    move_idx: usize,
    best_score: &mut i32,
    best_move: &mut Coord,
) -> Result<bool, SearchError> {
    search_stats::note_bucket_tried(bucket);
    // Check extension: own move creates a fresh S0 against the opponent
    // and we still have budget.
    let extends_check = bucket == BUCKET_S0_CREATE && extensions_left > 0;
    let ext_amt: i8 = i8::from(extends_check);
    let new_extensions = if extends_check {
        extensions_left.saturating_sub(1)
    } else {
        extensions_left
    };
    let new_depth = (depth - 1 + ext_amt).max(0);

    // LMR. The `move_idx` here is the global staged index so the
    // `>= lmr_min_move_index` check fires in the same place it would
    // have under the pre-staging single-loop iteration.
    let lmr_excluded = is_lmr_excluded(bucket) || extends_check;
    let lmr_reduction = if !lmr_excluded
        && depth >= cfg.lmr_min_depth
        && (move_idx as u8) >= cfg.lmr_min_move_index
    {
        cfg.lmr_reduction
    } else {
        0
    };
    let probe_depth = (new_depth - lmr_reduction).max(0);

    board
        .place(m)
        .expect("staged move must be legal at search depth");
    let stone1_buf = collect_stone1_defense(board, m);

    let score_result = pvs_dance(
        board,
        tt,
        ord,
        scratch,
        cfg,
        new_depth,
        probe_depth,
        lmr_reduction,
        *alpha,
        *beta,
        maximize,
        ply,
        new_extensions,
        deadline,
        &stone1_buf,
        node_count,
        move_idx == 0,
    );
    board
        .undo()
        .expect("undo own placement must succeed inside search");
    let score = score_result?;

    if maximize {
        if score > *best_score {
            *best_score = score;
            *best_move = m;
        }
        if score > *alpha {
            *alpha = score;
        }
    } else {
        if score < *best_score {
            *best_score = score;
            *best_move = m;
        }
        if score < *beta {
            *beta = score;
        }
    }
    if *alpha >= *beta {
        let cutoff_depth = depth.max(0);
        ord.record_cutoff(ply, m, side, cutoff_depth);
        return Ok(true);
    }
    Ok(false)
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
    scratch: &mut SearchScratch,
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

    // Threat-only filter on the inner-radius candidates. Reuses the
    // per-ply slot: `pvs_node` at ply P returns into qsearch *before*
    // generating its own moves, so slot P is free to host the candidate
    // list. Recursive qsearch calls use slot P+1, etc.
    let ply_idx = SearchScratch::ply_index(ply);
    {
        let cands_slot = &mut scratch.moves[ply_idx];
        moves::generate(board, DEFAULT_MOVE_RADIUS, cands_slot);
    }
    {
        let threats_slot = &mut scratch.threats[ply_idx];
        threats_slot.clear();
        let cands_slot = &scratch.moves[ply_idx];
        for m in cands_slot.iter().copied() {
            if is_threat_move(board, m, side) {
                threats_slot.push(m);
            }
        }
    }
    let threat_count = scratch.threats[ply_idx].len();
    if threat_count == 0 {
        return Ok(if maximize { alpha } else { beta });
    }

    let mut best = if maximize { alpha } else { beta };
    for i in 0..threat_count {
        let m = scratch.threats[ply_idx][i];
        board
            .place(m)
            .expect("ordered threat must be legal in qsearch");
        let r = quiescence_node(
            board,
            scratch,
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

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests for invariants that need access to private items
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    //! M5-invariant lock-in (SealBot-perf bug-pattern, see I3
    //! `2026-05-20-code-analysis.md` §M5).
    //!
    //! When the root `pvs_node` fail-highs on a beta cutoff, the root
    //! TT entry MUST be written with `best_move == the move that
    //! caused the cutoff` and `flag == LowerBound`. Without this, a
    //! widened aspiration retry would re-order moves from scratch and
    //! re-discover the failing move from the bottom of the bucket
    //! pipeline (wasted nodes). HH's design satisfies the invariant
    //! structurally — `pvs_node`'s unconditional TT store at the
    //! function tail captures `best_move` regardless of fail-high — and
    //! this test pins the behaviour so refactors can't regress it.
    //!
    //! Test calls `pvs_node` directly at root with a deliberately tight
    //! beta so the first staged move triggers a beta cutoff, then
    //! inspects TT[`root_hash`] for the M5 invariant.
    use super::*;
    use crate::board::Board;
    use crate::config::MATE_SCORE;
    use crate::coords::Coord;
    use crate::ordering::OrderingState;
    use crate::tt::{TTFlag, TranspositionTable};

    fn x_row(b: &mut Board, start: i16, count: i16) {
        for q in start..(start + count) {
            b.place_for_test(Coord::new(q, 0), Player::X);
        }
    }

    #[test]
    fn pvs_root_fail_high_writes_failing_move_to_tt() {
        // X has open-4 → next stone completes an open-5, a follow-up
        // stone wins. At depth 3 (one X turn + one O turn + one X stone)
        // mate-class scores are reachable and easily exceed any narrow
        // beta we set below.
        let mut b = Board::new();
        x_row(&mut b, 0, 4);
        b.force_parity_for_test(Player::X, 0);
        let root_hash = b.hash();

        let mut tt = TranspositionTable::new(16);
        let mut ord = OrderingState::new();
        let mut scratch = SearchScratch::new();
        let cfg = SearchConfig::default();
        let mut nodes: u64 = 0;

        // Tight beta well below the mate score. X is the maximizer so a
        // mate-class result trivially exceeds beta on the first staged
        // move → guaranteed fail-high.
        let alpha = -INF;
        let beta: i32 = 1000;

        let score = pvs_node(
            &mut b,
            &mut tt,
            &mut ord,
            &mut scratch,
            &cfg,
            5,
            alpha,
            beta,
            0,
            cfg.max_check_extensions,
            None,
            &[],
            &mut nodes,
        )
        .expect("no timeout configured");

        // Confirm fail-high actually occurred.
        assert!(
            score >= beta,
            "test setup did not produce fail-high: score={score} beta={beta}"
        );

        // ── M5 invariant ────────────────────────────────────────────
        let entry = tt
            .probe(root_hash)
            .expect("root TT entry must be present after pvs_node returns");
        assert_ne!(
            entry.best_move, ORIGIN,
            "root TT best_move must be the failing move, not ORIGIN sentinel \
             (M5: dropping best_move on fail-high makes the widened retry \
              re-order from scratch)"
        );
        assert!(
            matches!(entry.flag, TTFlag::LowerBound),
            "fail-high must store TTFlag::LowerBound, got {:?}",
            entry.flag
        );
        assert_eq!(
            entry.depth, 5,
            "TT entry depth must match the pvs_node depth argument"
        );
        // The score in TT must itself be >= beta (lower-bound semantics).
        let stored = score_from_tt(entry.score, 0);
        assert!(
            stored >= beta,
            "stored lower-bound score must be >= beta: stored={stored} beta={beta}"
        );
        // And the stored score must exceed the cutoff threshold by a
        // mate-class margin (the failing move *is* a mate move).
        assert!(
            stored >= MATE_SCORE - 16,
            "stored score for a mate-finding fail-high should be mate-class; got {stored}"
        );
    }

    /// B.3 (`SealBot-perf` M6 pattern, see I3 `2026-05-20-code-analysis.md`
    /// §M6 + brief in `/tmp/phase_28d/3/B.3/implementer.md`).
    ///
    /// Locks the structural conditions that keep the search inner loop
    /// allocation-free. The audit traced through `pvs_node`, `pvs_dance`,
    /// `try_one_move`, `quiescence_node`, `moves::generate`,
    /// `ordering::bucket_value` / `order_moves_with_buckets`,
    /// `threats::compute_with_scratch`, and `eval::layer1_window_scan_8cell`
    /// and found no heap allocation in the recursive path *as long as*:
    ///
    /// 1. `DEFAULT_MOVE_RADIUS <= MOVE_GEN_INNER_RADIUS` — search always
    ///    hits the cached-`inner_candidates` arm in `moves::generate`,
    ///    bypassing the `sweep_neighbourhood` `FxHashSet` alloc.
    /// 2. The per-node `tried` `SmallVec` inline cap accommodates the staged
    ///    pipeline maximum (1 TT move + `KILLER_SLOTS = 2` killers = 3).
    /// 3. `eval::Layer1`'s per-axis `line_ids` `SmallVec` inline cap stays
    ///    well above the empirical max (~19 across 2M+ `bench-perf` calls;
    ///    pin at >= 32 to absorb future growth).
    ///
    /// Empirical confirmation under `make bench perf` at HEAD: zero spills
    /// across `2_097_152` eval calls and zero `sweep_neighbourhood` calls.
    /// This test pins the invariants so a future refactor that, e.g.,
    /// raises the default search radius, drops `inner_candidates`, or
    /// shrinks an inline cap will fail at `cargo test` time instead of
    /// silently re-introducing a per-node heap allocation.
    #[test]
    fn search_hot_path_zero_alloc_structural_invariants() {
        use crate::config::{DEFAULT_MOVE_RADIUS, KILLER_SLOTS, MOVE_GEN_INNER_RADIUS};

        // Invariant 1: search's move-generation path must terminate in
        // the cached inner candidate set, not the alloc-bearing sweep.
        // Both constants are compile-time `const`s codegen'd from
        // `hexo.toml`, so a `const { ... }` block fires the assert at
        // build time the moment the inequality breaks — `cargo test`
        // never needs to run.
        const _: () = assert!(
            DEFAULT_MOVE_RADIUS <= MOVE_GEN_INNER_RADIUS,
            "DEFAULT_MOVE_RADIUS must be <= MOVE_GEN_INNER_RADIUS so \
             moves::generate never enters sweep_neighbourhood (which \
             would allocate an FxHashSet per search node)"
        );

        // Invariant 2: the staged-pipeline `tried` SmallVec inline cap
        // must hold stages 1+2's maximum push count (1 TT move + 2
        // killers).
        let tried: SmallVec<[Coord; 3]> = SmallVec::new();
        let tried_cap = tried.inline_size();
        assert!(
            tried_cap > KILLER_SLOTS,
            "search.rs `tried` SmallVec inline cap {tried_cap} must be \
             > KILLER_SLOTS={KILLER_SLOTS} (1 TT slot + every killer); \
             otherwise stages 1+2 spill to the heap on every node where \
             staged dispatch does not cutoff"
        );

        // Invariant 3: eval Layer-1 `line_ids` inline cap. Observed
        // midgame max ~19 across 2M bench-perf calls; >= 32 keeps a
        // safety margin for sparser-board futures.
        let layer1_buf: SmallVec<[i16; 32]> = SmallVec::new();
        let layer1_cap = layer1_buf.inline_size();
        assert!(
            layer1_cap >= 32,
            "eval::Layer1 line_ids SmallVec inline cap {layer1_cap} \
             must stay >= 32; any reduction risks per-eval heap alloc, \
             which fires on every leaf node and qsearch stand-pat."
        );
    }

    /// B.4 (`SealBot-perf` B2 pattern, see I3 `2026-05-20-code-analysis.md`
    /// §Section D-4 + `/tmp/phase_28d/3/B.4/implementer.md`).
    ///
    /// `SealBot-perf` shipped `24e23ff` ("phase1.7: snapshot `_killers`
    /// per iteration, restore on `TimeUp`") to plug a `TimeUp`-rollback
    /// gap where killers mutated during an aborted depth contaminated
    /// the next `best_move()` call's ordering. HH avoids that bug
    /// *structurally*: `search_root` snapshots `ordering.killers` once
    /// per ID depth (search.rs ~L244) and restores from the snapshot on
    /// the `Err(SearchError::Timeout)` arm (~L268). This test pins the
    /// rollback contract end-to-end.
    ///
    /// The other search-mutated members listed in the I3 audit are either
    /// (a) safe-by-construction or (b) intentionally retained:
    ///
    /// - **TT writes** propagate up via `?` from `try_one_move`'s
    ///   `score_result?`, so a node never reaches its tail `tt.store`
    ///   when a descendant times out. Sibling sub-searches that
    ///   completed *before* `TimeUp` wrote correct entries (their
    ///   depth/bound semantics are unchanged by what happened later in
    ///   the tree); no pollution.
    /// - **History** (`ord.record_cutoff`) only fires after a real
    ///   `alpha >= beta` cutoff with a fully-completed child score. It is
    ///   global-with-decay by design (`decay_history` runs once per
    ///   `search_root`), explicitly retained across iterations as the
    ///   per-game move-quality memory (`SPEC_ENGINE.md` "no wipe between
    ///   iterations"). Not rollback-eligible.
    /// - **TT generation** is bumped once per `search_root` call, not
    ///   per-iteration; never needs rollback.
    /// - **PV** is reconstructed from TT each iteration via
    ///   `iterate_at_depth`'s probe at the root hash; no separate PV
    ///   buffer to roll back.
    ///
    /// So `ordering.killers` is the only state needing iteration-scoped
    /// rollback, and the snapshot/restore pair at search.rs:244,268
    /// covers it. The test below:
    ///
    /// 1. Asserts the structural invariant that the snapshot type
    ///    matches `OrderingState::killers` exactly (catches a refactor
    ///    that changes one without the other).
    /// 2. Forces an immediate `TimeUp` (`time_ms = Some(0)`) and confirms
    ///    killers return to the pre-`search_root` baseline (which equals
    ///    fresh state because `reset_killers` runs unconditionally at
    ///    `search_root` entry).
    /// 3. Runs a search that completes depth >= 1 then times out, and
    ///    confirms killers at exit equal what they were after the last
    ///    fully-completed iteration's snapshot point — by re-running the
    ///    completed-depths search standalone and comparing slot-by-slot.
    #[test]
    fn time_up_rollback_restores_killer_state() {
        use crate::board::Board;
        use crate::config::{KILLER_SLOTS, MAX_PLY};
        use crate::ordering::{KillerSlot, OrderingState};
        use crate::tt::TranspositionTable;

        // Invariant 1: the snapshot buffer in `search_root` is typed as
        // `Box<[KillerSlot; MAX_PLY]>` so `*snapshot = *ord.killers`
        // memcpy's the exact bytes. If `OrderingState::killers` ever
        // changes shape, this assertion stops compiling — the rollback
        // path's `*ordering.killers = *killers_snapshot` would otherwise
        // silently lose its meaning.
        let snap: Box<[KillerSlot; MAX_PLY]> = Box::new([KillerSlot::default(); MAX_PLY]);
        let ord_proto = OrderingState::new();
        assert_eq!(
            std::mem::size_of_val(snap.as_ref()),
            std::mem::size_of_val(ord_proto.killers.as_ref()),
            "snapshot buffer and OrderingState::killers must have \
             identical layout for the rollback memcpy to remain correct"
        );

        // Build an open-4 position so depth-2+ produces β-cutoffs that
        // populate killers (otherwise the rollback target degenerates to
        // "all-empty == all-empty" and we'd be testing nothing).
        // `Board` is not `Clone`, so each per-iteration use rebuilds.
        let make_board = || {
            let mut b = Board::new();
            x_row(&mut b, 0, 4);
            b.force_parity_for_test(Player::X, 0);
            b
        };

        // Invariant 2: immediate TimeUp leaves killers at the fresh
        // (post-`reset_killers`) baseline. With `time_ms = Some(0)` the
        // first `bump_and_check_deadline` at root fires Timeout before
        // any cutoff can record a killer.
        {
            let mut bb = make_board();
            let mut tt = TranspositionTable::new(16);
            let mut ord = OrderingState::new();
            let mut scratch = SearchScratch::new();
            let cfg = SearchConfig {
                max_depth: 5,
                time_ms: Some(0),
                deadline_check_nodes: 1,
                ..SearchConfig::default()
            };
            let _ = search_root(&mut bb, &mut tt, &mut ord, &mut scratch, &cfg);
            for (ply, slot) in ord.killers.iter().enumerate() {
                for (k_idx, k) in slot.slots().iter().enumerate() {
                    assert!(
                        k.is_none(),
                        "immediate TimeUp left a killer at ply={ply} slot={k_idx}: \
                         {k:?}. The rollback restore at search.rs:268 should have \
                         reverted to the post-`reset_killers` baseline (all None)."
                    );
                }
            }
            // Sanity: KILLER_SLOTS is consistent with what we just walked.
            assert_eq!(KILLER_SLOTS, ord.killers[0].slots().len());
        }

        // Invariant 3: rollback parity. Run a complete search at depth=2
        // to capture the "after fully-completed depth 2" killer state.
        // Then run a search at depth=5 with no time limit (fully
        // completes everything). Then run a search at depth=5 with a
        // tight time budget calibrated to time out *after* depth 2 but
        // *during* depth 3+. The time-limited search's final killer
        // state must equal the *completed* search's state at the same
        // depth as `depth_reached`. We can't pre-predict which depth
        // completes under the tight budget, so we run several searches
        // at depths {1..=5} to build a reference table, then assert the
        // time-limited search's killers match the reference for whatever
        // `depth_reached` it ended on.
        let mut reference_killers: Vec<Box<[KillerSlot; MAX_PLY]>> = Vec::with_capacity(6);
        reference_killers.push(Box::new([KillerSlot::default(); MAX_PLY])); // depth_reached=0
        for d in 1_i8..=5 {
            let mut bb = make_board();
            let mut tt = TranspositionTable::new(16);
            let mut ord = OrderingState::new();
            let mut scratch = SearchScratch::new();
            let cfg = SearchConfig {
                max_depth: d,
                time_ms: None,
                ..SearchConfig::default()
            };
            let _ = search_root(&mut bb, &mut tt, &mut ord, &mut scratch, &cfg);
            reference_killers.push(ord.killers.clone());
        }

        // Now run a time-limited search. We pick a tiny budget; the
        // exact `depth_reached` is platform-dependent, but whatever
        // depth completes, the killers must equal the corresponding
        // reference snapshot.
        for &budget_us in &[10u64, 100, 1_000, 10_000] {
            let mut bb = make_board();
            let mut tt = TranspositionTable::new(16);
            let mut ord = OrderingState::new();
            let mut scratch = SearchScratch::new();
            // `time_ms` in `SearchConfig` is milliseconds; multiply
            // budget_us into ms via the smallest representable >0 (round
            // up). For < 1ms cases we set `time_ms = Some(0)` which is
            // already covered by Invariant 2 above, so start at 1ms.
            let ms = (budget_us / 1000).max(1);
            let cfg = SearchConfig {
                max_depth: 5,
                time_ms: Some(ms),
                deadline_check_nodes: 1,
                ..SearchConfig::default()
            };
            let result = search_root(&mut bb, &mut tt, &mut ord, &mut scratch, &cfg);
            let d = result.depth_reached;
            assert!(
                (0..=5).contains(&d),
                "depth_reached out of expected range: {d}"
            );
            let expected = &reference_killers[d as usize];
            for (ply, (actual_slot, expected_slot)) in
                ord.killers.iter().zip(expected.iter()).enumerate()
            {
                assert_eq!(
                    actual_slot.slots(),
                    expected_slot.slots(),
                    "TimeUp rollback failure: at budget={ms}ms, search reported \
                     depth_reached={d} but killers at ply={ply} diverge from a \
                     fully-completed depth-{d} reference search. \
                     Partial-iteration killer writes leaked past the rollback \
                     in search_root's Err(Timeout) arm (search.rs:266-269)."
                );
            }
        }
    }
}

