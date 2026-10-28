use crate::{Board, Direction, Length, Rotation};

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

/// How a board lies on a saw's table: rolled some number of quarter turns about its length
/// (onto an edge, or flipped over), then turned some number of quarter turns.
///
/// A saw measures positions on its table `across` it toward the fence, `along` it (from the
/// board's leading end on a table saw, toward the stop on a miter saw), and `up` from the table.
/// Unrolled and unturned, a board's width runs across, its length along, and its thickness up,
/// with its `t = 0` face down. A roll stands it on its `w = 0` edge with its `t = 0` face toward
/// the fence; two roll it face down. Turning is counterclockwise seen from above.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Orientation {
    /// Quarter turns, 0 to 3, made after the rolls.
    turns: u8,
    /// Quarter rolls about the board's length, 0 to 3: flat, on edge, flipped, on the other edge.
    rolls: u8,
}

/// One of the saw's directions: across the table toward the fence, up, or along the table.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Table {
    Across,
    Up,
    Along,
}

impl Orientation {
    /// Turned a quarter turn counterclockwise, seen from above.
    pub fn turned_left(self) -> Orientation {
        Orientation {
            turns: (self.turns + 1) % 4,
            ..self
        }
    }

    /// Turned a quarter turn clockwise, seen from above.
    pub fn turned_right(self) -> Orientation {
        Orientation {
            turns: (self.turns + 3) % 4,
            ..self
        }
    }

    /// Turned over, about the direction along the table. A board turned and then flipped lies
    /// as if it had been flipped and then turned the other way.
    pub fn flipped(self) -> Orientation {
        Orientation {
            turns: (4 - self.turns) % 4,
            rolls: (self.rolls + 2) % 4,
        }
    }

    /// Stood up on an edge, rolled a quarter turn about its length; or, standing on an edge,
    /// laid back down the way it was.
    pub fn tipped(self) -> Orientation {
        let rolls = if self.on_edge() {
            self.rolls - 1
        } else {
            self.rolls + 1
        };
        Orientation { rolls, ..self }
    }

    /// Quarter turns counterclockwise, 0 to 3, made after any rolls.
    pub fn turns(self) -> u8 {
        self.turns
    }

    /// Quarter rolls about the board's length, 0 to 3, made before the turns: each one tips its
    /// top face toward the operator, away from the fence.
    pub fn rolls(self) -> u8 {
        self.rolls
    }

    /// Whether the board stands on an edge rather than lying on a face.
    pub fn on_edge(self) -> bool {
        self.rolls % 2 == 1
    }

    /// The board's axis that lies across the table, and whether the board's coordinates run
    /// backward along it: from the fence side rather than toward it.
    pub fn across(self) -> (Axis, bool) {
        self.axis_on(Table::Across)
    }

    /// The board's axis that lies along the table, and whether the board's coordinates run
    /// backward along it.
    pub fn along(self) -> (Axis, bool) {
        self.axis_on(Table::Along)
    }

    /// The board's axis that stands up from the table, and whether the board's coordinates run
    /// backward along it: down from the top.
    pub fn up(self) -> (Axis, bool) {
        self.axis_on(Table::Up)
    }

    /// The turn that lays the board's axes (`l`, `w`, `t`) onto the saw's, as `x`, `y`, and `z`:
    /// along the table, across it toward the fence, and up. The same turn lays a glue-up on a
    /// saw the way this lays a board.
    pub fn rotation(self) -> Rotation {
        let mut columns = [[0i8; 3]; 3];
        for (row, (axis, backward)) in [self.along(), self.across(), self.up()]
            .into_iter()
            .enumerate()
        {
            let column = match axis {
                Axis::Length => 0,
                Axis::Width => 1,
                Axis::Thickness => 2,
            };
            columns[column][row] = if backward { -1 } else { 1 };
        }
        Rotation::square(columns)
    }

    /// The board's axis on the saw's direction `table`, rolled and then turned into place from
    /// the board lying flat with its length along the table.
    fn axis_on(self, table: Table) -> (Axis, bool) {
        let roll = |(on, forward): (Table, bool)| match on {
            Table::Across => (Table::Up, forward),
            Table::Up => (Table::Across, !forward),
            Table::Along => (Table::Along, forward),
        };
        let turn = |(on, forward): (Table, bool)| match on {
            Table::Along => (Table::Across, forward),
            Table::Across => (Table::Along, !forward),
            Table::Up => (Table::Up, forward),
        };
        [
            (Axis::Length, Table::Along),
            (Axis::Width, Table::Across),
            (Axis::Thickness, Table::Up),
        ]
        .into_iter()
        .find_map(|(axis, start)| {
            let mut placed = (start, true);
            for _ in 0..self.rolls {
                placed = roll(placed);
            }
            for _ in 0..self.turns {
                placed = turn(placed);
            }
            (placed.0 == table).then_some((axis, !placed.1))
        })
        .expect("each of the saw's directions has one of the board's axes")
    }

    /// The direction on the board that runs `across` the table toward the fence and `along` it,
    /// in those proportions, for a board lying on a face.
    pub(crate) fn direction_onto_board(self, across: i64, along: i64) -> Direction {
        let (across_axis, across_backward) = self.across();
        let (_, along_backward) = self.along();
        let across = if across_backward { -across } else { across };
        let along = if along_backward { -along } else { along };
        match across_axis {
            Axis::Width => Direction::new(along, across),
            Axis::Length => Direction::new(across, along),
            Axis::Thickness => panic!("a board on edge has no face direction across the table"),
        }
    }

    /// Where the point `across` and `along` the table from the near corner of `board` is on
    /// the board, as `(l, w)`, for a board lying on a face.
    pub(crate) fn point_onto_board(
        self,
        board: &Board,
        across: Length,
        along: Length,
    ) -> (Length, Length) {
        let (across_axis, across_backward) = self.across();
        let (along_axis, along_backward) = self.along();
        let (across, _) =
            Orientation::onto_board((across, across), across_axis.of(board), across_backward);
        let (along, _) =
            Orientation::onto_board((along, along), along_axis.of(board), along_backward);
        match across_axis {
            Axis::Width => (along, across),
            Axis::Length => (across, along),
            Axis::Thickness => panic!("a board on edge has no face point across the table"),
        }
    }

    /// Converts the span `from..to` on the table, measured from the near side of a board
    /// `extent` long in that direction, to the board's own coordinates.
    pub(crate) fn onto_board(
        (from, to): (Length, Length),
        extent: Length,
        backward: bool,
    ) -> (Length, Length) {
        if backward {
            (extent - to, extent - from)
        } else {
            (from, to)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_way_a_board_lies_is_a_turn_not_a_mirror_image() {
        for rolls in 0..4 {
            for turns in 0..4 {
                let lie = Orientation { turns, rolls };
                let [l, w, t] = lie.rotation().columns();
                let cross = [
                    l[1] * w[2] - l[2] * w[1],
                    l[2] * w[0] - l[0] * w[2],
                    l[0] * w[1] - l[1] * w[0],
                ];
                assert_eq!(cross, t, "{lie:?}");
            }
        }
        // Lying as it comes, its length runs along the table, its width across, and its
        // thickness up.
        assert_eq!(Orientation::default().rotation(), Rotation::NONE);
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

    #[test]
    fn spans_on_a_backward_axis_count_from_the_far_side() {
        let inch = Length::from_inches;
        assert_eq!(
            Orientation::onto_board((inch(1), inch(3)), inch(10), true),
            (inch(7), inch(9))
        );
    }
}
