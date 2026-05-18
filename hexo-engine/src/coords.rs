#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct Coord {
    pub q: i16,
    pub r: i16,
}

impl Coord {
    pub const fn new(q: i16, r: i16) -> Self {
        Self { q, r }
    }
}

pub const AXIS_Q: (i16, i16) = (1, 0);
pub const AXIS_R: (i16, i16) = (0, 1);
pub const AXIS_S: (i16, i16) = (1, -1);

pub const AXES: [(i16, i16); 3] = [AXIS_Q, AXIS_R, AXIS_S];

pub fn hex_distance(a: Coord, b: Coord) -> i16 {
    let dq = a.q - b.q;
    let dr = a.r - b.r;
    (dq.abs() + dr.abs() + (dq + dr).abs()) / 2
}
