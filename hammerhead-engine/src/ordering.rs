//! Move ordering for alpha-beta. Bucket-sorted by tactical priority.
//!
//! Stateful: killers (per-ply) and history (global) live in
//! [`OrderingState`], owned by the search driver. Buckets and encoding —
//! see `SPEC_ENGINE.md` "Ordering". The approximate `creates_s0`
//! predicate is documented inline.

use crate::axis_bitmap::Axis;
use crate::board::{Board, Player};
use crate::config::{
    HISTORY_CUTOFF_MAX, HISTORY_DECAY_DEN, HISTORY_DECAY_NUM, KILLER_SLOTS, MAX_PLY, MOVE_GEN_CAP,
};
use crate::coords::Coord;
use fxhash::FxHashMap;

// ────────────────────────────────────────────────────────────────────────
// Killer-move state
// ────────────────────────────────────────────────────────────────────────

/// Most-recent β-cutoff moves at a given ply. Slot 0 is the newest;
/// pushing dedups against every slot. Slot count is [`KILLER_SLOTS`].
#[derive(Clone, Copy, Debug, Default)]
pub struct KillerSlot([Option<Coord>; KILLER_SLOTS]);

impl KillerSlot {
    /// `true` iff `c` already occupies any slot.
    #[inline]
    #[must_use]
    pub fn contains(&self, c: Coord) -> bool {
        self.0.contains(&Some(c))
    }

    /// Insert `c` at the front, shifting older slots one step toward the
    /// back. Dedup: if `c` already occupies any slot, the array is
    /// unchanged.
    #[inline]
    pub fn push(&mut self, c: Coord) {
        if self.contains(c) {
            return;
        }
        for i in (1..KILLER_SLOTS).rev() {
            self.0[i] = self.0[i - 1];
        }
        self.0[0] = Some(c);
    }

    /// Borrow the slot array. Test/inspection only.
    #[inline]
    #[must_use]
    pub fn slots(&self) -> &[Option<Coord>; KILLER_SLOTS] {
        &self.0
    }
}

/// Move-ordering state owned by the search driver. One instance per
/// search; `record_cutoff` updates it during the alpha-beta traversal,
/// `decay_history` ages history scores between root iterations.
pub struct OrderingState {
    /// Killer moves indexed by ply. Boxed to keep the 128-slot array off
    /// the stack.
    pub killers: Box<[KillerSlot; MAX_PLY]>,
    /// Per-`(move, side-to-move)` β-cutoff history score, saturating at
    /// [`HISTORY_CUTOFF_MAX`].
    pub history: FxHashMap<(Coord, Player), u32>,
}

impl Default for OrderingState {
    fn default() -> Self {
        Self::new()
    }
}

impl OrderingState {
    /// Fresh state: zeroed killer slots, empty history map.
    #[must_use]
    pub fn new() -> Self {
        Self {
            killers: Box::new([KillerSlot::default(); MAX_PLY]),
            history: FxHashMap::default(),
        }
    }

    /// Record a β-cutoff at `ply` for move `m` by `p`. Pushes the killer
    /// and bumps `history[(m, p)] += depth²`, saturating at
    /// [`HISTORY_CUTOFF_MAX`]. `depth <= 0` is a no-op on history.
    pub fn record_cutoff(&mut self, ply: u8, m: Coord, p: Player, depth: i8) {
        debug_assert!(depth >= 0, "record_cutoff called with negative depth");
        let idx = (ply as usize).min(MAX_PLY - 1);
        self.killers[idx].push(m);
        let Ok(d) = u8::try_from(depth) else {
            return;
        };
        if d == 0 {
            return;
        }
        let d = u32::from(d);
        let inc = d.saturating_mul(d);
        let slot = self.history.entry((m, p)).or_insert(0);
        *slot = slot.saturating_add(inc).min(HISTORY_CUTOFF_MAX);
    }

    /// Age every history entry by `HISTORY_DECAY_NUM / HISTORY_DECAY_DEN`
    /// (integer floor) and drop entries that floor to zero. Called once
    /// per root iteration; the retain step keeps the map from growing
    /// monotonically.
    pub fn decay_history(&mut self) {
        if HISTORY_DECAY_DEN == 0 || HISTORY_DECAY_NUM >= HISTORY_DECAY_DEN {
            return;
        }
        let num = u64::from(HISTORY_DECAY_NUM);
        let den = u64::from(HISTORY_DECAY_DEN);
        self.history.retain(|_, v| {
            // num < den guarantees the result fits in the original u32.
            let decayed = u64::from(*v) * num / den;
            *v = u32::try_from(decayed).unwrap_or(u32::MAX);
            *v > 0
        });
    }

    /// Wipe killers and history. Used between top-level searches when the
    /// caller wants a deterministic clean slate.
    pub fn clear(&mut self) {
        for k in self.killers.iter_mut() {
            *k = KillerSlot::default();
        }
        self.history.clear();
    }

    /// Zero every killer slot. Called at the start of each `best_move()`
    /// so killers from a prior turn cannot bleed into the next search.
    /// Unlike [`Self::clear`], history is preserved — it is decay-smoothed
    /// across calls by design.
    pub fn reset_killers(&mut self) {
        for k in self.killers.iter_mut() {
            *k = KillerSlot::default();
        }
    }
}

// ────────────────────────────────────────────────────────────────────────
// Per-call context
// ────────────────────────────────────────────────────────────────────────

/// Read-only view of the ordering state needed to score one node's moves.
///
/// `stone1_s0_defense` is non-empty only when `board.halfmove() == 1` and
/// the just-played stone-1 created an own S0 threat — the search driver
/// passes that threat's `defense_cells` so bucket 7 can match
/// "complete-the-threat".
pub struct OrderingContext<'a> {
    /// Position to score moves on. Borrowed read-only — bucket predicates
    /// reason via virtual placements over `board.axes()`.
    pub board: &'a Board,
    /// Side that will play the move being scored. History indexed by
    /// `(move, side)`.
    pub side: Player,
    /// TT-suggested best move at this node, or `None`.
    pub tt_move: Option<Coord>,
    /// Killer slot for the current ply.
    pub killers: &'a KillerSlot,
    /// Global history table from [`OrderingState`].
    pub history: &'a FxHashMap<(Coord, Player), u32>,
    /// Defense cells of the S0 created by stone 1 of the current turn, or
    /// empty when stone-1-completion does not apply.
    pub stone1_s0_defense: &'a [Coord],
}

// ────────────────────────────────────────────────────────────────────────
// Bucket scoring
// ────────────────────────────────────────────────────────────────────────

/// Pack `(bucket, history, m)` into a sortable `u64` total-order key.
/// Bucket occupies bits 56..64; the 24-bit history field occupies bits
/// 32..56; the move's `(q, r)` (i16-bitcast-to-u16 halves) occupies the
/// low 32 bits. Two distinct legal `Coord`s have distinct keys, so the
/// key is total — `select_nth_unstable_by` over this key produces a
/// deterministic top-N selection without relying on sort stability.
/// Higher value = sorted earlier. See `SPEC_ENGINE.md` "Ordering".
#[inline]
#[must_use]
#[allow(clippy::cast_sign_loss)]
fn priority(bucket: u8, history: u32, m: Coord) -> u64 {
    let high = (u64::from(bucket) << 56) | (u64::from(history & HISTORY_CUTOFF_MAX) << 32);
    // Deliberate bitcast i16 → u16 → u64 (zero-extend). The low 32
    // bits make the key total: any two distinct legal Coords have
    // distinct (q, r), so they cannot collide here, and the cast
    // does not affect bucket/history ordering above.
    let q = u64::from(m.q as u16);
    let r = u64::from(m.r as u16);
    high | (q << 16) | r
}

/// Encoding value for `m` per `SPEC_ENGINE.md` "Ordering". First match
/// wins; values 10..=1 (with gaps at 4 and 2 — value 4 was the removed
/// creates-S1 bucket, and 0 is reserved for the unused tie-break term).
/// Higher = sorted earlier.
///
/// Exposed to the search crate so LMR and check-extension decisions can
/// reuse the same predicates without recomputing them.
///
/// Doubles as the bench entry point for ordering micro-benches — they
/// can call this directly to time bucket classification independently
/// of `order_moves`.
#[doc(hidden)]
#[inline]
#[must_use]
pub fn bench_bucket_value(ctx: &OrderingContext, m: Coord) -> u8 {
    bucket_value(ctx, m)
}

#[inline]
#[must_use]
pub(crate) fn bucket_value(ctx: &OrderingContext, m: Coord) -> u8 {
    if ctx.tt_move == Some(m) {
        return 10;
    }
    // Phase 25.5 R-02: fused 3-axis probe. Classifies buckets 9
    // (own ≥6), 8 (opp ≥6), and 6 (own creates_s0) from a single
    // axis-loop, halving `line()` slot loads and tripling-down on
    // `run_*` calls vs the prior three independent passes
    // (would_make_six(side) → would_make_six(opp) → creates_s0(side)).
    // Behaviour-identical to those helpers — see SPEC_ENGINE.md
    // "Fused 3-axis probe in `bucket_value`". The standalone
    // `would_make_six` / `creates_s0` helpers below are retained for
    // qsearch's `is_threat_move` frontier.
    let opp = ctx.side.opponent();
    let mut own_six = false;
    let mut opp_six = false;
    let mut own_s0 = false;
    for axis in Axis::all() {
        let p = axis_probe(ctx.board, m, axis, ctx.side);
        let own_total = 1u8 + p.own_back + p.own_fwd;
        let opp_total = 1u8 + p.opp_back + p.opp_fwd;
        if own_total >= 6 {
            own_six = true;
        }
        if opp_total >= 6 {
            opp_six = true;
        }
        if !own_s0 && (4..=5).contains(&own_total) {
            let id = axis.line_id(m);
            let pos = axis.pos(m);
            let left = coord_on_axis(axis, id, pos - i16::from(p.own_back) - 1);
            let right = coord_on_axis(axis, id, pos + i16::from(p.own_fwd) + 1);
            let left_open = ctx.board.piece_at(left) != Some(opp);
            let right_open = ctx.board.piece_at(right) != Some(opp);
            if left_open || right_open {
                own_s0 = true;
            }
        }
    }
    if own_six {
        return 9;
    }
    if opp_six {
        return 8;
    }
    if !ctx.stone1_s0_defense.is_empty() && ctx.stone1_s0_defense.contains(&m) {
        return 7;
    }
    if own_s0 {
        return 6;
    }
    if blocks_opp_s0(ctx.board, m, ctx.side) {
        return 5;
    }
    // The creates-S1 bucket (encoding value 4) was removed in Phase 20
    // with the rest of S1/S2 detection; a run-extending move falls
    // through to the killer / history buckets.
    if ctx.killers.contains(m) {
        return 3;
    }
    // Spec bucket 9 (history-only). Encoding value 2 is reserved (gap);
    // encoding value 0 would mean "history below the high-byte fence",
    // never emitted in v1.
    1
}

/// Fused per-axis run-length probe through the empty cell `m`. Captures
/// both colours' forward/backward runs in one axis-bitmap visit so
/// `bucket_value` can derive buckets 9 (own ≥6), 8 (opp ≥6), and 6
/// (own `creates_s0`) from a single 3-axis loop. None-line slots return
/// `(0, 0)`; the resulting `total = 1` fails every threshold so the
/// behaviour matches the standalone `would_make_six` / `creates_s0`
/// helpers exactly (see `SPEC_ENGINE.md` "Fused 3-axis probe").
#[derive(Clone, Copy, Default)]
struct AxisProbe {
    own_back: u8,
    own_fwd: u8,
    opp_back: u8,
    opp_fwd: u8,
}

#[inline]
fn axis_probe(board: &Board, m: Coord, axis: Axis, side: Player) -> AxisProbe {
    let opp = side.opponent();
    let id = axis.line_id(m);
    let pos = axis.pos(m);
    let bitmaps = board.axes();
    let (own_back, own_fwd) = match bitmaps.line(axis, side, id) {
        Some(l) => (l.run_backward(pos), l.run_forward(pos)),
        None => (0, 0),
    };
    let (opp_back, opp_fwd) = match bitmaps.line(axis, opp, id) {
        Some(l) => (l.run_backward(pos), l.run_forward(pos)),
        None => (0, 0),
    };
    AxisProbe {
        own_back,
        own_fwd,
        opp_back,
        opp_fwd,
    }
}

/// Virtual-place predicate: would placing `side` at the empty cell `m`
/// produce a run of length ≥ 6 on any axis through `m`? Cheap — only
/// inspects the existing axis bitmap, never mutates the board. The `≥`
/// (not `==`) matches the `HeXO` rule that overlines also win.
#[inline]
#[must_use]
pub(crate) fn would_make_six(board: &Board, m: Coord, side: Player) -> bool {
    for axis in Axis::all() {
        if axis_run_through_empty(board, m, axis, side) >= 6 {
            return true;
        }
    }
    false
}

/// Run length on `axis` through the empty cell `m` for `side`, treating
/// `m` as if it were already set. The cell `m` must be empty — callers
/// guarantee this via [`Board::is_empty_cell`].
#[inline]
fn axis_run_through_empty(board: &Board, m: Coord, axis: Axis, side: Player) -> u8 {
    let id = axis.line_id(m);
    let Some(line) = board.axes().line(axis, side, id) else {
        return 1;
    };
    let pos = axis.pos(m);
    1 + line.run_backward(pos) + line.run_forward(pos)
}

/// Approximation of "creates an S0 threat for `side`" — see
/// `SPEC_ENGINE.md` "Ordering — Approximations". Fires when virtually
/// placing `side` at `m` produces an axis run of length 4 or 5 with at
/// least one non-opp end cell (open or partially-open). False for the
/// 6+ case (handled by the prior `would_make_six` check) and for runs
/// boxed on both flanks (no completion path to 6).
#[must_use]
pub(crate) fn creates_s0(board: &Board, m: Coord, side: Player) -> bool {
    let opp = side.opponent();
    let axes = board.axes();
    for axis in Axis::all() {
        let id = axis.line_id(m);
        let Some(line) = axes.line(axis, side, id) else {
            continue;
        };
        let pos = axis.pos(m);
        let back = line.run_backward(pos);
        let fwd = line.run_forward(pos);
        let total = 1 + back + fwd;
        if !(4..=5).contains(&total) {
            continue;
        }
        let left = coord_on_axis(axis, id, pos - i16::from(back) - 1);
        let right = coord_on_axis(axis, id, pos + i16::from(fwd) + 1);
        let left_open = board.piece_at(left) != Some(opp);
        let right_open = board.piece_at(right) != Some(opp);
        if left_open || right_open {
            return true;
        }
    }
    false
}

/// `m` is in `defense_cells` of some current opponent S0 instance — see
/// `SPEC_ENGINE.md` "Ordering". O(opp S0 count); typically ≤ 2 instances.
#[must_use]
pub(crate) fn blocks_opp_s0(board: &Board, m: Coord, side: Player) -> bool {
    let opp_threats = board.threats(side.opponent());
    opp_threats
        .s0_instances
        .iter()
        .any(|inst| inst.defense_cells.contains(&m))
}

/// Reconstruct an axis-line cell from its `(line_id, pos)` pair. Mirrors
/// the private helper in `threats.rs`; duplicated here to keep ordering
/// self-contained.
#[inline]
fn coord_on_axis(axis: Axis, line_id: i16, pos: i16) -> Coord {
    match axis {
        Axis::Q => Coord::new(pos, line_id),
        Axis::R => Coord::new(line_id, pos),
        Axis::S => Coord::new(pos, line_id - pos),
    }
}

// ────────────────────────────────────────────────────────────────────────
// Public ordering entry point
// ────────────────────────────────────────────────────────────────────────

/// Score every entry in `moves`, stable-sort descending by priority, and
/// truncate to [`MOVE_GEN_CAP`]. Convenience wrapper around
/// [`order_moves_with_buckets`] for callers that don't need the per-move
/// bucket array — not on the search hot path (search threads
/// `SearchScratch` slots directly), so the two local scratch `Vec`s here
/// are acceptable.
pub fn order_moves(moves: &mut Vec<Coord>, ctx: &OrderingContext<'_>) {
    let mut scored: Vec<(u64, u8, Coord)> = Vec::new();
    let mut buckets: Vec<u8> = Vec::new();
    order_moves_with_buckets(moves, ctx, &mut scored, &mut buckets);
}

/// Like [`order_moves`] but writes per-move bucket values into `buckets`,
/// in the same order as the (truncated) `moves` list. Search reuses these
/// for LMR and check-extension decisions, avoiding a second
/// [`bucket_value`] pass per node.
///
/// `scored` and `buckets` are caller-owned scratch — both are cleared on
/// entry, so the caller need not pre-clear them. The search driver wires
/// them to per-ply slots in `SearchScratch` so the underlying allocations
/// amortise across the entire search.
pub(crate) fn order_moves_with_buckets(
    moves: &mut Vec<Coord>,
    ctx: &OrderingContext<'_>,
    scored: &mut Vec<(u64, u8, Coord)>,
    buckets: &mut Vec<u8>,
) {
    scored.clear();
    buckets.clear();
    if moves.is_empty() {
        return;
    }

    scored.reserve(moves.len());
    for &m in moves.iter() {
        let bucket = bucket_value(ctx, m);
        let h = ctx.history.get(&(m, ctx.side)).copied().unwrap_or(0);
        scored.push((priority(bucket, h, m), bucket, m));
    }

    // Partial sort: O(n) partition that places the top MOVE_GEN_CAP
    // entries in indices 0..MOVE_GEN_CAP (unsorted within), then sort
    // the surviving prefix. The priority key is total (Coord low-32
    // tie-break), so this selection is deterministic without
    // requiring sort stability — see `SPEC_ENGINE.md` "Ordering —
    // encoding".
    if scored.len() > MOVE_GEN_CAP {
        scored.select_nth_unstable_by_key(MOVE_GEN_CAP - 1, |s| std::cmp::Reverse(s.0));
        scored.truncate(MOVE_GEN_CAP);
    }
    scored.sort_by_key(|s| std::cmp::Reverse(s.0));

    moves.clear();
    moves.reserve(scored.len());
    buckets.reserve(scored.len());
    for &(_, bucket, m) in scored.iter() {
        moves.push(m);
        buckets.push(bucket);
    }
}
