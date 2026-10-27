use std::fmt;

use crate::{Board, Kerf, Length};

/// A round saw blade. Every cut turns a strip of wood as wide as the kerf into sawdust.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Blade {
    pub diameter: Length,
    pub kerf: Length,
}

impl Blade {
    /// The kerf of a standard full-kerf blade.
    pub const FULL_KERF: Length = Length::from_inch_fraction(1, 8);
    /// The kerf of a thin-kerf blade, common on lower-powered saws.
    pub const THIN_KERF: Length = Length::from_inch_fraction(3, 32);

    pub fn radius(&self) -> Length {
        Length::from_nm(self.diameter.as_nm() / 2)
    }
}

/// What one pass through a saw produces when it cuts a board apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
    /// The piece on the measured side of the blade: against the fence, or the length you cut to.
    pub piece: Board,
    /// The piece on the far side of the blade, or `None` if the blade turned it all to sawdust.
    pub offcut: Option<Board>,
    /// How much of the cut dimension became sawdust: the full kerf, or less if there's no offcut.
    pub waste: Length,
}

/// What feeding a board past a table saw's blade did to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pass {
    /// The blade hasn't touched the board, or is too low to.
    Missed,
    /// The blade cut a slot into the board without cutting it apart: a stopped cut, or a
    /// groove when the blade is lower than the board is thick.
    Kerfed(Board),
    /// The blade cut the board apart.
    Cut(Cut),
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
///
/// The board rides the fence on its `w = width` edge with its `l = 0` end leading and its
/// bottom face on the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableSaw {
    pub blade: Blade,
    /// Distance from the fence to the near side of the blade.
    pub fence: Length,
    /// How far the blade's teeth reach above the table.
    pub height: Length,
}

impl TableSaw {
    /// Feeds `board` along the fence while the blade's center travels from `start` to `end`
    /// along it, as `l` positions: negative while the board is still short of the blade, and
    /// past its length once the board is by. The piece against the fence comes out exactly as
    /// wide as the fence setting.
    ///
    /// Panics if `start` is past `end`.
    pub fn feed(&self, board: &Board, start: Length, end: Length) -> Result<Pass, CutError> {
        assert!(
            start <= end,
            "the blade's travel runs backward: {start} to {end}"
        );
        if self.fence <= Length::ZERO {
            return Err(CutError::NonPositiveSetting(self.fence));
        }
        let width = board.width();
        if self.fence >= width {
            return Err(CutError::MissesBoard {
                setting: self.fence,
                available: width,
            });
        }
        let far = width - self.fence;
        let radius = self.blade.radius();
        let kerf = Kerf {
            near: far - self.blade.kerf,
            far,
            radius,
            center: self.height - radius,
            start,
            end,
        };
        if !kerf.touches(board.length(), width, board.thickness()) {
            return Ok(Pass::Missed);
        }
        if !kerf.cuts_through(board.length(), board.thickness()) {
            return Ok(Pass::Kerfed(board.with_kerf(kerf)));
        }
        let piece = board.across(far, width);
        Ok(Pass::Cut(if kerf.near > Length::ZERO {
            Cut {
                piece,
                offcut: Some(board.across(Length::ZERO, kerf.near)),
                waste: self.blade.kerf,
            }
        } else {
            Cut {
                piece,
                offcut: None,
                waste: far,
            }
        }))
    }

    /// Pushes `board` all the way past the blade. It's cut apart if the blade is at least as
    /// high as the board is thick, and grooved if it's lower.
    pub fn rip(&self, board: &Board) -> Result<Pass, CutError> {
        let radius = self.blade.radius();
        self.feed(board, -radius, board.length() + radius)
    }
}

/// A miter saw, crosscutting square across the grain. The board's `l = length` end rests
/// against the stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiterSaw {
    pub blade: Blade,
}

impl MiterSaw {
    /// Crosscuts `board` so the measured piece is exactly `length` long.
    pub fn crosscut(&self, board: &Board, length: Length) -> Result<Cut, CutError> {
        let total = board.length();
        let (kept, rest, waste) = split(total, length, self.blade.kerf)?;
        Ok(Cut {
            piece: board.along(total - kept, total),
            offcut: rest.map(|rest| board.along(Length::ZERO, rest)),
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
