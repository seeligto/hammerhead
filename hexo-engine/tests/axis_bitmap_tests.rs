use hexo_engine::axis_bitmap::{Axis, AxisBitmaps};
use hexo_engine::board::Player;
use hexo_engine::coords::Coord;

#[test]
fn axis_line_id_pos_roundtrip() {
    // Stepping along an axis preserves line_id and increments pos by 1.
    let start = Coord::new(3, -1);

    for (axis, step) in [
        (Axis::Q, Coord::new(1, 0)),
        (Axis::R, Coord::new(0, 1)),
        (Axis::S, Coord::new(1, -1)),
    ] {
        for k in 0..6_i16 {
            let c = Coord::new(start.q + step.q * k, start.r + step.r * k);
            assert_eq!(axis.line_id(c), axis.line_id(start), "axis {axis:?} k={k}");
            assert_eq!(axis.pos(c), axis.pos(start) + k, "axis {axis:?} k={k}");
        }
    }
}

#[test]
fn single_set_get() {
    let mut axes = AxisBitmaps::new();
    let c = Coord::new(2, 1);

    assert_eq!(axes.run_length_through(c, Axis::Q, Player::X), 0);
    axes.set(c, Player::X);
    assert_eq!(axes.run_length_through(c, Axis::Q, Player::X), 1);
    assert_eq!(axes.run_length_through(c, Axis::R, Player::X), 1);
    assert_eq!(axes.run_length_through(c, Axis::S, Player::X), 1);

    axes.clear(c, Player::X);
    assert_eq!(axes.run_length_through(c, Axis::Q, Player::X), 0);
    assert_eq!(axes.run_length_through(c, Axis::R, Player::X), 0);
    assert_eq!(axes.run_length_through(c, Axis::S, Player::X), 0);
}

#[test]
fn growth_left_and_right() {
    // Set a wide range of positions on the same line; verify all read back.
    let mut axes = AxisBitmaps::new();
    // Axis Q, line_id r=0. Pos = q ranges from -100 to +100.
    for q in -100i16..=100 {
        axes.set(Coord::new(q, 0), Player::X);
    }
    // Walking ±5 from any interior stone should see 6 consecutive (caps at 5+1+5=11 but
    // run_length_through walks at most 5 each side, so up to 11).
    for q in -90i16..=90 {
        let len = axes.run_length_through(Coord::new(q, 0), Axis::Q, Player::X);
        assert_eq!(len, 11, "interior q={q} len={len}");
    }
}

#[test]
fn run_length_through_isolated() {
    let mut axes = AxisBitmaps::new();
    let c = Coord::new(5, 3);
    axes.set(c, Player::O);
    for axis in Axis::all() {
        assert_eq!(axes.run_length_through(c, axis, Player::O), 1);
        assert_eq!(axes.run_length_through(c, axis, Player::X), 0);
    }
}

#[test]
fn run_length_through_six() {
    let mut axes = AxisBitmaps::new();
    // 6 stones along axis Q: (0..6, 0).
    for q in 0i16..6 {
        axes.set(Coord::new(q, 0), Player::X);
    }
    for q in 0i16..6 {
        let len = axes.run_length_through(Coord::new(q, 0), Axis::Q, Player::X);
        assert_eq!(len, 6, "q={q}");
    }
}

#[test]
fn run_length_through_seven_overline() {
    let mut axes = AxisBitmaps::new();
    for q in 0i16..7 {
        axes.set(Coord::new(q, 0), Player::X);
    }
    // Middle stones see all 7 (run_length_through walks ±5, capped at 5+1+5 = 11, so 7 fits).
    let len = axes.run_length_through(Coord::new(3, 0), Axis::Q, Player::X);
    assert_eq!(len, 7);
}

#[test]
fn run_length_through_other_axis() {
    let mut axes = AxisBitmaps::new();
    // 6 X stones along axis Q: r is constant at 0.
    for q in 0i16..6 {
        axes.set(Coord::new(q, 0), Player::X);
    }
    // Each stone is alone on its axis-R line (line_id = q is unique per stone).
    for q in 0i16..6 {
        let len = axes.run_length_through(Coord::new(q, 0), Axis::R, Player::X);
        assert_eq!(len, 1, "q={q}");
    }
}

#[test]
fn window6_aligned() {
    let mut axes = AxisBitmaps::new();
    // Axis Q, line_id r=0. Set q = 0, 2, 4 → bits 0, 2, 4 within window starting at q=0.
    for q in [0i16, 2, 4] {
        axes.set(Coord::new(q, 0), Player::X);
    }
    let w = axes.window6(Axis::Q, 0, 0, Player::X);
    assert_eq!(w, 0b01_0101);
}

#[test]
fn clear_doesnt_break_neighbors() {
    let mut axes = AxisBitmaps::new();
    for q in 0i16..5 {
        axes.set(Coord::new(q, 0), Player::X);
    }
    axes.clear(Coord::new(2, 0), Player::X);
    for q in [0i16, 1, 3, 4] {
        assert!(
            axes.run_length_through(Coord::new(q, 0), Axis::Q, Player::X) >= 1,
            "q={q}"
        );
    }
    assert_eq!(
        axes.run_length_through(Coord::new(2, 0), Axis::Q, Player::X),
        0
    );
    // Run through q=0 should be 2 (q=0,1; q=2 cleared), through q=3 should be 2 (q=3,4).
    assert_eq!(
        axes.run_length_through(Coord::new(0, 0), Axis::Q, Player::X),
        2
    );
    assert_eq!(
        axes.run_length_through(Coord::new(3, 0), Axis::Q, Player::X),
        2
    );
}
