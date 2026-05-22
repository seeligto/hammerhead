//! Phase 26.5 feature-gated ordering / search counters.
//!
//! When the `ordering_stats` feature is off, every public fn is an
//! `#[inline]` no-op so callers in `search.rs` stay branchless.
//! When on, increments hit a single set of `AtomicU64`s with
//! `Ordering::Relaxed` — engine is single-threaded per process, but
//! atomics avoid `UnsafeCell` boilerplate. Dumped to stderr at the end
//! of each `search_root` call.

#[cfg(feature = "ordering_stats")]
mod inner {
    use std::sync::atomic::{AtomicU64, Ordering};

    pub const N_BUCKETS: usize = 11; // bucket_value range 0..=10
    pub const N_KILLER_SLOTS: usize = 2; // mirrors KILLER_SLOTS at the engine level
    pub const N_DEPTHS: usize = 32; // truncated histogram bins

    pub struct SearchStats {
        pub stage1_tried: AtomicU64,
        pub stage1_cut: AtomicU64,
        pub stage2_tried: AtomicU64,
        pub stage2_cut: AtomicU64,
        pub stage3_tried: AtomicU64,
        pub stage3_cut: AtomicU64,
        pub bucket_tried: [AtomicU64; N_BUCKETS],
        pub bucket_cut: [AtomicU64; N_BUCKETS],
        pub killer_slot_tried: [AtomicU64; N_KILLER_SLOTS],
        pub killer_slot_cut: [AtomicU64; N_KILLER_SLOTS],
        pub lmr_fired: AtomicU64,
        pub lmr_research: AtomicU64,
        pub lmr_research_full: AtomicU64,
        pub asp_iter: AtomicU64,
        pub asp_fail_low: AtomicU64,
        pub asp_fail_high: AtomicU64,
        pub asp_full_window: AtomicU64,
        pub cutoffs_by_depth: [AtomicU64; N_DEPTHS],
    }

    pub static STATS: SearchStats = SearchStats {
        stage1_tried: AtomicU64::new(0),
        stage1_cut: AtomicU64::new(0),
        stage2_tried: AtomicU64::new(0),
        stage2_cut: AtomicU64::new(0),
        stage3_tried: AtomicU64::new(0),
        stage3_cut: AtomicU64::new(0),
        bucket_tried: [const { AtomicU64::new(0) }; N_BUCKETS],
        bucket_cut: [const { AtomicU64::new(0) }; N_BUCKETS],
        killer_slot_tried: [const { AtomicU64::new(0) }; N_KILLER_SLOTS],
        killer_slot_cut: [const { AtomicU64::new(0) }; N_KILLER_SLOTS],
        lmr_fired: AtomicU64::new(0),
        lmr_research: AtomicU64::new(0),
        lmr_research_full: AtomicU64::new(0),
        asp_iter: AtomicU64::new(0),
        asp_fail_low: AtomicU64::new(0),
        asp_fail_high: AtomicU64::new(0),
        asp_full_window: AtomicU64::new(0),
        cutoffs_by_depth: [const { AtomicU64::new(0) }; N_DEPTHS],
    };

    #[inline]
    pub fn bump(a: &AtomicU64) {
        a.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn bidx(b: u8) -> usize {
        (b as usize).min(N_BUCKETS - 1)
    }

    #[inline]
    pub fn didx(d: i8) -> usize {
        let clamped = u8::try_from(d).unwrap_or(0);
        usize::from(clamped).min(N_DEPTHS - 1)
    }

    pub fn reset() {
        let s = &STATS;
        let single = [
            &s.stage1_tried, &s.stage1_cut, &s.stage2_tried, &s.stage2_cut,
            &s.stage3_tried, &s.stage3_cut, &s.lmr_fired, &s.lmr_research,
            &s.lmr_research_full, &s.asp_iter, &s.asp_fail_low,
            &s.asp_fail_high, &s.asp_full_window,
        ];
        for a in single {
            a.store(0, Ordering::Relaxed);
        }
        for a in s.bucket_tried.iter().chain(s.bucket_cut.iter()) {
            a.store(0, Ordering::Relaxed);
        }
        for a in s
            .killer_slot_tried
            .iter()
            .chain(s.killer_slot_cut.iter())
        {
            a.store(0, Ordering::Relaxed);
        }
        for a in &s.cutoffs_by_depth {
            a.store(0, Ordering::Relaxed);
        }
    }

    pub fn dump_stderr() {
        let s = &STATS;
        let g = |a: &AtomicU64| a.load(Ordering::Relaxed);
        eprintln!("ORDERING_STATS:");
        eprintln!(
            "  stage1 (TT):     tried={} cut={}",
            g(&s.stage1_tried),
            g(&s.stage1_cut)
        );
        eprintln!(
            "  stage2 (killer): tried={} cut={}",
            g(&s.stage2_tried),
            g(&s.stage2_cut)
        );
        eprintln!(
            "  stage3 (bucket): tried={} cut={}",
            g(&s.stage3_tried),
            g(&s.stage3_cut)
        );
        eprint!("  bucket_tried:");
        for (i, a) in s.bucket_tried.iter().enumerate() {
            eprint!(" b{i}={}", g(a));
        }
        eprintln!();
        eprint!("  bucket_cut:  ");
        for (i, a) in s.bucket_cut.iter().enumerate() {
            eprint!(" b{i}={}", g(a));
        }
        eprintln!();
        eprint!("  killer_slot_tried:");
        for (i, a) in s.killer_slot_tried.iter().enumerate() {
            eprint!(" k{i}={}", g(a));
        }
        eprintln!();
        eprint!("  killer_slot_cut:  ");
        for (i, a) in s.killer_slot_cut.iter().enumerate() {
            eprint!(" k{i}={}", g(a));
        }
        eprintln!();
        eprintln!(
            "  lmr: fired={} research={} research_full={}",
            g(&s.lmr_fired),
            g(&s.lmr_research),
            g(&s.lmr_research_full)
        );
        eprintln!(
            "  asp: iter={} fail_low={} fail_high={} full_window={}",
            g(&s.asp_iter),
            g(&s.asp_fail_low),
            g(&s.asp_fail_high),
            g(&s.asp_full_window)
        );
        eprint!("  cutoffs_by_depth:");
        for (i, a) in s.cutoffs_by_depth.iter().enumerate() {
            let v = g(a);
            if v > 0 {
                eprint!(" d{i}={v}");
            }
        }
        eprintln!();
    }
}

// ── Public API (always present; no-op when feature is off). ────────────────

#[cfg(feature = "ordering_stats")]
#[inline]
pub fn note_stage1_tried() {
    inner::bump(&inner::STATS.stage1_tried);
}
#[cfg(feature = "ordering_stats")]
#[inline]
pub fn note_stage2_tried(slot_idx: usize) {
    inner::bump(&inner::STATS.stage2_tried);
    if let Some(a) = inner::STATS.killer_slot_tried.get(slot_idx) {
        inner::bump(a);
    }
}
#[cfg(feature = "ordering_stats")]
#[inline]
pub fn note_stage3_tried() {
    inner::bump(&inner::STATS.stage3_tried);
}
#[cfg(feature = "ordering_stats")]
#[inline]
pub fn note_bucket_tried(bucket: u8) {
    inner::bump(&inner::STATS.bucket_tried[inner::bidx(bucket)]);
}
#[cfg(feature = "ordering_stats")]
#[inline]
pub fn note_cut(stage: u8, bucket: u8, depth: i8, killer_slot: Option<usize>) {
    let s = &inner::STATS;
    match stage {
        1 => inner::bump(&s.stage1_cut),
        2 => inner::bump(&s.stage2_cut),
        _ => inner::bump(&s.stage3_cut),
    }
    inner::bump(&s.bucket_cut[inner::bidx(bucket)]);
    if let Some(k) = killer_slot
        && let Some(a) = s.killer_slot_cut.get(k)
    {
        inner::bump(a);
    }
    inner::bump(&s.cutoffs_by_depth[inner::didx(depth)]);
}
#[cfg(feature = "ordering_stats")]
#[inline]
pub fn note_lmr_fired() {
    inner::bump(&inner::STATS.lmr_fired);
}
#[cfg(feature = "ordering_stats")]
#[inline]
pub fn note_lmr_research() {
    inner::bump(&inner::STATS.lmr_research);
}
#[cfg(feature = "ordering_stats")]
#[inline]
pub fn note_lmr_research_full() {
    inner::bump(&inner::STATS.lmr_research_full);
}
#[cfg(feature = "ordering_stats")]
#[inline]
pub fn note_asp_iter(fail_low: bool, fail_high: bool, at_full_window: bool) {
    let s = &inner::STATS;
    inner::bump(&s.asp_iter);
    if fail_low {
        inner::bump(&s.asp_fail_low);
    }
    if fail_high {
        inner::bump(&s.asp_fail_high);
    }
    if at_full_window {
        inner::bump(&s.asp_full_window);
    }
}
#[cfg(feature = "ordering_stats")]
pub fn reset() {
    inner::reset();
}
#[cfg(feature = "ordering_stats")]
pub fn dump_stderr() {
    inner::dump_stderr();
}

// No-op stubs when the feature is off — every call compiles out.
#[cfg(not(feature = "ordering_stats"))]
#[inline(always)]
pub fn note_stage1_tried() {}
#[cfg(not(feature = "ordering_stats"))]
#[inline(always)]
pub fn note_stage2_tried(_slot_idx: usize) {}
#[cfg(not(feature = "ordering_stats"))]
#[inline(always)]
pub fn note_stage3_tried() {}
#[cfg(not(feature = "ordering_stats"))]
#[inline(always)]
pub fn note_bucket_tried(_bucket: u8) {}
#[cfg(not(feature = "ordering_stats"))]
#[inline(always)]
pub fn note_cut(_stage: u8, _bucket: u8, _depth: i8, _killer_slot: Option<usize>) {}
#[cfg(not(feature = "ordering_stats"))]
#[inline(always)]
pub fn note_lmr_fired() {}
#[cfg(not(feature = "ordering_stats"))]
#[inline(always)]
pub fn note_lmr_research() {}
#[cfg(not(feature = "ordering_stats"))]
#[inline(always)]
pub fn note_lmr_research_full() {}
#[cfg(not(feature = "ordering_stats"))]
#[inline(always)]
pub fn note_asp_iter(_fail_low: bool, _fail_high: bool, _at_full_window: bool) {}
#[cfg(not(feature = "ordering_stats"))]
#[inline(always)]
pub fn reset() {}
#[cfg(not(feature = "ordering_stats"))]
#[inline(always)]
pub fn dump_stderr() {}
