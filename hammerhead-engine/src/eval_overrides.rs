//! Runtime eval-weight overrides (Phase 28B-1).
//!
//! `EvalOverrides` mirrors the 14 codegen'd scalars in `crate::config`
//! that drive the static eval (4 S0, 7 window-k, 2 extension, 1 fork).
//! `Default` constructs from `crate::config::*` — the build-time codegen
//! is the single source of truth, never hand-written literals
//! (CLAUDE.md "Magic numbers in code = bug").
//!
//! The override surface lets the sweep driver patch eval weights without
//! rebuilding the engine. See `specs/SPEC_EVAL.md` § Layer 1/2/3 and
//! `prompts/PHASE_28B_PROMPT.md` § C "Commit B-1.1".

use crate::config::{
    CLOSED_3_SCORE, CLOSED_4_SCORE, CLOSED_5_SCORE, CLOSED_EXTENSION_FACTOR, FORK_COVER2_BONUS,
    OPEN_2_SCORE, OPEN_3_SCORE, OPEN_4_SCORE, OPEN_5_SCORE, OPEN_EXTENSION_FACTOR,
    WINDOW_K_SCORES,
};

/// Number of entries in the Layer-1 `WINDOW_SCORE_8` ternary table.
/// Matches the codegen'd `static WINDOW_SCORE_8: [i32; 6561]` in
/// `config::generated`.
pub const WINDOW_SCORE_8_LEN: usize = 6561;

/// All runtime-tunable eval scalars. `Copy` — 17 i32s = 68 bytes, fits
/// in two cache lines. Held by value on `Board` (no Box, no alloc).
///
/// Phase 28D-3 D3-INFRA added the S1 trio (`open_3`, `closed_3`,
/// `open_2`) ahead of the per-shape detectors landing in D3-A.X.
/// Defaults are zero (codegen'd from hexo.toml), so the S1 contribution
/// is byte-equivalent to the prior build until both the detector and a
/// non-zero weight are in place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvalOverrides {
    /// Layer 2 S0 (mate-in-one) weights, X-positive per-player.
    pub open_5: i32,
    pub closed_5: i32,
    pub open_4: i32,
    pub closed_4: i32,
    /// Layer 2 S1 (pre-fork) weights. Zero until the matching detector
    /// (D3-A.1 / A.2 / A.3) and weight sweep land.
    pub open_3: i32,
    pub closed_3: i32,
    pub open_2: i32,
    /// Layer 1 window-scan scores indexed by own-stone count `[0..=6]`.
    /// Index 6 mirrors `mate_score` (build.rs enforces equality).
    pub window_k_scores: [i32; 7],
    /// Layer 1 extension factors (folded into `WINDOW_SCORE_8` table).
    pub open_extension_factor: i32,
    pub closed_extension_factor: i32,
    /// Layer 3 minimum-cover-2 fork bonus.
    pub fork_cover2_bonus: i32,
}

impl Default for EvalOverrides {
    /// Constructed from `crate::config::*` — the build.rs-codegen'd
    /// constants. Any drift from those constants is a bug; the test
    /// `default_mirrors_config_constants` is the enforcement.
    #[inline]
    fn default() -> Self {
        Self {
            open_5: OPEN_5_SCORE,
            closed_5: CLOSED_5_SCORE,
            open_4: OPEN_4_SCORE,
            closed_4: CLOSED_4_SCORE,
            open_3: OPEN_3_SCORE,
            closed_3: CLOSED_3_SCORE,
            open_2: OPEN_2_SCORE,
            window_k_scores: WINDOW_K_SCORES,
            open_extension_factor: OPEN_EXTENSION_FACTOR,
            closed_extension_factor: CLOSED_EXTENSION_FACTOR,
            fork_cover2_bonus: FORK_COVER2_BONUS,
        }
    }
}

impl EvalOverrides {
    /// `true` iff the Layer-1 inputs (window-k scores + extension
    /// factors) match `other`. When this returns `true` across two
    /// successive overrides, the runtime `WINDOW_SCORE_8` table need
    /// not be rebuilt.
    #[inline]
    #[must_use]
    pub fn layer1_inputs_eq(&self, other: &Self) -> bool {
        self.window_k_scores == other.window_k_scores
            && self.open_extension_factor == other.open_extension_factor
            && self.closed_extension_factor == other.closed_extension_factor
    }

    /// Build the 6561-entry Layer-1 `WINDOW_SCORE_8` ternary lookup
    /// table for this override. Mirrors `build.rs::emit_window_score_8_table`
    /// byte-for-byte — when called with `Default::default()` the result
    /// equals `crate::config::WINDOW_SCORE_8` (asserted by
    /// `runtime_table_matches_codegen` test).
    ///
    /// Cost: ~6561 iterations of tiny arithmetic, microseconds. Called
    /// at most once per `set_eval_overrides` — amortised across a whole
    /// match, never per node. Marked `#[cold]` so the inliner keeps it
    /// out of hot paths.
    ///
    /// # Panics
    ///
    /// Never in practice: the inner `try_into` from `Box<[i32]>` to
    /// `Box<[i32; WINDOW_SCORE_8_LEN]>` is infallible — the vector is
    /// constructed with exactly `WINDOW_SCORE_8_LEN` entries one line
    /// above. A panic here would mean the standard library's `vec!`
    /// macro miscounted, which is a stdlib invariant violation.
    #[cold]
    #[must_use]
    pub fn build_window_score_8(&self) -> Box<[i32; WINDOW_SCORE_8_LEN]> {
        // Heap-allocate via `vec!` to skip a 26 KB stack temporary
        // (clippy::large_stack_arrays). The `try_into` reseats the
        // length-erased `Box<[i32]>` as a `Box<[i32; 6561]>`; the unwrap
        // is infallible because the vector was constructed with that
        // exact capacity literal.
        let mut table: Box<[i32; WINDOW_SCORE_8_LEN]> = vec![0i32; WINDOW_SCORE_8_LEN]
            .into_boxed_slice()
            .try_into()
            .expect("vec was built with WINDOW_SCORE_8_LEN entries");
        let k = &self.window_k_scores;
        let open_f = self.open_extension_factor;
        let closed_f = self.closed_extension_factor;
        for (idx, slot) in table.iter_mut().enumerate() {
            // Decode the 8 ternary cells (c0..=c7), LSB-first. `idx % 3`
            // ∈ {0, 1, 2}, so the `u8` narrowing is exact.
            let mut cells = [0u8; 8];
            let mut n = idx;
            for c in &mut cells {
                #[allow(clippy::cast_possible_truncation)]
                {
                    *c = (n % 3) as u8;
                }
                n /= 3;
            }
            let (mut x_count, mut o_count) = (0u8, 0u8);
            for &cell in &cells[1..7] {
                match cell {
                    1 => x_count += 1,
                    2 => o_count += 1,
                    _ => {}
                }
            }
            let base = if x_count > 0 && o_count > 0 {
                0
            } else if x_count > 0 {
                k[x_count as usize]
            } else if o_count > 0 {
                -k[o_count as usize]
            } else {
                0
            };
            *slot = if base == 0 {
                0
            } else {
                let (own, opp) = if base > 0 { (1u8, 2u8) } else { (2u8, 1u8) };
                let (c0, c7) = (cells[0], cells[7]);
                let factor = if c0 == own || c7 == own || (c0 == opp && c7 == opp) {
                    0
                } else if c0 == 0 && c7 == 0 {
                    open_f
                } else {
                    closed_f
                };
                base * factor
            };
        }
        table
    }
}

#[cfg(test)]
mod tests {
    use super::{EvalOverrides, WINDOW_SCORE_8_LEN};
    use crate::config::{
        CLOSED_3_SCORE, CLOSED_4_SCORE, CLOSED_5_SCORE, CLOSED_EXTENSION_FACTOR,
        FORK_COVER2_BONUS, OPEN_2_SCORE, OPEN_3_SCORE, OPEN_4_SCORE, OPEN_5_SCORE,
        OPEN_EXTENSION_FACTOR, WINDOW_K_SCORES, WINDOW_SCORE_8,
    };

    /// `Default::default()` MUST equal the live `crate::config::*`
    /// constants field-by-field. Enforces the magic-number rule:
    /// override defaults are derived from hexo.toml via build.rs codegen,
    /// not from hand-written literals. A failure here means someone
    /// hand-coded a value that drifted from hexo.toml.
    #[test]
    fn default_mirrors_config_constants() {
        let d = EvalOverrides::default();
        assert_eq!(d.open_5, OPEN_5_SCORE, "open_5");
        assert_eq!(d.closed_5, CLOSED_5_SCORE, "closed_5");
        assert_eq!(d.open_4, OPEN_4_SCORE, "open_4");
        assert_eq!(d.closed_4, CLOSED_4_SCORE, "closed_4");
        assert_eq!(d.open_3, OPEN_3_SCORE, "open_3");
        assert_eq!(d.closed_3, CLOSED_3_SCORE, "closed_3");
        assert_eq!(d.open_2, OPEN_2_SCORE, "open_2");
        assert_eq!(d.window_k_scores, WINDOW_K_SCORES, "window_k_scores");
        assert_eq!(
            d.open_extension_factor, OPEN_EXTENSION_FACTOR,
            "open_extension_factor"
        );
        assert_eq!(
            d.closed_extension_factor, CLOSED_EXTENSION_FACTOR,
            "closed_extension_factor"
        );
        assert_eq!(d.fork_cover2_bonus, FORK_COVER2_BONUS, "fork_cover2_bonus");
    }

    /// `build_window_score_8` on `Default` overrides must produce a table
    /// byte-identical to the build.rs-emitted `WINDOW_SCORE_8`. Guarantees
    /// the runtime rebuild path matches the codegen path exactly.
    #[test]
    fn runtime_table_matches_codegen() {
        let d = EvalOverrides::default();
        let runtime = d.build_window_score_8();
        assert_eq!(runtime.len(), WINDOW_SCORE_8_LEN);
        for (i, (&r, &c)) in runtime.iter().zip(WINDOW_SCORE_8.iter()).enumerate() {
            assert_eq!(r, c, "WINDOW_SCORE_8 mismatch at index {i}");
        }
    }

    #[test]
    fn layer1_inputs_eq_detects_window_change() {
        let a = EvalOverrides::default();
        let mut b = a;
        b.window_k_scores[5] += 1;
        assert!(!a.layer1_inputs_eq(&b));
    }

    #[test]
    fn layer1_inputs_eq_detects_extension_change() {
        let a = EvalOverrides::default();
        let b = EvalOverrides {
            open_extension_factor: a.open_extension_factor + 1,
            ..a
        };
        assert!(!a.layer1_inputs_eq(&b));
        let c = EvalOverrides {
            closed_extension_factor: a.closed_extension_factor + 1,
            ..a
        };
        assert!(!a.layer1_inputs_eq(&c));
    }

    #[test]
    fn layer1_inputs_eq_ignores_s0_and_fork() {
        let a = EvalOverrides::default();
        let b = EvalOverrides {
            open_5: a.open_5 + 1,
            fork_cover2_bonus: a.fork_cover2_bonus + 1,
            ..a
        };
        assert!(a.layer1_inputs_eq(&b));
    }
}
