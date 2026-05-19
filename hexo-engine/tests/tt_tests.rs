//! Phase 6 TT tests. Two-bucket replacement, generation aging,
//! collision verification.

#![allow(clippy::cast_possible_truncation)]

use hexo_engine_core::coords::ORIGIN;
use hexo_engine_core::{Coord, TTFlag, TranspositionTable};

/// 1) `new(64)` rounds to a power-of-two slot count whose mask is
///    `n_slots - 1`. A zero-MB request still yields a 1-slot table.
#[test]
fn constructor_sizing() {
    let tt = TranspositionTable::new(64);
    let n = tt.slot_count();
    assert!(n.is_power_of_two(), "n_slots {n} not a power of two");
    assert_eq!(tt.mask(), n - 1);

    let zero = TranspositionTable::new(0);
    assert_eq!(zero.slot_count(), 1);
    assert_eq!(zero.mask(), 0);
}

/// 2) Probing a fresh table returns `None` for any hash.
#[test]
fn empty_probe_returns_none() {
    let tt = TranspositionTable::new(1);
    assert!(tt.probe(0).is_none());
    assert!(tt.probe(0xDEAD_BEEF).is_none());
    assert!(tt.probe(u128::MAX).is_none());
}

/// 3) Round-trip: store then probe the same hash and recover the stored
///    payload byte-for-byte.
#[test]
fn store_and_probe_round_trip() {
    let mut tt = TranspositionTable::new(1);
    let h: u128 = 0x1234_5678_9ABC_DEF0_1122_3344_5566_7788;
    tt.store(h, 5, 42, TTFlag::Exact, Coord::new(1, -1));
    let e = tt.probe(h).expect("entry should be present");
    assert_eq!(e.depth, 5);
    assert_eq!(e.score, 42);
    assert_eq!(e.flag, TTFlag::Exact);
    assert_eq!(e.best_move, Coord::new(1, -1));
    assert_eq!(e.hash, h);
}

/// 4) Index collisions are detected. Two hashes that share the same low
///    bits but differ overall must not return each other's entry.
#[test]
fn index_collision_rejected_by_full_hash_compare() {
    let mut tt = TranspositionTable::new(1);
    let h1: u128 = 0x1111_2222_3333_4444_5555_6666_7777_8888;
    // Force the same low-64 index by flipping only a high bit.
    let h2: u128 = h1 ^ ((1u128) << 100);
    assert_ne!(h1, h2, "hashes must differ");
    assert_eq!(
        (h1 as u64 as usize) & tt.mask(),
        (h2 as u64 as usize) & tt.mask(),
        "index collision required for this test"
    );
    tt.store(h1, 3, 7, TTFlag::Exact, ORIGIN);
    assert!(tt.probe(h2).is_none(), "probe must reject foreign hash");
    assert!(tt.probe(h1).is_some());
}

/// 5) Depth-preferred replacement: a deeper write evicts the shallower
///    occupant of bucket A; the displaced shallow entry migrates into
///    bucket B if it dominates the existing always-replace occupant.
#[test]
fn deeper_store_replaces_depth_preferred() {
    let mut tt = TranspositionTable::new(1);
    let h: u128 = 0xAAAA_BBBB_CCCC_DDDD_EEEE_FFFF_0000_1111;
    tt.store(h, 3, 100, TTFlag::Exact, ORIGIN);
    tt.store(h, 7, 200, TTFlag::Exact, ORIGIN);
    let e = tt.probe(h).expect("present");
    assert_eq!(e.depth, 7, "deeper write must occupy the probed slot");
    assert_eq!(e.score, 200);
}

/// 6) Lower-depth store routes around an occupied depth-preferred slot
///    of a different hash and lands in always-replace.
#[test]
fn lower_depth_store_goes_to_always_replace() {
    let mut tt = TranspositionTable::new(1);
    // Two hashes that collide on the (1-slot) index.
    let h_deep: u128 = 0x1000_0000_0000_0000_0000_0000_0000_0001;
    let h_shallow: u128 = 0x2000_0000_0000_0000_0000_0000_0000_0001;
    tt.store(h_deep, 7, 999, TTFlag::Exact, ORIGIN);
    tt.store(h_shallow, 3, 111, TTFlag::Exact, ORIGIN);

    let deep = tt.probe(h_deep).expect("depth-preferred kept");
    assert_eq!(deep.depth, 7);
    assert_eq!(deep.score, 999);
    let shallow = tt.probe(h_shallow).expect("always-replace populated");
    assert_eq!(shallow.depth, 3);
    assert_eq!(shallow.score, 111);
}

/// 7) Generation aging: a generation mismatch makes the depth-preferred
///    slot eligible for replacement regardless of depth.
#[test]
fn new_generation_demotes_old_depth_preferred() {
    let mut tt = TranspositionTable::new(1);
    let h_old: u128 = 0x3333_3333_3333_3333_4444_4444_4444_4444;
    let h_new: u128 = 0x5555_5555_5555_5555_6666_6666_6666_6666;
    tt.store(h_old, 9, 50, TTFlag::Exact, ORIGIN);
    tt.new_generation();
    tt.store(h_new, 3, 25, TTFlag::LowerBound, ORIGIN);

    // The fresh, shallower write must now own the depth-preferred slot.
    let e = tt.probe(h_new).expect("new entry should be reachable");
    assert_eq!(e.depth, 3);
    assert_eq!(e.flag, TTFlag::LowerBound);
    // The aged entry should have been displaced into always-replace
    // (it dominates the previously-empty B slot on depth).
    let aged = tt.probe(h_old).expect("aged entry migrated, not lost");
    assert_eq!(aged.depth, 9);
}

/// 8) Probe order: when both buckets are occupied at distinct hashes,
///    the probe returns the bucket whose hash matches — depth-preferred
///    if it matches, otherwise always-replace.
#[test]
fn probe_returns_matching_bucket() {
    let mut tt = TranspositionTable::new(1);
    let h_a: u128 = 0xAAAA_AAAA_AAAA_AAAA_AAAA_AAAA_AAAA_AAAA;
    let h_b: u128 = 0xBBBB_BBBB_BBBB_BBBB_BBBB_BBBB_BBBB_BBBB;
    tt.store(h_a, 8, 1, TTFlag::Exact, ORIGIN);
    tt.store(h_b, 2, 2, TTFlag::Exact, ORIGIN);
    assert_eq!(tt.probe(h_a).unwrap().score, 1);
    assert_eq!(tt.probe(h_b).unwrap().score, 2);
}

/// 9) `clear` wipes every slot and resets the generation tag.
#[test]
fn clear_drops_every_entry() {
    let mut tt = TranspositionTable::new(1);
    let h: u128 = 0x9999_8888_7777_6666_5555_4444_3333_2222;
    tt.store(h, 4, 17, TTFlag::Exact, ORIGIN);
    tt.new_generation();
    assert_eq!(tt.stats().generation, 1);
    assert!(tt.probe(h).is_some());

    tt.clear();
    assert!(tt.probe(h).is_none());
    assert_eq!(tt.stats().generation, 0);
    assert_eq!(tt.stats().occupied, 0);
}

/// 10) Occupancy reflected in stats: stores raise `occupied` by at
///     most the number of distinct buckets they hit.
#[test]
fn stats_track_bucket_occupancy() {
    let mut tt = TranspositionTable::new(2);
    let n = tt.slot_count();
    assert_eq!(tt.stats().occupied, 0);

    let hashes: [u128; 4] = [
        0x1111_2222_3333_4444_5555_6666_7777_8888,
        0x8888_7777_6666_5555_4444_3333_2222_1111,
        0xCAFE_BABE_DEAD_BEEF_0BAD_F00D_BEEF_CAFE,
        0x0123_4567_89AB_CDEF_FEDC_BA98_7654_3210,
    ];
    let mut buckets: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for &h in &hashes {
        tt.store(h, 1, 0, TTFlag::Exact, ORIGIN);
        buckets.insert((h as u64 as usize) & tt.mask());
    }

    let s = tt.stats();
    assert_eq!(s.n_slots, n);
    assert_eq!(s.generation, 0);
    assert_eq!(
        s.occupied,
        buckets.len(),
        "occupied count must match distinct-bucket count"
    );
}

/// 11) Snapshot columns for the optional `tt_stats` feature must
///     always be present in the struct. Without the feature they read
///     as zero; with it they reflect actual activity AND get cleared
///     on both `new_generation` and `clear`.
#[test]
fn stats_counters_default_zero_and_reset() {
    // Tiny TT so we can craft a same-bucket pair without playing
    // games with `mask`: `new(0)` rounds down to a 1-slot table where
    // every hash collides on the index.
    let mut tt = TranspositionTable::new(0);
    assert_eq!(tt.slot_count(), 1);
    let h: u128 = 0xAAAA_BBBB_CCCC_DDDD_EEEE_FFFF_0000_1111;
    // `other` shares no low-bit relationship with `h`, but with mask=0
    // both still index slot 0.
    let other: u128 = 0x5555_4444_3333_2222_1111_0000_FFFF_EEEE;
    assert_ne!(h, other);
    tt.store(h, 1, 0, TTFlag::Exact, ORIGIN);
    let _ = tt.probe(h);
    let _ = tt.probe(other);

    // Counters are present in the snapshot regardless of feature.
    let s = tt.stats();
    #[cfg(not(feature = "tt_stats"))]
    {
        assert_eq!(s.probes, 0);
        assert_eq!(s.hits, 0);
        assert_eq!(s.stores, 0);
        assert_eq!(s.collisions, 0);
    }
    #[cfg(feature = "tt_stats")]
    {
        assert_eq!(s.stores, 1);
        assert_eq!(s.probes, 2);
        assert_eq!(s.hits, 1);
        // Second probe lands on the same bucket (1-slot table) and
        // finds it non-empty without a hash match → collision.
        assert_eq!(s.collisions, 1);
    }

    tt.new_generation();
    let s2 = tt.stats();
    assert_eq!(s2.probes, 0);
    assert_eq!(s2.hits, 0);
    assert_eq!(s2.stores, 0);
    assert_eq!(s2.collisions, 0);

    // Re-arm a probe under the new generation, then `clear` and confirm
    // counters reset again (along with the buckets).
    let _ = tt.probe(h);
    tt.clear();
    let s3 = tt.stats();
    assert_eq!(s3.probes, 0);
    assert_eq!(s3.hits, 0);
    assert_eq!(s3.stores, 0);
    assert_eq!(s3.collisions, 0);
    assert_eq!(s3.occupied, 0);
    assert_eq!(s3.generation, 0);
}
