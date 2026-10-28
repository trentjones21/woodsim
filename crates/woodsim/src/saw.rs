use std::fmt;

use crate::{Board, Kerf, Length, Orientation, Piece};

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

/// What one pass through a saw produces when it cuts a board apart. Each piece says where it
/// was on the board that was cut.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
    /// The piece on the measured side of the blade: against the fence, or the length you cut to.
    pub piece: Piece,
    /// The piece on the far side of the blade, or `None` if the blade turned it all to sawdust.
    pub offcut: Option<Piece>,
    /// How much of the cut dimension became sawdust: the full kerf, or less if there's no offcut.
    pub waste: Length,
    /// More pieces the cut set loose. Where a slot already in the board goes all the way
    /// through what's left on one side of the blade, that side falls apart into strips; the
    /// biggest is the piece or the offcut, and the rest are here. A stopped cut that meets
    /// another slot can set a corner loose too.
    pub loose: Vec<Piece>,
}

/// What feeding a board past a table saw's blade did to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pass {
    /// The blade hasn't touched the board, or is too low to.
    Missed,
    /// The blade cut a slot into the board without cutting it apart: a stopped cut, or a
    /// groove or dado when the blade is lower than the board is thick.
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
    /// The board is wider, across the saw, than the blade can reach.
    TooWide { width: Length, capacity: Length },
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
            CutError::TooWide { width, capacity } => write!(
                f,
                "the blade can't reach across {width} of board; it cuts boards up to {capacity} \
                 wide"
            ),
        }
    }
}

impl std::error::Error for CutError {}

/// A table saw. With the board's length along the fence it rips, cutting with the grain; with
/// the board turned a quarter turn, it crosscuts.
///
/// The board lies on the table as its `Orientation` says, riding the fence on its far side
/// across the table and leading with its near end along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableSaw {
    pub blade: Blade,
    /// Distance from the fence to the near side of the blade.
    pub fence: Length,
    /// How far the blade's teeth reach above the table.
    pub height: Length,
}

impl TableSaw {
    /// Feeds `board`, lying as `lie` says, along the fence while the blade's center travels
    /// from `start` to `end` along it, measured from its leading end: negative while the board
    /// is still short of the blade, and past its far end once the board is by. The piece
    /// against the fence comes out exactly as wide, across the table, as the fence setting.
    ///
    /// Panics if `start` is past `end`.
    pub fn feed(
        &self,
        board: &Board,
        lie: Orientation,
        start: Length,
        end: Length,
    ) -> Result<Pass, CutError> {
        assert!(
            start <= end,
            "the blade's travel runs backward: {start} to {end}"
        );
        if self.fence <= Length::ZERO {
            return Err(CutError::NonPositiveSetting(self.fence));
        }
        let (across, across_backward) = lie.across();
        let (along, along_backward) = lie.along();
        let span = across.of(board);
        if self.fence >= span {
            return Err(CutError::MissesBoard {
                setting: self.fence,
                available: span,
            });
        }
        // On the table, the slot is the fence setting in from the board's side at the fence.
        let slot = (span - self.fence - self.blade.kerf, span - self.fence);
        let (near, far) = Orientation::onto_board(slot, span, across_backward);
        let (start, end) = Orientation::onto_board((start, end), along.of(board), along_backward);
        let radius = self.blade.radius();
        let center = self.height - radius;
        let kerf = Kerf {
            across,
            near,
            far,
            radius,
            center: if lie.is_flipped() {
                board.thickness() - center
            } else {
                center
            },
            start,
            end,
        };
        if !kerf.touches(board.length(), board.width(), board.thickness()) {
            return Ok(Pass::Missed);
        }
        let kerfed = board.with_kerf(kerf);
        let mut pieces = kerfed.pieces();
        if let [whole] = pieces.as_slice()
            && whole.board == kerfed
        {
            return Ok(Pass::Kerfed(kerfed));
        }
        let waste = slot.1.min(span) - slot.0.max(Length::ZERO);
        // The piece against the fence is the one reaching furthest toward it.
        let toward_fence = |piece: &Piece| {
            let from = piece.at(across);
            if across_backward {
                span - from
            } else {
                from + across.of(&piece.board)
            }
        };
        let index = (0..pieces.len())
            .max_by_key(|&i| (toward_fence(&pieces[i]), area(&pieces[i])))
            .expect("the wood beside the fence is never in the blade's path");
        let piece = pieces.remove(index);
        let offcut = take_biggest(&mut pieces, toward_fence);
        Ok(Pass::Cut(Cut {
            piece,
            offcut,
            waste,
            loose: pieces,
        }))
    }

    /// Pushes `board`, lying as `lie` says, all the way past the blade. It's cut apart if the
    /// blade is at least as high as the board is thick, and grooved if it's lower.
    pub fn rip(&self, board: &Board, lie: Orientation) -> Result<Pass, CutError> {
        let radius = self.blade.radius();
        let reach = lie.along().0.of(board);
        self.feed(board, lie, -radius, reach + radius)
    }
}

/// A miter saw, chopping square across a board where it lies along the fence. The board's far
/// end along the fence rests against the stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiterSaw {
    pub blade: Blade,
    /// The widest board the blade can cut across: about 7-1/2" for a 12" saw that doesn't
    /// slide.
    pub capacity: Length,
}

impl MiterSaw {
    /// Cuts `board`, lying as `lie` says, so the measured piece against the stop is exactly
    /// `length` long along the fence. With the board's length along the fence, that's a
    /// crosscut.
    pub fn crosscut(
        &self,
        board: &Board,
        lie: Orientation,
        length: Length,
    ) -> Result<Cut, CutError> {
        let (across, across_backward) = lie.across();
        let span = across.of(board);
        if span > self.capacity {
            return Err(CutError::TooWide {
                width: span,
                capacity: self.capacity,
            });
        }
        let (along, along_backward) = lie.along();
        let total = along.of(board);
        let (kept, rest, waste) = split(total, length, self.blade.kerf)?;
        // Each side of the blade may fall apart where a slot runs all the way through it.
        let side = |range: (Length, Length)| -> Vec<Piece> {
            let (from, to) = Orientation::onto_board(range, total, along_backward);
            let mut pieces = board.slice(along, from, to).pieces();
            for piece in &mut pieces {
                piece.shift(along, from);
            }
            pieces
        };
        let toward_fence = |piece: &Piece| {
            let from = piece.at(across);
            if across_backward {
                span - from
            } else {
                from + across.of(&piece.board)
            }
        };
        let mut against_stop = side((total - kept, total));
        let mut beyond = rest.map_or_else(Vec::new, |rest| side((Length::ZERO, rest)));
        let piece = take_biggest(&mut against_stop, toward_fence)
            .expect("a board has wood right up to its ends, so the stop side holds some");
        let offcut = take_biggest(&mut beyond, toward_fence);
        against_stop.append(&mut beyond);
        Ok(Cut {
            piece,
            offcut,
            waste,
            loose: against_stop,
        })
    }
}

/// How much of a board's face a piece covers, as its bounding rectangle.
fn area(piece: &Piece) -> i128 {
    piece.board.length().as_nm() as i128 * piece.board.width().as_nm() as i128
}

/// Removes and returns the biggest piece, the one nearest the fence among equals.
fn take_biggest(pieces: &mut Vec<Piece>, toward_fence: impl Fn(&Piece) -> Length) -> Option<Piece> {
    let index = (0..pieces.len()).max_by_key(|&i| (area(&pieces[i]), toward_fence(&pieces[i])))?;
    Some(pieces.remove(index))
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
