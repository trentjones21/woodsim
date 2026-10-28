use std::fmt;

use crate::{Axis, Board, Kerf, Length, Orientation, Piece};

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
/// the board turned a quarter turn and pushed by the miter gauge, it crosscuts.
///
/// The board lies on the table as its `Orientation` says and leads with its near end along it.
/// Against the rip fence, it rides the fence on its far side across the table; with the miter
/// gauge, its near side rests against the gauge's stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TableSaw {
    pub blade: Blade,
    /// Distance from the rip fence to the near side of the blade.
    pub fence: Length,
    /// How far the blade's teeth reach above the table.
    pub height: Length,
    /// The miter gauge, when it guides the board instead of the rip fence: the distance from
    /// its stop to the far side of the blade. It holds the board square to the blade, and the
    /// rip fence is slid out of the way so it can't trap the offcut.
    pub miter_gauge: Option<Length>,
}

impl TableSaw {
    /// Feeds `board`, lying as `lie` says, past the blade while the blade's center travels from
    /// `start` to `end` along it, measured from its leading end: negative while the board is
    /// still short of the blade, and past its far end once the board is by. The piece against
    /// the fence, or against the gauge's stop, comes out exactly as wide across the table as
    /// the setting.
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
        let (across, across_backward) = lie.across();
        let (along, along_backward) = lie.along();
        let span = across.of(board);
        let reaches = |setting: Length| {
            if setting <= Length::ZERO {
                Err(CutError::NonPositiveSetting(setting))
            } else if setting >= span {
                Err(CutError::MissesBoard {
                    setting,
                    available: span,
                })
            } else {
                Ok(setting)
            }
        };
        // Where the slot is on the table, from the board's near side: the gauge's stop setting
        // in from that side, or the fence setting in from the far side.
        let slot = match self.miter_gauge {
            Some(stop) => {
                let stop = reaches(stop)?;
                (stop, stop + self.blade.kerf)
            }
            None => {
                let fence = reaches(self.fence)?;
                (span - fence - self.blade.kerf, span - fence)
            }
        };
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
        let pieces = kerfed.pieces();
        if let [whole] = pieces.as_slice()
            && whole.board == kerfed
        {
            return Ok(Pass::Kerfed(kerfed));
        }
        let waste = slot.1.min(span) - slot.0.max(Length::ZERO);
        // The measured piece is on the fence's side of the slot, or the gauge's stop's.
        let toward_fence = |piece: &Piece| on_table(piece, across, span, across_backward).1;
        let measured = |piece: &Piece| {
            let beyond = past_middle(on_table(piece, across, span, across_backward), slot);
            beyond == self.miter_gauge.is_none()
        };
        Ok(Pass::Cut(sort_out(pieces, measured, toward_fence, waste)))
    }

    /// Pushes `board`, lying as `lie` says, all the way past the blade. It's cut apart if the
    /// blade is at least as high as the board is thick, and grooved if it's lower.
    pub fn rip(&self, board: &Board, lie: Orientation) -> Result<Pass, CutError> {
        let radius = self.blade.radius();
        let reach = lie.along().0.of(board);
        self.feed(board, lie, -radius, reach + radius)
    }
}

/// A miter saw that doesn't slide: its blade swings down on an arm to chop square across a
/// board lying along the fence. The board's far end along the fence rests against the stop.
///
/// Heights are above the saw's table; distances across it are out in front of the fence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiterSaw {
    pub blade: Blade,
    /// The widest board the blade can cut across.
    pub capacity: Length,
    /// The pivot the arm swings on: how far behind the fence, and how high.
    pub pivot: (Length, Length),
    /// How far the blade's center is from the pivot.
    pub arm: Length,
    /// How high the blade's lowest point is with the arm all the way up, and all the way down,
    /// into the slot in the table.
    pub stroke: (Length, Length),
}

impl MiterSaw {
    /// A 12" saw that doesn't slide, with a full-kerf blade. It cuts boards up to 7-1/2" wide.
    pub fn twelve_inch() -> MiterSaw {
        MiterSaw {
            blade: Blade {
                diameter: Length::from_inches(12),
                kerf: Blade::FULL_KERF,
            },
            capacity: Length::from_inch_fraction(15, 2),
            pivot: (Length::from_inches(4), Length::from_inches(6)),
            arm: Length::from_inches(8),
            stroke: (Length::from_inch_fraction(9, 2), -Length::from_inches(3)),
        }
    }

    /// Where the blade's center is when its lowest point is `bottom` high: how far out in
    /// front of the fence, and how high. It's on the arc the arm swings it around the pivot.
    pub fn center(&self, bottom: Length) -> (Length, Length) {
        let height = bottom + self.blade.radius();
        let rise = height - self.pivot.1;
        let reach = (self.arm.squared() - rise.squared()).max(0).isqrt();
        (Length::from_nm(reach as i64) - self.pivot.0, height)
    }

    /// Brings the blade down on `board`, lying as `lie` says against the stop `length` from the
    /// blade, until its lowest point is `bottom` high. All the way down, it chops the board in
    /// two. Stopped short, by hand or by a depth stop, it leaves a scar shaped like the blade's
    /// circle: deepest under the blade's center and shallower toward the fence and the front.
    ///
    /// The blade's center swings along an arc on the way down; within the board, the circle at
    /// its lowest takes all the wood the higher ones did, so the scar is that one circle.
    pub fn chop(
        &self,
        board: &Board,
        lie: Orientation,
        length: Length,
        bottom: Length,
    ) -> Result<Pass, CutError> {
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
        if length <= Length::ZERO {
            return Err(CutError::NonPositiveSetting(length));
        }
        if length >= total {
            return Err(CutError::MissesBoard {
                setting: length,
                available: total,
            });
        }
        // Along the fence, the blade is `length` from the stop at the board's far end.
        let slot = (total - length - self.blade.kerf, total - length);
        let (near, far) = Orientation::onto_board(slot, total, along_backward);
        // Across the saw, the board reaches out from the fence to its front edge.
        let (out, height) = self.center(bottom);
        let (at, _) = Orientation::onto_board((span - out, span - out), span, across_backward);
        let kerf = Kerf {
            across: along,
            near,
            far,
            radius: self.blade.radius(),
            center: if lie.is_flipped() {
                board.thickness() - height
            } else {
                height
            },
            start: at,
            end: at,
        };
        if !kerf.touches(board.length(), board.width(), board.thickness()) {
            return Ok(Pass::Missed);
        }
        let kerfed = board.with_kerf(kerf);
        let pieces = kerfed.pieces();
        if let [whole] = pieces.as_slice()
            && whole.board == kerfed
        {
            return Ok(Pass::Kerfed(kerfed));
        }
        let waste = slot.1.min(total) - slot.0.max(Length::ZERO);
        let toward_fence = |piece: &Piece| on_table(piece, across, span, across_backward).1;
        let toward_stop =
            |piece: &Piece| past_middle(on_table(piece, along, total, along_backward), slot);
        Ok(Pass::Cut(sort_out(
            pieces,
            toward_stop,
            toward_fence,
            waste,
        )))
    }

    /// Brings the blade all the way down on `board`, crosscutting it so the measured piece
    /// against the stop is exactly `length` long, if the blade reaches across and through it.
    pub fn crosscut(
        &self,
        board: &Board,
        lie: Orientation,
        length: Length,
    ) -> Result<Pass, CutError> {
        self.chop(board, lie, length, self.stroke.1)
    }
}

/// Where `piece` reaches on one of the saw's axes, measured from the near side of a board
/// `extent` long on it.
fn on_table(piece: &Piece, axis: Axis, extent: Length, backward: bool) -> (Length, Length) {
    let from = piece.at(axis);
    Orientation::onto_board((from, from + axis.of(&piece.board)), extent, backward)
}

/// Whether the span `(from, to)` is past the middle of `slot`, on its far side.
fn past_middle((from, to): (Length, Length), slot: (Length, Length)) -> bool {
    (from + to).as_nm() > (slot.0 + slot.1).as_nm()
}

/// Sorts the pieces of a board the blade cut apart: the measured piece is the biggest on the
/// measured side of the slot, the offcut the biggest on the other, and any more are loose.
fn sort_out(
    pieces: Vec<Piece>,
    measured: impl Fn(&Piece) -> bool,
    toward_fence: impl Fn(&Piece) -> Length + Copy,
    waste: Length,
) -> Cut {
    let (mut near, mut far): (Vec<Piece>, Vec<Piece>) = pieces.into_iter().partition(measured);
    let piece = take_biggest(&mut near, toward_fence)
        .expect("the wood beside the fence or stop is never in the blade's path");
    let offcut = take_biggest(&mut far, toward_fence);
    near.append(&mut far);
    Cut {
        piece,
        offcut,
        waste,
        loose: near,
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
