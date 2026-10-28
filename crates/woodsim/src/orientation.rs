use crate::{Board, Length};

/// One of the two directions in a board's face: along its length (with the grain), or across
/// its width.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    Length,
    Width,
}

impl Axis {
    pub fn other(self) -> Axis {
        match self {
            Axis::Length => Axis::Width,
            Axis::Width => Axis::Length,
        }
    }

    /// How far `board` reaches in this direction.
    pub fn of(self, board: &Board) -> Length {
        self.pick(board.length(), board.width())
    }

    /// `length` or `width`, whichever this axis is.
    pub(crate) fn pick(self, length: Length, width: Length) -> Length {
        match self {
            Axis::Length => length,
            Axis::Width => width,
        }
    }
}

/// How a board lies flat on a saw's table: turned some number of quarter turns, and face up or
/// flipped over.
///
/// A saw measures positions on its table `across` it toward the fence, `along` it (from the
/// board's leading end on a table saw, toward the stop on a miter saw), and `up` from the table.
/// Unturned and face up, a board's width runs across, its length along, and its `t = 0` face is
/// down. Turning is counterclockwise seen from above; flipping rolls the board over about the
/// direction along the table, so its two edges swap sides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Orientation {
    /// Quarter turns, 0 to 3, made after any flip.
    turns: u8,
    flipped: bool,
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
            flipped: !self.flipped,
        }
    }

    /// Quarter turns counterclockwise, 0 to 3, made after any flip.
    pub fn turns(self) -> u8 {
        self.turns
    }

    pub fn is_flipped(self) -> bool {
        self.flipped
    }

    /// The board's axis that lies across the table, and whether the board's coordinates run
    /// backward along it: from the fence side rather than toward it.
    pub fn across(self) -> (Axis, bool) {
        match self.turns {
            0 => (Axis::Width, self.flipped),
            1 => (Axis::Length, false),
            2 => (Axis::Width, !self.flipped),
            _ => (Axis::Length, true),
        }
    }

    /// The board's axis that lies along the table, and whether the board's coordinates run
    /// backward along it.
    pub fn along(self) -> (Axis, bool) {
        match self.turns {
            0 => (Axis::Length, false),
            1 => (Axis::Width, !self.flipped),
            2 => (Axis::Length, true),
            _ => (Axis::Width, self.flipped),
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
