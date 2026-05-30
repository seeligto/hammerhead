//! Outcome-trained tiny-net leaf eval with an INCREMENTAL accumulator
//! (NNUE Stage 2, Gate B). Replaces the Stage-1 full-recompute path.
//!
//! The net is trained in Python on human-game OUTCOME labels and exported
//! as float weights; Rust runs integer-or-float inference on the hot path.
//! Mate / fork-mate logic in `eval::eval` still runs first and dominates.
//!
//! ## Features (Gate-A locked: per-axis open-k, D6-aug)
//! Per axis (Q/R/S) and per length-6 window on every populated line, count
//! "open-k" = window holding exactly k stones of one side and 0 of the
//! other (a live k-toward-6 threat). The classification is SYMMETRIC in
//! X/O, so the accumulator stores ABSOLUTE per-axis histograms:
//!   block a = `[X-open1..5, O-open1..5]`   (a in 0..3)  -> `ACC_DIM` = 30
//! plus absolute stone counts `n_x`, `n_o`. The stm-oriented feature vector
//! is ASSEMBLED at eval by choosing block order on the side to move — the
//! chess-NNUE "perspective" trick, which makes the accumulator independent
//! of the per-stone side-to-move flip.
//!
//! ## Incremental accumulator (rides Phase 27)
//! Placing/removing a stone at `c` touches only the 3 lines through it
//! (one per axis). `on_set`/`on_clear` re-scan those lines, diff against a
//! per-line cache (the `LineContrib` pattern, vector-valued), and apply the
//! bounded local delta to `feat`. Hooked into `Board::apply_set` /
//! `apply_clear`, so every place/undo path stays consistent.

// Fixed-point / float casts are inherent to quantised NNUE inference;
// the multi-array index loops are clearest as explicit range loops.
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::needless_range_loop
)]

use crate::axis_bitmap::{Axis, AxisBitmaps, LINE_ID_OFFSET, LINE_ID_RANGE};
use crate::board::Player;
use crate::coords::Coord;

pub const NHID: usize = 16;
pub const MAX_FEAT: usize = 32;
pub const NFEAT_HIST: usize = 12;
pub const NFEAT_PERAXIS: usize = 32;
const NUM_AXES: usize = 3;
/// Per-axis absolute open-k histogram width: 3 axes x (5 X-open + 5 O-open).
pub const ACC_DIM: usize = NUM_AXES * 10;
const WIN: i16 = 6;

/// Which stm-oriented feature vector the net consumes (both derive from
/// the same 30-d absolute accumulator).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureKind {
    /// 12-d: per-axis blocks summed (D6-invariant). Stage-1 set.
    Hist,
    /// 32-d: per-axis blocks kept separate (Gate-A locked, D6-aug).
    PerAxis,
}

/// Trained tiny-net parameters + standardiser. Boxed on the board.
/// Float weights are authoritative; `q*` mirror them for integer inference.
#[derive(Clone, Debug)]
pub struct NnueParams {
    pub kind: FeatureKind,
    pub nfeat: usize,
    pub mean: [f32; MAX_FEAT],
    pub scale: [f32; MAX_FEAT],
    pub w1: [[f32; MAX_FEAT]; NHID], // w1[h][f]
    pub b1: [f32; NHID],
    pub w2: [f32; NHID],
    pub b2: f32,
    /// Multiplies the pre-sigmoid logit to produce the X-positive score.
    pub out_scale: f32,
    /// Quantised mirror (Gate B): int16 first layer + clipped-ReLU.
    pub quant: Option<QuantParams>,
}

/// int16 post-training quantisation (clipped-ReLU), `Stockfish`-style.
/// hidden pre-activation z1 is computed in i32 from int16 weights applied
/// to int16-quantised standardised features, then clipped-ReLU to
/// `[0, QA]`, then the output layer in i32.
#[derive(Clone, Debug)]
pub struct QuantParams {
    /// int16 first-layer weights: w1q = round(w1 * SW1).
    pub w1q: [[i16; MAX_FEAT]; NHID],
    /// first-layer bias in the z1 domain (SX*SW1).
    pub b1q: [i32; NHID],
    /// int16 output weights: w2q = round(w2 * SW2).
    pub w2q: [i16; NHID],
    /// output bias in the logit domain (SX*SW1*SW2).
    pub b2q: i64,
    /// dequant: logit = (i64 acc) / `q_logit`.
    pub q_logit: f32,
}

/// Quantisation scales (post-training, int16). Conservative so the i32
/// hidden accumulator and i64 output accumulator cannot overflow for the
/// 32->16->1 net (max abs `z1_int` ~ 0.5M, max abs `acc` ~ 1e9 << i64).
const SX: f32 = 1024.0;
const SW1: f32 = 512.0;
const SW2: f32 = 512.0;

impl NnueParams {
    /// Pre-sigmoid logit z2 = w2 . relu(W1.((x-mean)/scale)+b1) + b2 (float).
    #[must_use]
    #[inline]
    pub fn forward_logit(&self, x: &[f32; MAX_FEAT]) -> f32 {
        let mut z2 = self.b2;
        let mut std = [0.0f32; MAX_FEAT];
        for f in 0..self.nfeat {
            std[f] = (x[f] - self.mean[f]) / self.scale[f];
        }
        for h in 0..NHID {
            let mut z1 = self.b1[h];
            let row = &self.w1[h];
            for f in 0..self.nfeat {
                z1 += row[f] * std[f];
            }
            if z1 > 0.0 {
                z2 += self.w2[h] * z1; // ReLU
            }
        }
        z2
    }

    /// Integer logit via int16 quantised weights + clipped-ReLU. Falls
    /// back to float when no quant table is installed.
    #[must_use]
    #[inline]
    pub fn forward_logit_q(&self, x: &[f32; MAX_FEAT]) -> f32 {
        let Some(q) = self.quant.as_ref() else {
            return self.forward_logit(x);
        };
        // Quantise standardised features to int16.
        let mut xq = [0i32; MAX_FEAT];
        for f in 0..self.nfeat {
            let s = (x[f] - self.mean[f]) / self.scale[f] * SX;
            xq[f] = s.round().clamp(-32767.0, 32767.0) as i32;
        }
        let mut acc: i64 = q.b2q;
        for h in 0..NHID {
            let mut z1 = q.b1q[h];
            let row = &q.w1q[h];
            for f in 0..self.nfeat {
                z1 += i32::from(row[f]) * xq[f];
            }
            // clipped-ReLU: clamp to [0, CLIP_HI] in the z1 domain.
            let z1c = z1.clamp(0, CLIP_HI);
            acc += i64::from(q.w2q[h]) * i64::from(z1c);
        }
        acc as f32 / q.q_logit
    }

    /// Build the int16 quantised mirror from the float weights
    /// (post-training quantisation). Idempotent.
    pub fn quantize(&mut self) {
        let mut w1q = [[0i16; MAX_FEAT]; NHID];
        let mut b1q = [0i32; NHID];
        let mut w2q = [0i16; NHID];
        let q_z1 = SX * SW1;
        let q_logit = SX * SW1 * SW2;
        for h in 0..NHID {
            for f in 0..self.nfeat {
                w1q[h][f] = (self.w1[h][f] * SW1).round().clamp(-32767.0, 32767.0) as i16;
            }
            b1q[h] = (self.b1[h] * q_z1).round() as i32;
            w2q[h] = (self.w2[h] * SW2).round().clamp(-32767.0, 32767.0) as i16;
        }
        let b2q = f64::from(self.b2 * q_logit).round() as i64;
        self.quant = Some(QuantParams { w1q, b1q, w2q, b2q, q_logit });
    }
}

/// Clipped-ReLU upper bound in the quantised z1 domain. Generous: real
/// pre-activations stay well below this, so clipping is semantically
/// present (`Stockfish` scheme) but inactive at this net size (reported).
const CLIP_HI: i32 = 1 << 26;

/// Flat-array slot for one line's cached 10-vector contribution.
#[inline]
fn cache_base(axis: Axis, line_id: i16) -> usize {
    let line_idx = (line_id - LINE_ID_OFFSET) as usize;
    (axis as usize * LINE_ID_RANGE + line_idx) * 10
}

/// Incremental absolute-feature accumulator. Owns the running per-axis
/// open-k histogram (`feat`), absolute stone counts, and a per-line cache
/// keyed identically to `LineContrib`.
pub struct Accumulator {
    /// Absolute per-axis open-k counts: block a in [a*10, a*10+10).
    feat: [i32; ACC_DIM],
    n_x: i32,
    n_o: i32,
    /// Per-(axis,line) last-known 10-vector contribution; flat, sized like
    /// `LineContrib` x10. Lets `on_set`/`on_clear` apply a bounded diff.
    line_cache: Box<[i16]>,
}

impl Accumulator {
    #[cold]
    #[must_use]
    pub fn new() -> Self {
        Self {
            feat: [0; ACC_DIM],
            n_x: 0,
            n_o: 0,
            line_cache: vec![0i16; NUM_AXES * LINE_ID_RANGE * 10].into_boxed_slice(),
        }
    }

    /// Full recompute from the current bitmaps + stone counts. Used on
    /// install (`set_nnue`) and as the regression oracle baseline.
    #[cold]
    pub fn rebuild(&mut self, bitmaps: &AxisBitmaps, n_x: i32, n_o: i32) {
        self.feat = [0; ACC_DIM];
        self.line_cache.fill(0);
        self.n_x = n_x;
        self.n_o = n_o;
        for axis in Axis::all() {
            let mut seen: smallvec::SmallVec<[i16; 64]> = smallvec::SmallVec::new();
            for id in bitmaps.line_ids(axis, Player::X) {
                seen.push(id);
            }
            for id in bitmaps.line_ids(axis, Player::O) {
                if !seen.contains(&id) {
                    seen.push(id);
                }
            }
            for &line_id in &seen {
                let v = scan_line_openk(bitmaps, axis, line_id);
                let cb = cache_base(axis, line_id);
                let ab = axis as usize * 10;
                for k in 0..10 {
                    self.feat[ab + k] += i32::from(v[k]);
                    self.line_cache[cb + k] = v[k];
                }
            }
        }
    }

    /// Re-scan the line through `c` on `axis` and apply the diff vs cache.
    /// Correct for both set and clear: the current bitmap is authoritative,
    /// and `feat` tracks the current absolute counts.
    #[inline]
    fn touch_line(&mut self, bitmaps: &AxisBitmaps, axis: Axis, line_id: i16) {
        let v = scan_line_openk(bitmaps, axis, line_id);
        let cb = cache_base(axis, line_id);
        let ab = axis as usize * 10;
        for k in 0..10 {
            let old = self.line_cache[cb + k];
            if v[k] != old {
                self.feat[ab + k] += i32::from(v[k]) - i32::from(old);
                self.line_cache[cb + k] = v[k];
            }
        }
    }

    /// Apply a placement at `c` (bitmaps already updated). Bounded: 3 lines.
    #[inline]
    pub fn on_set(&mut self, bitmaps: &AxisBitmaps, c: Coord, player: Player) {
        for axis in Axis::all() {
            self.touch_line(bitmaps, axis, axis.line_id(c));
        }
        match player {
            Player::X => self.n_x += 1,
            Player::O => self.n_o += 1,
        }
    }

    /// Apply a removal at `c` (bitmaps already updated). Bounded: 3 lines.
    #[inline]
    pub fn on_clear(&mut self, bitmaps: &AxisBitmaps, c: Coord, player: Player) {
        for axis in Axis::all() {
            self.touch_line(bitmaps, axis, axis.line_id(c));
        }
        match player {
            Player::X => self.n_x -= 1,
            Player::O => self.n_o -= 1,
        }
    }

    /// Assemble the stm-oriented feature vector the net consumes.
    #[inline]
    #[must_use]
    pub fn assemble(&self, kind: FeatureKind, stm: Player) -> [f32; MAX_FEAT] {
        let mut f = [0.0f32; MAX_FEAT];
        let x_first = stm == Player::X;
        match kind {
            FeatureKind::PerAxis => {
                for a in 0..NUM_AXES {
                    let ab = a * 10;
                    for k in 0..5 {
                        let xo = self.feat[ab + k] as f32;
                        let oo = self.feat[ab + 5 + k] as f32;
                        if x_first {
                            f[ab + k] = xo;
                            f[ab + 5 + k] = oo;
                        } else {
                            f[ab + k] = oo;
                            f[ab + 5 + k] = xo;
                        }
                    }
                }
                let (ns, no) = if x_first { (self.n_x, self.n_o) } else { (self.n_o, self.n_x) };
                f[30] = ns as f32;
                f[31] = no as f32;
            }
            FeatureKind::Hist => {
                let mut xsum = [0i32; 5];
                let mut osum = [0i32; 5];
                for a in 0..NUM_AXES {
                    let ab = a * 10;
                    for k in 0..5 {
                        xsum[k] += self.feat[ab + k];
                        osum[k] += self.feat[ab + 5 + k];
                    }
                }
                for k in 0..5 {
                    let (s, o) = if x_first { (xsum[k], osum[k]) } else { (osum[k], xsum[k]) };
                    f[k] = s as f32;
                    f[5 + k] = o as f32;
                }
                let (ns, no) = if x_first { (self.n_x, self.n_o) } else { (self.n_o, self.n_x) };
                f[10] = ns as f32;
                f[11] = no as f32;
            }
        }
        f
    }

    /// X-positive net eval from the accumulator. `quant` selects the
    /// integer path. Mate clamp keeps it below terminal magnitudes.
    #[inline]
    #[must_use]
    pub fn eval(&self, params: &NnueParams, stm: Player, quant: bool) -> i32 {
        let x = self.assemble(params.kind, stm);
        let logit = if quant { params.forward_logit_q(&x) } else { params.forward_logit(&x) };
        let adv = params.out_scale * logit;
        let x_pos = if stm == Player::X { adv } else { -adv };
        let cap = (crate::eval::MATE_SCORE - 1_000).max(1) as f32;
        x_pos.clamp(-cap, cap) as i32
    }
}

impl Default for Accumulator {
    fn default() -> Self {
        Self::new()
    }
}

/// Open-k histogram (absolute X/O) for one `(axis, line)`. Mirrors
/// `diag_nnue/features2.py` byte-for-byte: slide a length-6 window over
/// `[min_pos - 5, max_pos]`; a window with only-X stones bumps X-open[k],
/// only-O bumps O-open[k], mixed/empty contributes nothing.
#[must_use]
pub fn scan_line_openk(bitmaps: &AxisBitmaps, axis: Axis, line_id: i16) -> [i16; 10] {
    let mut out = [0i16; 10];
    let xl = bitmaps.line(axis, Player::X, line_id);
    let ol = bitmaps.line(axis, Player::O, line_id);
    let xr = xl.and_then(crate::axis_bitmap::LineBitmap::populated_range);
    let or_ = ol.and_then(crate::axis_bitmap::LineBitmap::populated_range);
    let (min_pos, max_pos) = match (xr, or_) {
        (Some((xa, xb)), Some((oa, ob))) => (xa.min(oa), xb.max(ob)),
        (Some(r), None) | (None, Some(r)) => r,
        (None, None) => return out,
    };
    let mut start = min_pos - (WIN - 1);
    while start <= max_pos {
        let x6 = xl.map_or(0, |l| (l.window8(start) & 0x3F).count_ones());
        let o6 = ol.map_or(0, |l| (l.window8(start) & 0x3F).count_ones());
        if o6 == 0 && x6 > 0 {
            out[(x6.min(5) - 1) as usize] += 1;
        } else if x6 == 0 && o6 > 0 {
            out[(5 + o6.min(5) - 1) as usize] += 1;
        }
        start += 1;
    }
    out
}

/// Full-recompute stm-oriented feature vector (the regression oracle and
/// the Python cross-check). Independent of the accumulator path.
#[must_use]
pub fn features_full(cells: &[Coord], players: &[Player], stm: Player, kind: FeatureKind) -> [f32; MAX_FEAT] {
    let mut bitmaps = AxisBitmaps::new();
    let mut n_x = 0i32;
    let mut n_o = 0i32;
    for (i, &c) in cells.iter().enumerate() {
        bitmaps.set(c, players[i]);
        match players[i] {
            Player::X => n_x += 1,
            Player::O => n_o += 1,
        }
    }
    let mut acc = Accumulator::new();
    acc.rebuild(&bitmaps, n_x, n_o);
    acc.assemble(kind, stm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::Coord;

    // Deterministic LCG so tests need no rng dep (Math.random-free anyway).
    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            self.0 >> 16
        }
        fn range(&mut self, n: i64) -> i64 {
            (self.next() % n as u64) as i64
        }
    }

    fn rebuild_oracle(bm: &AxisBitmaps, nx: i32, no: i32, kind: FeatureKind, stm: Player) -> [f32; MAX_FEAT] {
        let mut a = Accumulator::new();
        a.rebuild(bm, nx, no);
        a.assemble(kind, stm)
    }

    /// incremental `on_set`/`on_clear` == full rebuild, after every step of a
    /// random place sequence AND its full unwind. Both feature kinds.
    #[test]
    #[allow(clippy::float_cmp)] // features are integer counts stored as f32
    fn incremental_matches_full_recompute() {
        for kind in [FeatureKind::PerAxis, FeatureKind::Hist] {
            let mut rng = Lcg(0x1234_5678_9abc_def0);
            for _trial in 0..40 {
                let mut bm = AxisBitmaps::new();
                let mut acc = Accumulator::new();
                let mut stack: Vec<(Coord, Player)> = Vec::new();
                let mut nx = 0i32;
                let mut no = 0i32;
                // build up
                let len = 4 + rng.range(24) as usize;
                let mut occupied: std::collections::HashSet<(i16, i16)> = std::collections::HashSet::new();
                while stack.len() < len {
                    let q = (rng.range(21) - 10) as i16;
                    let r = (rng.range(21) - 10) as i16;
                    if !occupied.insert((q, r)) {
                        continue;
                    }
                    let c = Coord { q, r };
                    let player = if rng.range(2) == 0 { Player::X } else { Player::O };
                    bm.set(c, player);
                    acc.on_set(&bm, c, player);
                    match player {
                        Player::X => nx += 1,
                        Player::O => no += 1,
                    }
                    stack.push((c, player));
                    for stm in [Player::X, Player::O] {
                        assert_eq!(
                            acc.assemble(kind, stm),
                            rebuild_oracle(&bm, nx, no, kind, stm),
                            "place step {} kind {:?} stm {:?}",
                            stack.len(),
                            kind,
                            stm
                        );
                    }
                }
                // unwind (place/undo symmetry)
                while let Some((c, player)) = stack.pop() {
                    bm.clear(c, player);
                    acc.on_clear(&bm, c, player);
                    match player {
                        Player::X => nx -= 1,
                        Player::O => no -= 1,
                    }
                    for stm in [Player::X, Player::O] {
                        assert_eq!(
                            acc.assemble(kind, stm),
                            rebuild_oracle(&bm, nx, no, kind, stm),
                            "undo to {} kind {:?}",
                            stack.len(),
                            kind
                        );
                    }
                }
                // fully unwound -> empty feature vector
                assert_eq!(acc.assemble(kind, Player::X), [0.0f32; MAX_FEAT]);
            }
        }
    }

    /// Quantised logit tracks the float logit within a small bound on
    /// realistic feature magnitudes; mate clamp keeps it sub-terminal.
    #[test]
    fn quant_error_bounded() {
        let mut rng = Lcg(0xfeed_face_dead_beef);
        // a small net with plausible weights
        let mut p = NnueParams {
            kind: FeatureKind::PerAxis,
            nfeat: 32,
            mean: [2.0; MAX_FEAT],
            scale: [3.0; MAX_FEAT],
            w1: [[0.0; MAX_FEAT]; NHID],
            b1: [0.0; NHID],
            w2: [0.0; NHID],
            b2: 0.1,
            out_scale: 600.0,
            quant: None,
        };
        for h in 0..NHID {
            for f in 0..32 {
                p.w1[h][f] = ((rng.range(2001) - 1000) as f32) / 1000.0; // [-1,1]
            }
            p.b1[h] = ((rng.range(2001) - 1000) as f32) / 2000.0;
            p.w2[h] = ((rng.range(2001) - 1000) as f32) / 1000.0;
        }
        p.quantize();
        let mut max_err: f32 = 0.0;
        for _ in 0..500 {
            let mut x = [0.0f32; MAX_FEAT];
            for f in 0..32 {
                x[f] = rng.range(12) as f32; // open-k counts ~ 0..11
            }
            let lf = p.forward_logit(&x);
            let lq = p.forward_logit_q(&x);
            max_err = max_err.max((lf - lq).abs());
        }
        assert!(max_err < 0.05, "quant logit error too large: {max_err}");
    }
}
