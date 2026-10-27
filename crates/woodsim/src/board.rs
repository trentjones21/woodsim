use std::fmt;

use crate::{Kerf, Length};

/// A board: rectangular stock, plus any slots saws have cut into it without cutting it apart.
/// Its length runs along the grain.
///
/// Positions on a board are in its own coordinates: `l` along its length from one end, `w`
/// across its width from one edge, and `t` up through its thickness from its bottom face.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Board {
    length: Length,
    width: Length,
    thickness: Length,
    kerfs: Vec<Kerf>,
}

impl Board {
    /// Panics if any dimension is zero or negative.
    pub fn new(length: Length, width: Length, thickness: Length) -> Board {
        assert!(
            length > Length::ZERO && width > Length::ZERO && thickness > Length::ZERO,
            "board dimensions must be positive, got {thickness} x {width} x {length}"
        );
        Board {
            length,
            width,
            thickness,
            kerfs: Vec::new(),
        }
    }

    pub fn length(&self) -> Length {
        self.length
    }

    pub fn width(&self) -> Length {
        self.width
    }

    pub fn thickness(&self) -> Length {
        self.thickness
    }

    /// The slots cut into this board, oldest first.
    pub fn kerfs(&self) -> &[Kerf] {
        &self.kerfs
    }

    /// This board with one more slot cut into it.
    pub(crate) fn with_kerf(&self, kerf: Kerf) -> Board {
        let mut board = self.clone();
        board.kerfs.push(kerf);
        board
    }

    /// The part of this board from `from` to `to` across its width, as a board of its own.
    pub(crate) fn across(&self, from: Length, to: Length) -> Board {
        self.part(Length::ZERO, from, self.length, to - from)
    }

    /// The part of this board from `from` to `to` along its length, as a board of its own.
    pub(crate) fn along(&self, from: Length, to: Length) -> Board {
        self.part(from, Length::ZERO, to - from, self.width)
    }

    /// The part of this board with its corner at `(l, w)` = (`along`, `across`) and the given
    /// length and width. It keeps the slots that reach into it.
    fn part(&self, along: Length, across: Length, length: Length, width: Length) -> Board {
        let mut part = Board::new(length, width, self.thickness);
        part.kerfs = self
            .kerfs
            .iter()
            .map(|kerf| kerf.moved_to(along, across))
            .filter(|kerf| kerf.touches(length, width, self.thickness))
            .collect();
        part
    }
}

/// Formats the way woodworkers write it: thickness x width x length.
impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} x {} x {}", self.thickness, self.width, self.length)
    }
}
