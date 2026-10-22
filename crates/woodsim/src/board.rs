use std::fmt;

use crate::Length;

/// A rectangular board. Its length runs along the grain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    length: Length,
    width: Length,
    thickness: Length,
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

    pub(crate) fn with_length(&self, length: Length) -> Board {
        Board::new(length, self.width, self.thickness)
    }

    pub(crate) fn with_width(&self, width: Length) -> Board {
        Board::new(self.length, width, self.thickness)
    }
}

/// Formats the way woodworkers write it: thickness x width x length.
impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} x {} x {}", self.thickness, self.width, self.length)
    }
}
