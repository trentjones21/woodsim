use std::fmt;

use crate::{Board, Length, Rotation};

/// One of a board's three directions: along its length (with the grain), across its width, or
/// through its thickness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    Length,
    Width,
    Thickness,
}

impl Axis {
    /// How far `board` reaches in this direction.
    pub fn of(self, board: &Board) -> Length {
        match self {
            Axis::Length => board.length(),
            Axis::Width => board.width(),
            Axis::Thickness => board.thickness(),
        }
    }
}

/// How a board, or a glue-up, lies on a saw's table: any of the 24 square ways, flat, on an edge,
/// or on an end, and turned any of four ways; or any way at all, resting on a surface you
/// choose (`Assembly::resting`).
///
/// A saw measures positions on its table `across` it toward the fence, `along` it (from the
/// board's leading end on a table saw, toward the stop on a miter saw), and `up` from the table.
/// As it comes, a board's width runs across, its length along, and its thickness up, with its
/// `t = 0` face down. Tipped, it stands on its `w = 0` edge with its `t = 0` face toward the
/// fence; stood up from there, it stands on its `l = 0` end. Turning is counterclockwise seen
/// from above.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Orientation {
    /// The turn from the board's axes, `l`, `w`, and `t`, onto the saw's: along the table,
    /// across it toward the fence, and up. Exact for the square ways.
    rotation: Rotation,
}

impl Default for Orientation {
    fn default() -> Orientation {
        Orientation {
            rotation: Rotation::NONE,
        }
    }
}

impl fmt::Debug for Orientation {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let way =
            |(axis, backward): (Axis, bool)| format!("{}{axis:?}", if backward { "-" } else { "" });
        let nearly = if self.is_square() { "" } else { "nearly " };
        write!(
            f,
            "Orientation {{ {nearly}along: {}, across: {}, up: {} }}",
            way(self.along()),
            way(self.across()),
            way(self.up())
        )
    }
}

/// The saw's directions, by where they are in a column: along the table, across it toward the
/// fence, and up.
const ALONG: usize = 0;
const ACROSS: usize = 1;
const UP: usize = 2;

impl Orientation {
    /// Lying as `rotation` turns the board's axes onto the saw's, any way at all.
    pub fn from_rotation(rotation: Rotation) -> Orientation {
        Orientation { rotation }
    }

    /// Turned by `turn` in the saw's own frame.
    fn turned_on_saw(self, turn: Rotation) -> Orientation {
        Orientation {
            rotation: self.rotation.then(turn),
        }
    }

    /// Turned by `turn` about the board's own axes, wherever they lie.
    fn turned_on_board(self, turn: Rotation) -> Orientation {
        Orientation {
            rotation: turn.then(self.rotation),
        }
    }

    /// Turned a quarter turn counterclockwise, seen from above.
    pub fn turned_left(self) -> Orientation {
        self.turned_on_saw(Rotation::quarter_turns(UP, 1))
    }

    /// Turned a quarter turn clockwise, seen from above.
    pub fn turned_right(self) -> Orientation {
        self.turned_on_saw(Rotation::quarter_turns(UP, -1))
    }

    /// Turned over, about the direction along the table. A board turned and then flipped lies
    /// as if it had been flipped and then turned the other way.
    pub fn flipped(self) -> Orientation {
        self.turned_on_saw(Rotation::quarter_turns(ALONG, 2))
    }

    /// Rolled a quarter turn about its length: lying flat, stood up on an edge, its top face
    /// tipped toward the operator, away from the fence; or, standing on an edge, laid back down
    /// the way it was. Standing on end, rolling about its length turns it where it stands.
    pub fn tipped(self) -> Orientation {
        let roll = if self.on_edge() { -1 } else { 1 };
        self.turned_on_board(Rotation::quarter_turns(0, roll))
    }

    /// Turned a quarter turn about its thickness, so the edge it stood on becomes an end: stood
    /// on an edge, stood up on an end (its `l = 0` end, from its `w = 0` edge); or, standing on
    /// an end, laid back down onto an edge the same way. Lying flat, it's tipped onto its edge
    /// first.
    pub fn stood(self) -> Orientation {
        match self.up().0 {
            Axis::Width => self.turned_on_board(Rotation::quarter_turns(2, 1)),
            Axis::Length => self.turned_on_board(Rotation::quarter_turns(2, -1)),
            Axis::Thickness => self.tipped().stood(),
        }
    }

    /// Whether it lies one of the 24 square ways, each of its axes along one of the saw's.
    pub fn is_square(self) -> bool {
        self.rotation.is_square()
    }

    /// Whether the board stands on an edge rather than lying on a face or standing on an end.
    pub fn on_edge(self) -> bool {
        self.up().0 == Axis::Width
    }

    /// Whether the board stands on an end, its length straight up.
    pub fn on_end(self) -> bool {
        self.up().0 == Axis::Length
    }

    /// The board's axis that lies across the table, and whether the board's coordinates run
    /// backward along it: from the fence side rather than toward it. Turned out of square, the
    /// axis that lies most nearly across.
    pub fn across(self) -> (Axis, bool) {
        self.axis_on(ACROSS)
    }

    /// The board's axis that lies along the table, and whether the board's coordinates run
    /// backward along it.
    pub fn along(self) -> (Axis, bool) {
        self.axis_on(ALONG)
    }

    /// The board's axis that stands up from the table, and whether the board's coordinates run
    /// backward along it: down from the top.
    pub fn up(self) -> (Axis, bool) {
        self.axis_on(UP)
    }

    /// The turn that lays the board's axes (`l`, `w`, `t`) onto the saw's, as `x`, `y`, and `z`:
    /// along the table, across it toward the fence, and up. The same turn lays a glue-up on a
    /// saw the way this lays a board.
    pub fn rotation(self) -> Rotation {
        self.rotation
    }

    /// The board's axis most nearly along the saw's direction `table`, and whether it runs
    /// backward.
    fn axis_on(self, table: usize) -> (Axis, bool) {
        let columns = self.rotation.columns();
        [Axis::Length, Axis::Width, Axis::Thickness]
            .into_iter()
            .zip(columns)
            .map(|(axis, column)| (axis, column[table]))
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .map(|(axis, on)| (axis, on < 0.0))
            .expect("three axes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every way a board lies: on each of its faces, edges, and ends, turned each way.
    fn all_ways() -> Vec<Orientation> {
        let flat = Orientation::default();
        let mut ways = Vec::new();
        for mut lie in [
            flat,
            flat.tipped(),
            flat.flipped(),
            flat.flipped().tipped(),
            flat.stood(),
            flat.stood().flipped(),
        ] {
            for _ in 0..4 {
                ways.push(lie);
                lie = lie.turned_left();
            }
        }
        ways
    }

    #[test]
    fn a_board_lies_24_ways_each_a_turn_not_a_mirror_image() {
        let ways = all_ways();
        for (i, lie) in ways.iter().enumerate() {
            let [l, w, t] = lie.rotation().columns();
            let cross = [
                l[1] * w[2] - l[2] * w[1],
                l[2] * w[0] - l[0] * w[2],
                l[0] * w[1] - l[1] * w[0],
            ];
            assert_eq!(cross, t, "{lie:?}");
            assert!(!ways[..i].contains(lie), "{lie:?} twice");
        }
        assert_eq!(ways.len(), 24);
        // Lying as it comes, its length runs along the table, its width across, and its
        // thickness up.
        assert_eq!(Orientation::default().rotation(), Rotation::NONE);
    }

    /// Where a board's axis lies on the saw after `rolls` quarter rolls about its length and
    /// then `turns` quarter turns, as lies were worked out before a board could stand on end.
    fn rolled_and_turned(rolls: u8, turns: u8, table: usize) -> (Axis, bool) {
        let roll = |(on, forward): (usize, bool)| match on {
            ACROSS => (UP, forward),
            UP => (ACROSS, !forward),
            _ => (ALONG, forward),
        };
        let turn = |(on, forward): (usize, bool)| match on {
            ALONG => (ACROSS, forward),
            ACROSS => (ALONG, !forward),
            _ => (UP, forward),
        };
        [
            (Axis::Length, ALONG),
            (Axis::Width, ACROSS),
            (Axis::Thickness, UP),
        ]
        .into_iter()
        .find_map(|(axis, start)| {
            let mut placed = (start, true);
            for _ in 0..rolls {
                placed = roll(placed);
            }
            for _ in 0..turns {
                placed = turn(placed);
            }
            (placed.0 == table).then_some((axis, !placed.1))
        })
        .unwrap()
    }

    #[test]
    fn flat_and_on_edge_it_lies_as_rolling_and_turning_always_laid_it() {
        for rolls in 0..4u8 {
            for turns in 0..4u8 {
                let mut lie = Orientation::default();
                for _ in 0..rolls {
                    // Each roll tips the top face toward the operator.
                    lie = lie.turned_on_board(Rotation::quarter_turns(0, 1));
                }
                for _ in 0..turns {
                    lie = lie.turned_left();
                }
                assert_eq!(lie.along(), rolled_and_turned(rolls, turns, ALONG));
                assert_eq!(lie.across(), rolled_and_turned(rolls, turns, ACROSS));
                assert_eq!(lie.up(), rolled_and_turned(rolls, turns, UP));
                // Flipping and tipping agree with how they changed the rolls and turns.
                let flipped = lie.flipped();
                let (rolls_f, turns_f) = ((rolls + 2) % 4, (4 - turns) % 4);
                assert_eq!(
                    flipped.across(),
                    rolled_and_turned(rolls_f, turns_f, ACROSS)
                );
                assert_eq!(flipped.up(), rolled_and_turned(rolls_f, turns_f, UP));
                let tipped = lie.tipped();
                let rolls_t = if rolls % 2 == 1 { rolls - 1 } else { rolls + 1 };
                assert_eq!(tipped.across(), rolled_and_turned(rolls_t, turns, ACROSS));
                assert_eq!(tipped.up(), rolled_and_turned(rolls_t, turns, UP));
            }
        }
    }

    #[test]
    fn stood_up_from_its_edge_it_stands_on_its_end() {
        let on_edge = Orientation::default().tipped();
        let lie = on_edge.stood();
        assert!(lie.on_end());
        assert!(!lie.on_edge());
        // On its `l = 0` end, its face still toward the fence, its width along the table.
        assert_eq!(lie.up(), (Axis::Length, false));
        assert_eq!(lie.across(), (Axis::Thickness, true));
        assert_eq!(lie.along(), (Axis::Width, true));
        // Laid back down, it's on its edge as it was, and from flat it goes up by its edge.
        assert_eq!(lie.stood(), on_edge);
        assert_eq!(Orientation::default().stood(), lie);
        // Flipped, it stands on its other end.
        assert_eq!(lie.flipped().up(), (Axis::Length, true));
    }

    #[test]
    fn four_quarter_turns_come_back_around() {
        let mut lie = Orientation::default();
        for _ in 0..4 {
            lie = lie.turned_left();
        }
        assert_eq!(lie, Orientation::default());
        assert_eq!(
            Orientation::default().turned_left().turned_right(),
            Orientation::default()
        );
    }

    #[test]
    fn flipping_twice_puts_it_back() {
        let lie = Orientation::default().turned_left();
        assert_eq!(lie.flipped().flipped(), lie);
    }

    #[test]
    fn a_quarter_turn_lays_the_length_across_the_table() {
        let lie = Orientation::default().turned_left();
        assert_eq!(lie.across(), (Axis::Length, false));
        // The width now runs along the table, its far edge leading.
        assert_eq!(lie.along(), (Axis::Width, true));
    }

    #[test]
    fn flipping_turns_the_width_around_but_not_the_length() {
        let lie = Orientation::default().flipped();
        assert_eq!(lie.across(), (Axis::Width, true));
        assert_eq!(lie.along(), (Axis::Length, false));
        assert_eq!(lie.up(), (Axis::Thickness, true));
    }

    #[test]
    fn tipped_up_it_stands_on_its_edge_with_its_bottom_face_to_the_fence() {
        let lie = Orientation::default().tipped();
        assert!(lie.on_edge());
        assert_eq!(lie.up(), (Axis::Width, false));
        // Its `t = 0` face is toward the fence, so its thickness counts from that side.
        assert_eq!(lie.across(), (Axis::Thickness, true));
        assert_eq!(lie.along(), (Axis::Length, false));
        assert_eq!(lie.tipped(), Orientation::default());
    }

    #[test]
    fn flipped_on_edge_it_stands_on_its_other_edge() {
        let lie = Orientation::default().tipped().flipped();
        assert_eq!(lie.up(), (Axis::Width, true));
        assert_eq!(lie.across(), (Axis::Thickness, false));
        // Laid down from there, it's face down.
        assert_eq!(lie.tipped(), Orientation::default().flipped());
    }

    #[test]
    fn on_edge_and_turned_its_length_lies_across_the_table() {
        let lie = Orientation::default().tipped().turned_left();
        assert_eq!(lie.across(), (Axis::Length, false));
        assert_eq!(lie.up(), (Axis::Width, false));
        assert_eq!(lie.along().0, Axis::Thickness);
    }

    #[test]
    fn every_way_a_board_lies_puts_one_of_its_axes_each_way() {
        let flat = Orientation::default();
        for rolled in [flat, flat.tipped(), flat.flipped(), flat.flipped().tipped()] {
            let mut lie = rolled;
            for _ in 0..4 {
                let mut axes = [lie.across().0, lie.up().0, lie.along().0].map(|a| a as u8);
                axes.sort();
                assert_eq!(axes, [0, 1, 2], "{lie:?}");
                lie = lie.turned_left();
            }
        }
    }
}
