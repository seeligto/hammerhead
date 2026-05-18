//! Move ordering for alpha-beta. Bucket-sorted by tactical priority.
//!
//! Stateful: killers (per-ply) and history (global) live in
//! [`OrderingState`], owned by the search driver. Buckets and encoding —
//! see `SPEC_ENGINE.md` "Ordering". Approximate predicates for
//! `creates_s0` / `creates_s1` are documented inline.

use crate::axis_bitmap::Axis;
use crate::board::{Board, Player};
use crate::config::{
    HISTORY_CUTOFF_MAX, HISTORY_DECAY_DEN, HISTORY_DECAY_NUM, KILLER_SLOTS, MAX_PLY, MOVE_GEN_CAP,
};
use crate::coords::Coord;
use crate::moves::MoveList;
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

/// Pack a `(bucket, history)` pair into a sortable `u32`. Bucket bits are
/// the high 8; history occupies the low 24. Higher value = sorted earlier.
#[inline]
#[must_use]
fn priority(bucket: u8, history: u32) -> u32 {
    (u32::from(bucket) << 24) | (history & HISTORY_CUTOFF_MAX)
}

/// Decide the bucket encoding value for `m`. See `SPEC_ENGINE.md`
/// "Ordering" for the full table. First match wins; encoding values
/// correspond to spec buckets 1..=9 mapped to 10..=1 (with a gap at
/// `bucket == 2` and 0 reserved for the unused tie-break term).
#[inline]
fn bucket_value(ctx: &OrderingContext, m: Coord) -> u8 {
    if ctx.tt_move == Some(m) {
        return 10;
    }
    if would_make_six(ctx.board, m, ctx.side) {
        return 9;
    }
    if would_make_six(ctx.board, m, ctx.side.opponent()) {
        return 8;
    }
    if !ctx.stone1_s0_defense.is_empty() && ctx.stone1_s0_defense.contains(&m) {
        return 7;
    }
    if creates_s0(ctx.board, m, ctx.side) {
        return 6;
    }
    if blocks_opp_s0(ctx.board, m, ctx.side) {
        return 5;
    }
    if creates_s1(ctx.board, m, ctx.side) {
        return 4;
    }
    if ctx.killers.contains(m) {
        return 3;
    }
    // Spec bucket 9 (history-only). Encoding value 2 is reserved (gap);
    // encoding value 0 would mean "history below the high-byte fence",
    // never emitted in v1.
    1
}

/// Virtual-place predicate: would placing `side` at the empty cell `m`
/// produce a run of length ≥ 6 on any axis through `m`? Cheap — only
/// inspects the existing axis bitmap, never mutates the board. The `≥`
/// (not `==`) matches the `HeXO` rule that overlines also win.
#[inline]
fn would_make_six(board: &Board, m: Coord, side: Player) -> bool {
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
fn creates_s0(board: &Board, m: Coord, side: Player) -> bool {
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
fn blocks_opp_s0(board: &Board, m: Coord, side: Player) -> bool {
    let opp_threats = board.threats(side.opponent());
    opp_threats
        .s0_instances
        .iter()
        .any(|inst| inst.defense_cells.contains(&m))
}

/// Approximation of "creates an S1 threat for `side`" — see
/// `SPEC_ENGINE.md` "Ordering — Approximations". Fires when the virtual
/// placement extends an own axis run to length ≥ 3. Catches open-3
/// directly and most rhombus / arch / trapezoid / bone extensions whose
/// added stone is collinear with two existing stones; pure non-collinear
/// shapes are bucket-7 noise per spec.
fn creates_s1(board: &Board, m: Coord, side: Player) -> bool {
    for axis in Axis::all() {
        if axis_run_through_empty(board, m, axis, side) >= 3 {
            return true;
        }
    }
    false
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
/// truncate to [`MOVE_GEN_CAP`]. Scratch buffer is a `SmallVec` inline up
/// to `MOVE_GEN_CAP_INLINE` items — no heap allocation in the typical case.
pub fn order_moves(moves: &mut MoveList, ctx: &OrderingContext<'_>) {
    if moves.is_empty() {
        return;
    }

    let mut scored: smallvec::SmallVec<[(u32, Coord); crate::moves::MOVE_GEN_CAP_INLINE]> =
        smallvec::SmallVec::with_capacity(moves.len());
    for &m in moves.iter() {
        let bucket = bucket_value(ctx, m);
        let h = ctx.history.get(&(m, ctx.side)).copied().unwrap_or(0);
        scored.push((priority(bucket, h), m));
    }

    // `sort_by` is stable in std; preserves insertion order on ties.
    scored.sort_by(|a, b| b.0.cmp(&a.0));

    moves.clear();
    for (_, m) in scored.iter().take(MOVE_GEN_CAP) {
        moves.push(*m);
    }
}
