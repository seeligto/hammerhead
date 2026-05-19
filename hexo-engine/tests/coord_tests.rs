use hexo_engine_core::config::MAX_PIECE_DISTANCE;
use hexo_engine_core::coords::{
    AXES, AXIS_Q, AXIS_R, AXIS_S, Coord, ORIGIN, RANGE_OFFSET_COUNT, RANGE_OFFSETS,
    for_each_in_range, hex_distance, within_range,
};
use std::collections::HashSet;

#[test]
fn distance_symmetry() {
    let pts = [
        Coord::new(0, 0),
        Coord::new(3, -2),
        Coord::new(-5, 1),
        Coord::new(8, 8),
        Coord::new(-7, 4),
    ];
    for &a in &pts {
        for &b in &pts {
            assert_eq!(hex_distance(a, b), hex_distance(b, a));
        }
    }
}

#[test]
fn distance_unit_axes() {
    for axis in AXES {
        assert_eq!(hex_distance(ORIGIN, axis), 1);
    }
}

#[test]
fn distance_along_axis_q() {
    assert_eq!(hex_distance(ORIGIN, Coord::new(8, 0)), 8);
    assert_eq!(hex_distance(Coord::new(0, 0), Coord::new(16, 0)), 16);
}

#[test]
fn triangle_inequality_small_grid() {
    let pts: Vec<Coord> = (-3..=3)
        .flat_map(|q| (-3..=3).map(move |r| Coord::new(q, r)))
        .collect();
    for &a in &pts {
        for &b in &pts {
            for &c in &pts {
                assert!(hex_distance(a, c) <= hex_distance(a, b) + hex_distance(b, c));
            }
        }
    }
}

#[test]
fn cube_invariant() {
    let pts = [
        Coord::new(0, 0),
        Coord::new(3, -2),
        Coord::new(-5, 1),
        Coord::new(8, 8),
        Coord::new(-7, 4),
    ];
    for p in pts {
        assert_eq!(p.q + p.r + p.s(), 0);
    }
}

#[test]
fn axis_definitions() {
    assert_eq!(AXIS_Q, Coord::new(1, 0));
    assert_eq!(AXIS_R, Coord::new(0, 1));
    assert_eq!(AXIS_S, Coord::new(1, -1));
}

#[test]
fn range_offsets_length() {
    let r = MAX_PIECE_DISTANCE as usize;
    assert_eq!(RANGE_OFFSETS.len(), 3 * r * (r + 1));
    assert_eq!(RANGE_OFFSET_COUNT, RANGE_OFFSETS.len());
}

#[test]
fn range_offsets_distance_bounds() {
    for &d in &RANGE_OFFSETS {
        let dist = hex_distance(ORIGIN, d);
        assert!(dist >= 1, "offset {d:?} has distance 0");
        assert!(
            dist <= MAX_PIECE_DISTANCE,
            "offset {d:?} has distance {dist}"
        );
    }
}

#[test]
fn range_offsets_no_origin() {
    for &d in &RANGE_OFFSETS {
        assert_ne!(d, ORIGIN);
    }
}

#[test]
fn range_offsets_unique() {
    let set: HashSet<Coord> = RANGE_OFFSETS.iter().copied().collect();
    assert_eq!(set.len(), RANGE_OFFSETS.len());
}

#[test]
fn within_range_matches_distance() {
    let pts: Vec<Coord> = (-5..=5)
        .flat_map(|q| (-5..=5).map(move |r| Coord::new(q, r)))
        .collect();
    for &a in &pts {
        for &b in &pts {
            for range in 0..=10 {
                assert_eq!(within_range(a, b, range), hex_distance(a, b) <= range);
            }
        }
    }
}

#[test]
fn for_each_in_range_inclusive_of_center() {
    let mut count = 0usize;
    let mut saw_center = false;
    for_each_in_range(Coord::new(3, -1), 2, |c| {
        if c == Coord::new(3, -1) {
            saw_center = true;
        }
        assert!(hex_distance(c, Coord::new(3, -1)) <= 2);
        count += 1;
    });
    assert!(saw_center);
    // Hex of radius 2: 1 + 3*2*3 = 19 cells.
    assert_eq!(count, 19);
}

#[test]
fn for_each_in_range_matches_range_offsets() {
    let center = Coord::new(0, 0);
    let mut walked: HashSet<Coord> = HashSet::new();
    for_each_in_range(center, MAX_PIECE_DISTANCE, |c| {
        walked.insert(c);
    });
    // Expected: origin + RANGE_OFFSETS.
    let mut expected: HashSet<Coord> = RANGE_OFFSETS.iter().copied().collect();
    expected.insert(ORIGIN);
    assert_eq!(walked, expected);
}

#[test]
fn coord_arithmetic() {
    let a = Coord::new(3, -1);
    let b = Coord::new(2, 4);
    assert_eq!(a.add(b), Coord::new(5, 3));
    assert_eq!(a.sub(b), Coord::new(1, -5));
    assert_eq!(a.add(b).sub(b), a);
}

#[test]
fn coord_repr_packs_to_32_bits() {
    assert_eq!(std::mem::size_of::<Coord>(), 4);
}

#[test]
fn axis_q_const_is_axis_q() {
    let _ = AXIS_Q;
    let _ = AXIS_R;
    let _ = AXIS_S;
}
