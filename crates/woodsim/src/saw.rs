use std::fmt;

use crate::{Board, Length};

/// A saw blade. Every cut turns a strip of wood as wide as the kerf into sawdust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blade {
    pub kerf: Length,
}

impl Blade {
    /// A standard full-kerf blade.
    pub const FULL_KERF: Blade = Blade {
        kerf: Length::from_inch_fraction(1, 8),
    };
    /// A thin-kerf blade, common on lower-powered saws.
    pub const THIN_KERF: Blade = Blade {
        kerf: Length::from_inch_fraction(3, 32),
    };
}

/// What one pass through a saw produces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
    /// The piece on the measured side of the blade: against the fence, or the length you cut to.
    pub piece: Board,
    /// The piece on the far side of the blade, or `None` if the blade turned it all to sawdust.
    pub offcut: Option<Board>,
    /// How much of the cut dimension became sawdust: the full kerf, or less if there's no offcut.
    pub waste: Length,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutError {
    /// The fence or cut length is zero or negative.
    NonPositiveSetting(Length),
    /// The setting is at or past the board's far edge, so the blade never touches it.
    MissesBoard { setting: Length, available: Length },
}

impl fmt::Display for CutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CutError::NonPositiveSetting(setting) => {
                write!(f, "the saw can't be set to {setting}")
            }
            CutError::MissesBoard { setting, available } => write!(
                f,
                "the blade misses the board: set to {setting}, but the board is only {available}"
            ),
        }
    }
}

impl std::error::Error for CutError {}

/// A table saw. Ripping runs the board along the fence, cutting with the grain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableSaw {
    pub blade: Blade,
    /// Distance from the fence to the near side of the blade.
    pub fence: Length,
}

impl TableSaw {
    /// Rips `board` so the piece against the fence is exactly as wide as the fence setting.
    pub fn rip(&self, board: &Board) -> Result<Cut, CutError> {
        let (kept, rest, waste) = split(board.width(), self.fence, self.blade.kerf)?;
        Ok(Cut {
            piece: board.with_width(kept),
            offcut: rest.map(|width| board.with_width(width)),
            waste,
        })
    }
}

/// A miter saw, crosscutting square across the grain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiterSaw {
    pub blade: Blade,
}

impl MiterSaw {
    /// Crosscuts `board` so the measured piece is exactly `length` long.
    pub fn crosscut(&self, board: &Board, length: Length) -> Result<Cut, CutError> {
        let (kept, rest, waste) = split(board.length(), length, self.blade.kerf)?;
        Ok(Cut {
            piece: board.with_length(kept),
            offcut: rest.map(|length| board.with_length(length)),
            waste,
        })
    }
}

/// Splits `total` at `setting`, losing up to `kerf` to the blade.
/// Returns the measured side, the far side if any wood is left, and the waste.
fn split(
    total: Length,
    setting: Length,
    kerf: Length,
) -> Result<(Length, Option<Length>, Length), CutError> {
    if setting <= Length::ZERO {
        return Err(CutError::NonPositiveSetting(setting));
    }
    if setting >= total {
        return Err(CutError::MissesBoard {
            setting,
            available: total,
        });
    }
    let rest = total - setting - kerf;
    if rest > Length::ZERO {
        Ok((setting, Some(rest), kerf))
    } else {
        Ok((setting, None, total - setting))
    }
}
