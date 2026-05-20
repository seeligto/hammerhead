//! Threat / shape detection per WSC theory (tenderloin345).
//!
//! Produces per-player [`ThreatCounts`] and a list of S0 [`ThreatInstance`]s
//! with defense cells. Cached on [`Board`], recomputed incrementally
//! within `THREAT_RECOMPUTE_RADIUS` of the last change center.
//!
//! Phase 15: the `centers` / `prior` hints select between two paths:
//!
//! - **Full recompute** ([`full_recompute`]): scan every piece-line and
//!   every cross-axis anchor. Used on initial read, on overflow of the
//!   dirty-center accumulator, and as the oracle in the correctness
//!   test (`tests/threats_oracle.rs`).
//! - **Incremental** ([`incremental`]): walk linear runs in identical
//!   order to `full_recompute` (so `s0_instances` iteration order is
//!   preserved — see `subagents/scans/phase15-threats-cache-callers.md`)
//!   but skip cross-axis pattern matching for anchors outside every
//!   dirty cluster (radius `THREAT_CLUSTER_RADIUS`). Each anchor's
//!   prior cross-axis contribution is inherited via
//!   [`ThreatSet::cross_axis_per_piece`].

// `span` is guaranteed to be in `[2, 5]` by the surrounding range check
// before each `as u8` cast in `walk_linear_runs`.
#![allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]

use crate::axis_bitmap::Axis;
use crate::board::{Board, Player};
use crate::config::{MAX_S0_INSTANCES, THREAT_CLUSTER_RADIUS};
use crate::coords::{Coord, hex_distance};
use fxhash::FxHashSet;
use smallvec::SmallVec;

/// Per-player count of every detected shape. All u8 — saturated at 255 by
/// the detection loop (deep enough never to be reached in legal play).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreatCounts {
    /// `_XXXXX_` (both ends empty).
    pub open_5: u8,
    /// `OXXXXX_` or `_XXXXXO` (one end empty).
    pub closed_5: u8,
    /// `_XXXX_` (both ends empty).
    pub open_4: u8,
    /// `OXXXX_` (one end empty + extension space).
    pub closed_4: u8,
    /// `_XXX_` with room to grow to a 6-window.
    pub open_3: u8,
    /// 4-piece hex parallelogram.
    pub rhombus: u8,
    /// 3-piece L-shape (one bend on the hex grid).
    pub arch: u8,
    /// 5-piece bowtie (two triangles sharing an edge).
    pub bone: u8,
    /// 5-piece trapezoid / pentagon.
    pub trapezoid: u8,
    /// 2-piece run isolated from opponent within 2 cells on the same axis.
    pub open_2: u8,
    /// `OXXX_` (one end empty).
    pub closed_3: u8,
    /// 3 mutually-adjacent stones.
    pub triangle: u8,
}

/// Tag of an S0 (mate-in-one-turn) threat. Cross-axis shapes are S1/S2 and
/// are not represented here — they only contribute to [`ThreatCounts`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ThreatKind {
    /// `_XXXXX_` — opponent must play one of two endpoints to deny win.
    OpenFive,
    /// `OXXXXX_` — opponent must play the single empty endpoint.
    ClosedFive,
    /// `_XXXX_` — opponent must play both endpoints to deny.
    OpenFour,
    /// `OXXXX_` — opponent must play the single open extension.
    ClosedFour,
}

/// One detected S0 threat with its participating pieces and the minimal
/// blocker set that denies completion next stone.
#[derive(Clone, Debug)]
pub struct ThreatInstance {
    /// Shape category.
    pub kind: ThreatKind,
    /// Stones forming the run, in axis-order (low pos → high pos).
    pub pieces: SmallVec<[Coord; 5]>,
    /// Cells whose occupation by the opponent denies completion. Size 1 for
    /// closed shapes, size 2 for open shapes.
    pub defense_cells: SmallVec<[Coord; 4]>,
}

/// Phase 15: per-anchor cross-axis contribution. Records how many shapes
/// of each cross-axis type matched at this anchor. Used by the incremental
/// path to inherit unchanged contributions without re-running the pattern
/// matcher. Stored on [`ThreatSet`] in the same order as the recomputed
/// piece list (insertion order).
#[derive(Copy, Clone, Debug, Default)]
pub struct CrossAxisContribution {
    /// `# TRIANGLE_PATTERNS` matches with this anchor.
    pub triangle: u8,
    /// `# ARCH_PATTERNS` matches.
    pub arch: u8,
    /// `# RHOMBUS_PATTERNS` matches.
    pub rhombus: u8,
    /// `# BONE_PATTERNS` matches.
    pub bone: u8,
    /// `# TRAPEZOID_PATTERNS` matches.
    pub trapezoid: u8,
}

/// Per-player threat snapshot. Cheap to clone (counts + small Vec).
///
/// Phase 15: the per-anchor cross-axis breakdown used by the
/// incremental compute path lives on [`ThreatScratch`] (owned by
/// `Board`) rather than here — storing it on `ThreatSet` would bloat
/// the struct and regress every consumer that iterates
/// `s0_instances` or reads `counts`. Search consumers see only
/// `counts` + `s0_instances`.
#[derive(Clone, Debug, Default)]
pub struct ThreatSet {
    /// Shape counts across all detected threats.
    pub counts: ThreatCounts,
    /// S0 threats (mate-in-one-turn). One entry per distinct run.
    pub s0_instances: Vec<ThreatInstance>,
}

/// Reusable scratch buffers for `compute`. Owned by `Board` and reset
/// between calls so the `FxHashSet` `seen` dedup and the per-player
/// pieces `Vec` keep their backing capacity instead of reallocating on
/// every dirty read.
///
/// Phase 15: also carries `cross_axis_x` / `cross_axis_o` — the
/// per-anchor cross-axis contribution from the last compute, per
/// player. These persist across [`ThreatScratch::reset`] (which
/// only clears the per-call buffers) so the incremental path can
/// inherit clean-anchor contributions without cloning prior
/// `ThreatSet`s.
///
/// Cleared (not shrunk) at the start of each `compute_with_scratch`.
#[derive(Debug, Default)]
pub struct ThreatScratch {
    seen: FxHashSet<(Axis, i16, i16)>,
    pieces: Vec<Coord>,
    /// Cross-axis breakdown from the last compute for `Player::X`.
    /// Cleared in `full_recompute`, swapped in `incremental`.
    cross_axis_x: Vec<(Coord, CrossAxisContribution)>,
    /// Cross-axis breakdown from the last compute for `Player::O`.
    cross_axis_o: Vec<(Coord, CrossAxisContribution)>,
    /// Phase 16: alternating back-buffers for the incremental path.
    /// `incremental` swaps `cross_axis_x` ⇄ `cross_axis_x_spare` so the
    /// prior breakdown moves to the spare slot and the recovered spare
    /// (which keeps its capacity) is the fresh write target — no
    /// realloc. The old `mem::take` left a cap-0 `Vec` and re-grew it
    /// on every reconcile.
    cross_axis_x_spare: Vec<(Coord, CrossAxisContribution)>,
    /// Back-buffer for `cross_axis_o`. See [`Self::cross_axis_x_spare`].
    cross_axis_o_spare: Vec<(Coord, CrossAxisContribution)>,
}

impl ThreatScratch {
    /// Per-call reset: clears the per-call buffers (`seen` dedup and
    /// `pieces` work list). Does **not** touch the cross-axis
    /// breakdowns — those persist as the inherited prior for the
    /// incremental path.
    #[inline]
    fn reset(&mut self) {
        self.seen.clear();
        self.pieces.clear();
    }

    /// Full reset: clears every internal buffer including the
    /// cross-axis breakdowns. Called from `Board::reset` so a fresh
    /// game starts without stale prior breakdowns from the previous
    /// game (the prior breakdown would index pieces that no longer
    /// exist; incremental would still produce a correct result via
    /// lookup-miss fallback, but pretending no prior exists is
    /// strictly simpler).
    #[inline]
    pub fn clear_all(&mut self) {
        self.seen.clear();
        self.pieces.clear();
        self.cross_axis_x.clear();
        self.cross_axis_o.clear();
        self.cross_axis_x_spare.clear();
        self.cross_axis_o_spare.clear();
    }
}

impl ThreatSet {
    /// `true` iff at least two S0 threats exist and no single cell is in
    /// every threat's `defense_cells`. Conservative: a real fork-mate
    /// requires no 2-cell response covers all threats; this returns `true`
    /// for the simpler "no 1-cell response covers all" — a primitive used by
    /// Phase 5's full fork detector.
    #[must_use]
    pub fn is_mate_pending(&self) -> bool {
        self.s0_instances.len() >= 2 && !single_cell_blocks_all(&self.s0_instances)
    }
}

/// Compute the threat set for `player` on `board`.
///
/// `centers` lists the coords of every `place` / `undo` since the last
/// [`Board::threats`] read; combined with `prior`, the implementation
/// reconciles incrementally over the dirty radius. `centers.is_empty()`
/// OR `prior == None` forces a full recompute.
///
/// Until Phase 15 STEP 2.2 lands the incremental path, this body delegates
/// unconditionally to [`full_recompute`].
///
/// This convenience wrapper allocates a fresh `ThreatScratch` per call.
/// `Board::threats` uses [`compute_with_scratch`] directly so the
/// search hot path reuses backing storage across nodes.
#[must_use]
pub fn compute(
    board: &Board,
    player: Player,
    centers: &[Coord],
    prior: Option<&ThreatSet>,
) -> ThreatSet {
    let mut scratch = ThreatScratch::default();
    compute_with_scratch(board, player, &mut scratch, centers, prior)
}

/// Variant of [`compute`] that reuses caller-provided scratch buffers.
/// `scratch` is reset on entry, so the caller can freely reuse the same
/// buffers across many calls — only the buffers' capacities are
/// retained, eliminating the per-call allocation seen in the
/// flamegraph's threats compute frame.
///
/// Phase 15 dispatch:
///
/// - `centers.is_empty() || prior.is_none()` → [`full_recompute`].
///   Initial reads (`prior == None`) and overflow-fallback reads
///   (`centers == &[]` passed by `Board::reconcile_threats` when the
///   dirty-center accumulator overflowed) take this path.
/// - `scratch.cross_axis_<player>` is empty (i.e. no prior compute
///   has populated the per-anchor breakdown) → also [`full_recompute`].
/// - Otherwise → [`incremental`]. Walks linear runs in identical order
///   to `full_recompute` and inherits cross-axis contributions from
///   the scratch breakdown for anchors outside every dirty cluster.
#[must_use]
pub fn compute_with_scratch(
    board: &Board,
    player: Player,
    scratch: &mut ThreatScratch,
    centers: &[Coord],
    prior: Option<&ThreatSet>,
) -> ThreatSet {
    let breakdown_present = !match player {
        Player::X => scratch.cross_axis_x.is_empty(),
        Player::O => scratch.cross_axis_o.is_empty(),
    };
    let try_incremental = !centers.is_empty() && prior.is_some() && breakdown_present;
    if try_incremental {
        incremental(board, player, scratch, centers)
    } else {
        full_recompute(board, player, scratch)
    }
}

#[cold]
fn full_recompute(board: &Board, player: Player, scratch: &mut ThreatScratch) -> ThreatSet {
    let mut out = ThreatSet::default();
    scratch.reset();
    for (c, p) in board.pieces() {
        if p == player {
            scratch.pieces.push(c);
        }
    }

    walk_linear_runs(board, player, &scratch.pieces, &mut scratch.seen, &mut out);

    // Take the per-player breakdown buffer, clear it, write fresh.
    let breakdown = match player {
        Player::X => &mut scratch.cross_axis_x,
        Player::O => &mut scratch.cross_axis_o,
    };
    breakdown.clear();
    walk_cross_axis_full(board, player, &scratch.pieces, &mut out.counts, breakdown);

    out
}

/// Incremental recompute. Walks linear runs in identical piece-order to
/// [`full_recompute`] so the `s0_instances` iteration order is
/// preserved (load-bearing — see
/// `subagents/scans/phase15-threats-cache-callers.md`: `search.rs:726`
/// `collect_stone1_defense` uses `.find()`). Cross-axis pattern
/// matching runs only for anchors within `THREAT_CLUSTER_RADIUS` of
/// any dirty center; other anchors inherit their contribution from
/// the scratch breakdown, which alternates between two retained
/// buffers (`cross_axis_*` ⇄ `cross_axis_*_spare`) so neither the
/// prior nor the fresh breakdown reallocates.
fn incremental(
    board: &Board,
    player: Player,
    scratch: &mut ThreatScratch,
    centers: &[Coord],
) -> ThreatSet {
    debug_assert!(!centers.is_empty(), "incremental requires non-empty centers");
    let mut out = ThreatSet::default();
    scratch.reset();
    for (c, p) in board.pieces() {
        if p == player {
            scratch.pieces.push(c);
        }
    }

    // Linear walk is identical to full_recompute — preserves iteration
    // order of `s0_instances` and recomputes counts contributions for
    // open_3 / closed_3 / open_2. The cost of this walk is the floor
    // for the incremental path; the cross-axis selective scan below is
    // where the speedup lives.
    walk_linear_runs(board, player, &scratch.pieces, &mut scratch.seen, &mut out);

    // Two-buffer alternation (Phase 16): swap the live breakdown into
    // the spare slot — it becomes the `prior` lookup — then clear the
    // recovered spare (capacity retained) and write the fresh breakdown
    // into it. The pre-Phase-16 `mem::take` left a cap-0 `Vec`, forcing
    // a re-grow on every reconcile.
    match player {
        Player::X => std::mem::swap(&mut scratch.cross_axis_x, &mut scratch.cross_axis_x_spare),
        Player::O => std::mem::swap(&mut scratch.cross_axis_o, &mut scratch.cross_axis_o_spare),
    }
    let (prior_breakdown, breakdown_slot) = match player {
        Player::X => (&scratch.cross_axis_x_spare, &mut scratch.cross_axis_x),
        Player::O => (&scratch.cross_axis_o_spare, &mut scratch.cross_axis_o),
    };
    breakdown_slot.clear();
    walk_cross_axis_incremental(
        board,
        player,
        &scratch.pieces,
        centers,
        prior_breakdown,
        &mut out.counts,
        breakdown_slot,
    );
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Linear shape detection
// ─────────────────────────────────────────────────────────────────────────────

fn walk_linear_runs(
    board: &Board,
    player: Player,
    pieces: &[Coord],
    seen: &mut FxHashSet<(Axis, i16, i16)>,
    out: &mut ThreatSet,
) {
    let axes = board.axes();

    for &c in pieces {
        for axis in Axis::all() {
            let Some((start_pos, end_pos)) = axes.run_endpoints(c, axis, player) else {
                continue;
            };
            let line_id = axis.line_id(c);
            if !seen.insert((axis, line_id, start_pos)) {
                continue;
            }

            let span = end_pos - start_pos + 1;
            if !(2..6).contains(&span) {
                continue;
            }
            let length = span as u8; // span ∈ [2, 5], fits.

            let left_cell = coord_at(axis, line_id, start_pos - 1);
            let right_cell = coord_at(axis, line_id, end_pos + 1);
            let opp = player.opponent();
            let left_open = !axes.is_player(left_cell, opp);
            let right_open = !axes.is_player(right_cell, opp);
            debug_assert!(
                !axes.is_player(left_cell, player),
                "non-maximal run on left"
            );
            debug_assert!(
                !axes.is_player(right_cell, player),
                "non-maximal run on right"
            );

            classify_linear_run(
                board, player, axis, line_id, start_pos, end_pos, length, left_open, right_open,
                out,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn classify_linear_run(
    board: &Board,
    player: Player,
    axis: Axis,
    line_id: i16,
    start_pos: i16,
    end_pos: i16,
    length: u8,
    left_open: bool,
    right_open: bool,
    out: &mut ThreatSet,
) {
    let pieces = run_pieces(axis, line_id, start_pos, end_pos);
    let left_cell = coord_at(axis, line_id, start_pos - 1);
    let right_cell = coord_at(axis, line_id, end_pos + 1);
    let opp = player.opponent();
    let open_ends = u8::from(left_open) + u8::from(right_open);

    match (length, open_ends) {
        (5, 2) => push_s0(
            out,
            ThreatKind::OpenFive,
            pieces,
            smallvec_two(left_cell, right_cell),
            |c| c.open_5 = c.open_5.saturating_add(1),
        ),
        (5, 1) => {
            let def = if left_open { left_cell } else { right_cell };
            push_s0(
                out,
                ThreatKind::ClosedFive,
                pieces,
                smallvec_one(def),
                |c| c.closed_5 = c.closed_5.saturating_add(1),
            );
        }
        (4, 2) => push_s0(
            out,
            ThreatKind::OpenFour,
            pieces,
            smallvec_two(left_cell, right_cell),
            |c| c.open_4 = c.open_4.saturating_add(1),
        ),
        (4, 1) => {
            // Closed-4 needs the cell *beyond* the open neighbour to be
            // non-opp; otherwise extending to 5 produces a fully-boxed run
            // and no 6-in-row is possible.
            let (def_cell, beyond) = if left_open {
                (left_cell, coord_at(axis, line_id, start_pos - 2))
            } else {
                (right_cell, coord_at(axis, line_id, end_pos + 2))
            };
            if !board.axes().is_player(beyond, opp) {
                push_s0(
                    out,
                    ThreatKind::ClosedFour,
                    pieces,
                    smallvec_one(def_cell),
                    |c| c.closed_4 = c.closed_4.saturating_add(1),
                );
            }
        }
        (3, 2) => {
            if has_room_for_six(board, player, axis, line_id, start_pos, end_pos) {
                out.counts.open_3 = out.counts.open_3.saturating_add(1);
            }
        }
        (3, 1) => {
            out.counts.closed_3 = out.counts.closed_3.saturating_add(1);
        }
        (2, 2) => {
            if is_isolated_open_two(board, player, axis, line_id, start_pos) {
                out.counts.open_2 = out.counts.open_2.saturating_add(1);
            }
        }
        _ => {}
    }
}

fn push_s0(
    out: &mut ThreatSet,
    kind: ThreatKind,
    pieces: SmallVec<[Coord; 5]>,
    defense_cells: SmallVec<[Coord; 4]>,
    bump: impl FnOnce(&mut ThreatCounts),
) {
    if out.s0_instances.len() >= MAX_S0_INSTANCES {
        return;
    }
    bump(&mut out.counts);
    out.s0_instances.push(ThreatInstance {
        kind,
        pieces,
        defense_cells,
    });
}

#[inline]
fn smallvec_one(a: Coord) -> SmallVec<[Coord; 4]> {
    let mut v = SmallVec::new();
    v.push(a);
    v
}

#[inline]
fn smallvec_two(a: Coord, b: Coord) -> SmallVec<[Coord; 4]> {
    let mut v = SmallVec::new();
    v.push(a);
    v.push(b);
    v
}

fn run_pieces(axis: Axis, line_id: i16, start: i16, end: i16) -> SmallVec<[Coord; 5]> {
    let mut v = SmallVec::new();
    let mut p = start;
    while p <= end {
        v.push(coord_at(axis, line_id, p));
        p += 1;
    }
    v
}

/// Reconstruct an axis-line cell from its `(line_id, pos)` pair.
#[inline]
fn coord_at(axis: Axis, line_id: i16, pos: i16) -> Coord {
    match axis {
        Axis::Q => Coord::new(pos, line_id),
        Axis::R => Coord::new(line_id, pos),
        Axis::S => Coord::new(pos, line_id - pos),
    }
}

/// For a 3-run `_XXX_` at `[start..=end]`: at least one 6-cell window
/// containing the run is opp-free. The two flank cells `start-1` /
/// `end+1` are already empty (callers ensure `open_ends == 2`), so this
/// reduces to "at least one of the cells 2 beyond the run is not opp".
fn has_room_for_six(
    board: &Board,
    player: Player,
    axis: Axis,
    line_id: i16,
    start: i16,
    end: i16,
) -> bool {
    let beyond_left = coord_at(axis, line_id, start - 2);
    let beyond_right = coord_at(axis, line_id, end + 2);
    let bitmaps = board.axes();
    let opp = player.opponent();
    !bitmaps.is_player(beyond_left, opp) || !bitmaps.is_player(beyond_right, opp)
}

/// Open-2 qualifier: no opponent stone within 2 cells either side along
/// the axis. Run is at `[start..=start+1]`.
fn is_isolated_open_two(
    board: &Board,
    player: Player,
    axis: Axis,
    line_id: i16,
    start: i16,
) -> bool {
    let bitmaps = board.axes();
    let opp = player.opponent();
    for delta in [-2_i16, -1, 2, 3] {
        let c = coord_at(axis, line_id, start + delta);
        if bitmaps.is_player(c, opp) {
            return false;
        }
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Cross-axis shape detection
// ─────────────────────────────────────────────────────────────────────────────

/// 4-piece rhombus patterns: 3 orientations, one per axis-pair. Each entry
/// is the offset list of the other three stones relative to the anchor at
/// `(0,0)`. The anchor is the lex-min stone of the rhombus; offsets are all
/// strictly lex-positive so each rhombus is enumerated exactly once.
#[cfg(feature = "eval_s1s2")]
const RHOMBUS_PATTERNS: &[[(i16, i16); 3]] = &[
    // axes (Q, R): {(0,0), (1,0), (0,1), (1,1)}
    [(1, 0), (0, 1), (1, 1)],
    // axes (Q, S): {(0,0), (1,0), (1,-1), (2,-1)}
    [(1, 0), (1, -1), (2, -1)],
    // axes (R, S): {(0,0), (0,1), (1,-1), (1,0)} — lex-min still (0,0).
    [(0, 1), (1, -1), (1, 0)],
];

/// 3 mutually-adjacent stones. Two orientations (upward / downward).
/// Anchor = lex-min stone.
#[cfg(feature = "eval_s1s2")]
const TRIANGLE_PATTERNS: &[[(i16, i16); 2]] = &[
    // Upward: {(0,0), (1,0), (0,1)}
    [(1, 0), (0, 1)],
    // Downward: {(0,0), (1,0), (1,-1)}
    [(1, 0), (1, -1)],
];

/// 3-piece arches (L-shape): two adjacent pairs, one distance-2 pair.
/// Anchor = lex-min stone. Patterns enumerated by axis-pair / chirality.
#[cfg(feature = "eval_s1s2")]
const ARCH_PATTERNS: &[[(i16, i16); 2]] = &[
    // {(0,0), (1,0), (1,1)}
    [(1, 0), (1, 1)],
    // {(0,0), (1,0), (2,-1)}
    [(1, 0), (2, -1)],
    // {(0,0), (0,1), (-1,2)}
    [(0, 1), (-1, 2)],
    // {(0,0), (1,-1), (2,-1)}
    [(1, -1), (2, -1)],
];

/// 5-piece trapezoid: parallel long-edge pair plus short closing edge.
#[cfg(feature = "eval_s1s2")]
const TRAPEZOID_PATTERNS: &[[(i16, i16); 4]] = &[
    // axes Q-long, R-short:
    [(1, 0), (2, 0), (0, 1), (1, 1)],
    // axes Q-long, S-short:
    [(1, 0), (2, 0), (1, -1), (2, -1)],
    // axes R-long, S-short:
    [(0, 1), (0, 2), (1, -1), (1, 0)],
];

/// 5-piece bone / bowtie: two triangles sharing an edge.
#[cfg(feature = "eval_s1s2")]
const BONE_PATTERNS: &[[(i16, i16); 4]] = &[
    [(1, 0), (0, 1), (-1, 1), (1, -1)],
    [(1, 0), (1, -1), (2, -1), (0, 1)],
    [(0, 1), (1, -1), (1, 0), (-1, 2)],
];

/// Cross-axis pattern matching at `anchor` for `player`. Returns the
/// per-shape match count contributed by this anchor.
#[inline]
fn anchor_cross_axis(board: &Board, player: Player, anchor: Coord) -> CrossAxisContribution {
    // Phase 16: the cross-axis pattern matchers are the bulk of the
    // S1/S2 detection cost. When the `eval_s1s2` feature is off they
    // are skipped entirely and the contribution collapses to zero.
    // See `SPEC_EVAL.md § Layer 2 ablation`.
    #[cfg(not(feature = "eval_s1s2"))]
    {
        let _ = (board, player, anchor);
        CrossAxisContribution::default()
    }
    #[cfg(feature = "eval_s1s2")]
    {
        let mut contrib = CrossAxisContribution::default();
        for pat in TRIANGLE_PATTERNS {
            if matches_pattern(board, player, anchor, pat) {
                contrib.triangle = contrib.triangle.saturating_add(1);
            }
        }
        for pat in ARCH_PATTERNS {
            if matches_pattern(board, player, anchor, pat) {
                contrib.arch = contrib.arch.saturating_add(1);
            }
        }
        for pat in RHOMBUS_PATTERNS {
            if matches_pattern(board, player, anchor, pat) {
                contrib.rhombus = contrib.rhombus.saturating_add(1);
            }
        }
        for pat in BONE_PATTERNS {
            if matches_pattern(board, player, anchor, pat) {
                contrib.bone = contrib.bone.saturating_add(1);
            }
        }
        for pat in TRAPEZOID_PATTERNS {
            if matches_pattern(board, player, anchor, pat) {
                contrib.trapezoid = contrib.trapezoid.saturating_add(1);
            }
        }
        contrib
    }
}

#[inline]
fn add_cross_axis_contrib(counts: &mut ThreatCounts, contrib: CrossAxisContribution) {
    counts.triangle = counts.triangle.saturating_add(contrib.triangle);
    counts.arch = counts.arch.saturating_add(contrib.arch);
    counts.rhombus = counts.rhombus.saturating_add(contrib.rhombus);
    counts.bone = counts.bone.saturating_add(contrib.bone);
    counts.trapezoid = counts.trapezoid.saturating_add(contrib.trapezoid);
}

/// Full cross-axis walk: pattern match at every anchor. Writes each
/// anchor's contribution to `breakdown` for the subsequent
/// incremental path to inherit.
fn walk_cross_axis_full(
    board: &Board,
    player: Player,
    pieces: &[Coord],
    counts: &mut ThreatCounts,
    breakdown: &mut Vec<(Coord, CrossAxisContribution)>,
) {
    breakdown.reserve(pieces.len());
    for &anchor in pieces {
        let contrib = anchor_cross_axis(board, player, anchor);
        add_cross_axis_contrib(counts, contrib);
        breakdown.push((anchor, contrib));
    }
}

/// Incremental cross-axis walk: per anchor, if it falls within
/// `THREAT_CLUSTER_RADIUS` of any dirty center the cross-axis pattern
/// match is run fresh; otherwise the prior contribution is inherited
/// (linear scan over `prior_breakdown`). The walk preserves piece
/// insertion order so the resulting `breakdown` stays parallel to
/// the player's piece list.
fn walk_cross_axis_incremental(
    board: &Board,
    player: Player,
    pieces: &[Coord],
    centers: &[Coord],
    prior_breakdown: &[(Coord, CrossAxisContribution)],
    counts: &mut ThreatCounts,
    breakdown: &mut Vec<(Coord, CrossAxisContribution)>,
) {
    breakdown.reserve(pieces.len());
    for &anchor in pieces {
        let dirty = centers
            .iter()
            .any(|&c| hex_distance(anchor, c) <= THREAT_CLUSTER_RADIUS);
        let contrib = if dirty {
            anchor_cross_axis(board, player, anchor)
        } else {
            prior_cross_axis(prior_breakdown, anchor)
                .unwrap_or_else(|| anchor_cross_axis(board, player, anchor))
        };
        add_cross_axis_contrib(counts, contrib);
        breakdown.push((anchor, contrib));
    }
}

/// Linear lookup over a prior breakdown slice. `None` if the anchor is
/// new (no entry — e.g. a stone placed since the last reconcile).
#[inline]
fn prior_cross_axis(
    prior_breakdown: &[(Coord, CrossAxisContribution)],
    anchor: Coord,
) -> Option<CrossAxisContribution> {
    prior_breakdown
        .iter()
        .find(|&&(c, _)| c == anchor)
        .map(|&(_, contrib)| contrib)
}

#[cfg(feature = "eval_s1s2")]
#[inline]
fn matches_pattern<const N: usize>(
    board: &Board,
    player: Player,
    anchor: Coord,
    offsets: &[(i16, i16); N],
) -> bool {
    let axes = board.axes();
    for (dq, dr) in offsets {
        let c = Coord::new(anchor.q + dq, anchor.r + dr);
        if !axes.is_player(c, player) {
            return false;
        }
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Fork primitives
// ─────────────────────────────────────────────────────────────────────────────

/// `true` iff a single cell appears in every instance's `defense_cells`.
/// Empty `insts` returns `true` (vacuously coverable). Used by
/// [`ThreatSet::is_mate_pending`] and by Phase 5's fork-mate scorer.
#[inline]
#[must_use]
pub fn single_cell_blocks_all(insts: &[ThreatInstance]) -> bool {
    let Some(first) = insts.first() else {
        return true;
    };
    'outer: for candidate in &first.defense_cells {
        for inst in &insts[1..] {
            if !inst.defense_cells.contains(candidate) {
                continue 'outer;
            }
        }
        return true;
    }
    false
}
