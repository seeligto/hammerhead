use crate::board::{Board, Player};

#[derive(Copy, Clone, Debug, Default)]
pub struct ThreatCounts {
    pub open_5: u8,
    pub closed_5: u8,
    pub open_4: u8,
    pub closed_4: u8,
    pub open_3: u8,
    pub rhombus: u8,
    pub arch: u8,
    pub bone: u8,
    pub trapezoid: u8,
    pub open_2: u8,
    pub closed_3: u8,
    pub triangle: u8,
}

pub fn detect(_board: &Board, _player: Player) -> ThreatCounts {
    todo!()
}
