use std::fmt;

use crate::{
    Angle, Assembly, Axis, Board, Direction, Edge, Kerf, Length, Orientation, Piece, Rotation,
    glue_up::{GlueUpPass, Laid, Path},
    history::{Lie, Operation, Side, Tool},
    kerf::Across,
};

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
    /// The board lies a way the simulation can't cut yet, for the reason given.
    NotModeled(&'static str),
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
            CutError::NotModeled(reason) => write!(f, "{reason}"),
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
    /// its stop to the far side of the blade. It holds the board square to the blade, or at
    /// `gauge_angle`, and the rip fence is slid out of the way so it can't trap the offcut.
    pub miter_gauge: Option<Length>,
    /// How far the miter gauge's head is turned from square, either way, up to `MAX_GAUGE`.
    /// Its face holds the board's trailing side and its stop the board's end; turned, the blade
    /// crosses the board at the angle, its stop-side face crossing the gauge's face the stop's
    /// distance from the stop, so the piece against the stop is that long along the gauge, to
    /// its long point. Positive swings the blade's front, where the board meets it first,
    /// toward the stop.
    pub gauge_angle: Angle,
    /// How far the blade is tilted from square to the table, leaning away from the fence going
    /// up, up to `MAX_TILT`. It tilts about where it meets the table, so the fence and the
    /// gauge's stop still measure to there: the piece is the setting wide where it lay on the
    /// table, and wider or narrower on top. `height` is still how high its teeth reach.
    pub tilt: Angle,
}

/// As far as a miter gauge's head turns, either way.
pub const MAX_GAUGE: Angle = Angle::from_degrees(60);

/// As far as the blade tilts.
pub const MAX_TILT: Angle = Angle::from_degrees(45);

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
        assert!(
            Angle::ZERO <= self.tilt && self.tilt <= MAX_TILT,
            "the blade tilts from square to {MAX_TILT}, not {}",
            self.tilt
        );
        let tool = Tool::TableSaw {
            saw: *self,
            lie: Lie::Board(lie),
            travel: (start, end),
        };
        if self.tilt != Angle::ZERO {
            return self.feed_tilted(board, lie, (start, end), tool);
        }
        if let Some(stop) = self.miter_gauge
            && self.gauge_angle != Angle::ZERO
        {
            return self.feed_at_angle(board, lie, (start, end), stop, tool);
        }
        let (across, across_backward) = lie.across();
        let (along, along_backward) = lie.along();
        let (up, up_backward) = lie.up();
        if along == Axis::Thickness {
            return Err(CutError::NotModeled(
                "standing on edge, the board has to run along the table lengthwise",
            ));
        }
        let span = across.of(board);
        let slot = self.slot(span)?;
        let (near, far) = Orientation::onto_board(slot, span, across_backward);
        let (start, end) = Orientation::onto_board((start, end), along.of(board), along_backward);
        let radius = self.blade.radius();
        let height = self.height - radius;
        let center = if up_backward {
            up.of(board) - height
        } else {
            height
        };
        // Lying flat, the blade stands square to the board's face; on edge, parallel to it,
        // resawing it.
        let kind = match up {
            Axis::Thickness => Across::Face(Direction::of(across)),
            _ => Across::Thickness,
        };
        let kerf = Kerf::new(
            kind,
            (near, far),
            Direction::of(along),
            (start, end),
            radius,
            center,
        );
        if !board.touched_by(&kerf) {
            return Ok(Pass::Missed);
        }
        let (kerfed, pieces) = board.sawn(kerf, &Operation::new(tool));
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

    /// Feeds a board against the miter gauge turned to an angle, as `feed` does: the blade
    /// crosses it slanted, from the point on the gauge's face `stop` from the stop.
    fn feed_at_angle(
        &self,
        board: &Board,
        lie: Orientation,
        (start, end): (Length, Length),
        stop: Length,
        tool: Tool,
    ) -> Result<Pass, CutError> {
        assert!(
            self.gauge_angle.abs() <= MAX_GAUGE,
            "the gauge turns {MAX_GAUGE} either way, not {}",
            self.gauge_angle
        );
        if lie.on_edge() {
            return Err(CutError::NotModeled(
                "standing on edge against the miter gauge, a board can only be cut square",
            ));
        }
        let (across, across_backward) = lie.across();
        let (along, _) = lie.along();
        let (up, up_backward) = lie.up();
        // Along the gauge's face, and out from it to the board's leading side.
        let (span, depth) = (across.of(board), along.of(board));
        if stop <= Length::ZERO {
            return Err(CutError::NonPositiveSetting(stop));
        }
        let (cos, sin) = self.gauge_angle.unit();
        let toward_stop = lie.direction_onto_board(-cos, sin);
        let travel = lie.direction_onto_board(sin, cos);
        // The blade's stop-side face crosses the gauge's face, at the board's trailing side,
        // `stop` from the stop.
        let (l, w) = lie.point_onto_board(board, stop, depth);
        let face = Length::from_nm(toward_stop.dot(l, w) as i64);
        reaches_blade([(board, toward_stop, face)], stop, self.gauge_angle)?;
        let kerf_width = scaled(self.blade.kerf, toward_stop);
        // The blade's travel, from the board's leading corner.
        let lead = [
            (Length::ZERO, Length::ZERO),
            (board.length(), Length::ZERO),
            (Length::ZERO, board.width()),
            (board.length(), board.width()),
        ]
        .into_iter()
        .map(|(l, w)| travel.dot(l, w))
        .min()
        .expect("four corners");
        let lead = Length::from_nm(lead as i64);
        let radius = self.blade.radius();
        let height = self.height - radius;
        let center = if up_backward {
            up.of(board) - height
        } else {
            height
        };
        let kerf = Kerf::new(
            Across::Face(toward_stop),
            (face - kerf_width, face),
            travel,
            (lead + scaled(start, travel), lead + scaled(end, travel)),
            radius,
            center,
        );
        if !board.touched_by(&kerf) {
            return Ok(Pass::Missed);
        }
        let (kerfed, pieces) = board.sawn(kerf, &Operation::new(tool));
        if let [whole] = pieces.as_slice()
            && whole.board == kerfed
        {
            return Ok(Pass::Kerfed(kerfed));
        }
        // Along the gauge's face, a slanted kerf is wider than the blade.
        let along_face = Length::from_nm(
            (self.blade.kerf.as_nm() as f64 / self.gauge_angle.radians().cos()).round() as i64,
        );
        let waste = ((stop + along_face).min(span) - stop).max(Length::ZERO);
        let toward_fence = |piece: &Piece| on_table(piece, across, span, across_backward).1;
        // Each piece is wholly on one side of the slot, so any corner of it says which.
        let slot_middle = 2 * face.as_nm() as i128 - kerf_width.as_nm() as i128;
        let measured = |piece: &Piece| {
            let [l, w, _] = piece.board.corners()[0];
            2 * toward_stop.dot(piece.l + l, piece.w + w) > slot_middle
        };
        Ok(Pass::Cut(sort_out(pieces, measured, toward_fence, waste)))
    }

    /// Feeds a board past the blade tilted, as `feed` does, against the rip fence or the miter
    /// gauge at any angle: the blade leans about its path along the table, so the slot's faces
    /// lean in the board, and where the setting puts them is where they meet the table.
    fn feed_tilted(
        &self,
        board: &Board,
        lie: Orientation,
        (start, end): (Length, Length),
        tool: Tool,
    ) -> Result<Pass, CutError> {
        if lie.on_edge() {
            return Err(CutError::NotModeled(
                "standing on edge, a board can only be cut with the blade square so far",
            ));
        }
        let (across, across_backward) = lie.across();
        let (along, _) = lie.along();
        let (up, up_backward) = lie.up();
        let thickness = up.of(board);
        if self.height <= thickness {
            return Err(CutError::NotModeled(
                "a tilted blade has to reach above the board so far",
            ));
        }
        let (span, depth) = (across.of(board), along.of(board));
        // Toward the fence and along the blade's path, in the board's face: turned with the
        // gauge, if it's guiding the board.
        let angle = match self.miter_gauge {
            Some(_) => self.gauge_angle,
            None => Angle::ZERO,
        };
        let (cos, sin) = angle.unit();
        let toward_fence = lie.direction_onto_board(cos, -sin);
        let travel = lie.direction_onto_board(sin, cos);
        let (out, lean) = toward_fence.tilted(self.tilt);
        // Flipped over, the board's `t` runs down the table's up.
        let lean = if up_backward { -lean } else { lean };
        let table = if up_backward { thickness } else { Length::ZERO };
        let normal = ((out.scale_squared() + i128::from(lean).pow(2)) as f64).sqrt();
        let at_table = |(l, w): (Length, Length)| {
            Length::from_nm((out.dot(l, w) + i128::from(lean) * table.as_nm() as i128) as i64)
        };
        let kerf_width = Length::from_nm((self.blade.kerf.as_nm() as f64 * normal).round() as i64);
        // Square to its faces and along the table, the blade's tilted faces move the setting's
        // change times both cosines.
        let cosines = angle.radians().cos() * out.scale() / normal;
        let (near, far, waste) = match self.miter_gauge {
            // The stop-side face crosses the gauge's face, at the board's trailing side, `stop`
            // from the stop, and leans toward the stop going up.
            Some(stop) => {
                if stop <= Length::ZERO {
                    return Err(CutError::NonPositiveSetting(stop));
                }
                let near = at_table(lie.point_onto_board(board, stop, depth));
                let toward = Edge::leaning(out, lean, near.as_nm() as i128);
                reaches_beyond(board, toward, stop, cosines)?;
                let along_face =
                    Length::from_nm((self.blade.kerf.as_nm() as f64 / cosines).round() as i64);
                let waste = ((stop + along_face).min(span) - stop).max(Length::ZERO);
                (near, near + kerf_width, waste)
            }
            // The fence-side face is `fence` from the fence, and leans away from it going up.
            None => {
                let (_, fence_side) = self.slot(span)?;
                let far = at_table(lie.point_onto_board(board, fence_side, Length::ZERO));
                let across_table =
                    Length::from_nm((self.blade.kerf.as_nm() as f64 / cosines).round() as i64);
                let waste = (fence_side.min(span) - (fence_side - across_table).max(Length::ZERO))
                    .max(Length::ZERO);
                (far - kerf_width, far, waste)
            }
        };
        // The blade's travel, from the board's leading corner.
        let lead = [
            (Length::ZERO, Length::ZERO),
            (board.length(), Length::ZERO),
            (Length::ZERO, board.width()),
            (board.length(), board.width()),
        ]
        .into_iter()
        .map(|(l, w)| travel.dot(l, w))
        .min()
        .expect("four corners");
        let lead = Length::from_nm(lead as i64);
        // Its teeth reach `height` up, at the top of a circle tilted from upright.
        let radius = self.blade.radius();
        let rise = Length::from_nm((radius.as_nm() as f64 * out.scale() / normal).round() as i64);
        let center = self.height - rise;
        let center = if up_backward {
            thickness - center
        } else {
            center
        };
        let kerf = Kerf::tilted(
            out,
            lean,
            (near, far),
            travel,
            (lead + scaled(start, travel), lead + scaled(end, travel)),
            radius,
            center,
        );
        if !board.touched_by(&kerf) {
            return Ok(Pass::Missed);
        }
        let (kerfed, pieces) = board.sawn(kerf, &Operation::new(tool));
        if let [whole] = pieces.as_slice()
            && whole.board == kerfed
        {
            return Ok(Pass::Kerfed(kerfed));
        }
        let toward_fence = |piece: &Piece| on_table(piece, across, span, across_backward).1;
        // Each piece is wholly on one side of the slot, so any corner of it says which: the
        // fence's side is where its faces are further along their normal.
        let middle = near.as_nm() as i128 + far.as_nm() as i128;
        let measured = |piece: &Piece| {
            let [l, w, t] = piece.board.corners()[0];
            let at = out.dot(piece.l + l, piece.w + w)
                + i128::from(lean) * (piece.t + t).as_nm() as i128;
            (2 * at > middle) == self.miter_gauge.is_none()
        };
        Ok(Pass::Cut(sort_out(pieces, measured, toward_fence, waste)))
    }

    /// Pushes `board`, lying as `lie` says, all the way past the blade. It's cut apart if the
    /// blade is at least as high as the board is thick, and grooved if it's lower.
    pub fn rip(&self, board: &Board, lie: Orientation) -> Result<Pass, CutError> {
        let radius = self.blade.radius();
        self.feed(board, lie, -radius, self.reach(board, lie) + radius)
    }

    /// How far `board`, lying as `lie` says, reaches along the table as it's fed: its length
    /// that way, or, held at an angle by the miter gauge, from its leading corner back to its
    /// trailing one along the blade's travel.
    pub fn reach(&self, board: &Board, lie: Orientation) -> Length {
        if self.miter_gauge.is_none() || self.gauge_angle == Angle::ZERO || lie.on_edge() {
            return lie.along().0.of(board);
        }
        let (cos, sin) = self.gauge_angle.unit();
        let travel = lie.direction_onto_board(sin, cos);
        let along: Vec<i128> = [
            (Length::ZERO, Length::ZERO),
            (board.length(), Length::ZERO),
            (Length::ZERO, board.width()),
            (board.length(), board.width()),
        ]
        .into_iter()
        .map(|(l, w)| travel.dot(l, w))
        .collect();
        let (low, high) = (along.iter().min().unwrap(), along.iter().max().unwrap());
        Length::from_nm(((high - low) as f64 / travel.scale()).ceil() as i64)
    }

    /// Where the slot is across the table, from the near side of what's on it, `span` wide:
    /// the gauge's stop setting in from that side, or the fence setting in from the far side.
    fn slot(&self, span: Length) -> Result<(Length, Length), CutError> {
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
        Ok(match self.miter_gauge {
            Some(stop) => {
                let stop = reaches(stop)?;
                (stop, stop + self.blade.kerf)
            }
            None => {
                let fence = reaches(self.fence)?;
                (span - fence - self.blade.kerf, span - fence)
            }
        })
    }

    /// Feeds a glue-up past the blade, the way `feed` does a board, with `lie` laying its axes
    /// onto the table's: along it, across it toward the fence, and up, as `x`, `y`, `z` (as a
    /// board's `Orientation::rotation` lays a board). Every board in it has to lie flat.
    ///
    /// Panics if `start` is past `end`.
    pub fn feed_glue_up(
        &self,
        glue_up: &Assembly,
        lie: Rotation,
        start: Length,
        end: Length,
    ) -> Result<GlueUpPass, CutError> {
        assert!(
            start <= end,
            "the blade's travel runs backward: {start} to {end}"
        );
        if self.miter_gauge.is_some() && self.gauge_angle != Angle::ZERO {
            return Err(CutError::NotModeled(
                "a glue-up against the miter gauge can only be cut square so far",
            ));
        }
        if self.tilt != Angle::ZERO {
            return Err(CutError::NotModeled(
                "a glue-up can only be cut with the blade square so far",
            ));
        }
        let laid = Laid::new(glue_up, lie)?;
        let slot = self.slot(laid.extent[1])?;
        let radius = self.blade.radius();
        Ok(laid.cut(&Path {
            tool: Tool::TableSaw {
                saw: *self,
                lie: Lie::GlueUp(lie),
                travel: (start, end),
            },
            across: (0, 1),
            face: [Length::ZERO, slot.1],
            kerf: self.blade.kerf,
            through: [Length::ZERO; 2],
            travel: (start, end),
            height: self.height - radius,
            radius,
            measured_ahead: self.miter_gauge.is_none(),
        }))
    }

    /// Pushes a glue-up, lying as `lie` lays it, all the way past the blade.
    pub fn rip_glue_up(&self, glue_up: &Assembly, lie: Rotation) -> Result<GlueUpPass, CutError> {
        let radius = self.blade.radius();
        let reach = Laid::new(glue_up, lie)?.extent[0];
        self.feed_glue_up(glue_up, lie, -radius, reach + radius)
    }
}

/// A miter saw that doesn't slide: its blade swings down on an arm to chop across a board lying
/// along the fence, square or at the angle its table is turned to. The board's far end along
/// the fence rests against the stop.
///
/// Heights are above the saw's table; distances across it are out in front of the fence. The
/// table turns about the point where the blade's stop-side face crosses the fence, so wherever
/// it's turned, the cut leaves the board against the stop the same length along the fence.
///
/// How wide a board it cuts through is however far the blade reaches at the bottom of its
/// stroke. A wider board it cuts as far as it reaches, and it stays in one piece.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MiterSaw {
    pub blade: Blade,
    /// How far the table is turned from square to the fence: positive when the blade's front
    /// swings toward the stop, so the board against the stop comes out shorter along its front
    /// edge than along the fence.
    pub miter: Angle,
    /// How far the table turns either way.
    pub max_miter: Angle,
    /// The pivot the arm swings on: how far behind the fence, and how high.
    pub pivot: (Length, Length),
    /// How far the blade's center is from the pivot.
    pub arm: Length,
    /// How high the blade's lowest point is with the arm all the way up, and all the way down,
    /// into the slot in the table.
    pub stroke: (Length, Length),
}

impl MiterSaw {
    /// A 12" saw that doesn't slide, with a full-kerf blade. All the way down, it reaches from
    /// behind the fence to 7-1/2" in front of it.
    pub fn twelve_inch() -> MiterSaw {
        MiterSaw {
            blade: Blade {
                diameter: Length::from_inches(12),
                kerf: Blade::FULL_KERF,
            },
            miter: Angle::ZERO,
            max_miter: Angle::from_degrees(50),
            pivot: (Length::from_inches(5), Length::from_inches(6)),
            arm: Length::from_inches(8),
            stroke: (
                Length::from_inch_fraction(9, 2),
                -Length::from_inch_fraction(13, 5),
            ),
        }
    }

    /// How far out from the fence, along the blade, it cuts through the board's bottom face at
    /// the bottom of its stroke: where its circle crosses the table.
    pub fn reach(&self) -> Length {
        let (out, height) = self.center(self.stroke.1);
        let crossing = (self.blade.radius().squared() - height.squared()).max(0);
        out + Length::from_nm(crossing.isqrt() as i64)
    }

    /// The widest board the blade cuts all the way across at its miter angle, to the 64th
    /// below: at an angle, it reaches less far out from the fence.
    pub fn widest(&self) -> Length {
        let sixty_fourth = Length::from_inch_fraction(1, 64).as_nm();
        let reach = self.reach().as_nm() as f64 * self.miter.radians().cos();
        Length::from_nm((reach / sixty_fourth as f64).floor() as i64 * sixty_fourth)
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
    /// The stop touches the board where it reaches furthest toward it: the long point, if that
    /// end is mitered. Along the fence, the piece against the stop comes out exactly `length`
    /// long whatever the miter angle.
    ///
    /// The blade's center swings along an arc on the way down; within the board, the circle at
    /// its lowest takes all the wood the higher ones did, so the scar is that one circle.
    ///
    /// Panics if the miter is past `max_miter`.
    pub fn chop(
        &self,
        board: &Board,
        lie: Orientation,
        length: Length,
        bottom: Length,
    ) -> Result<Pass, CutError> {
        assert!(
            self.miter.abs() <= self.max_miter,
            "the table turns {} either way, not {}",
            self.max_miter,
            self.miter
        );
        if lie.on_edge() {
            return self.chop_on_edge(board, lie, length, bottom);
        }
        let (across, across_backward) = lie.across();
        let span = across.of(board);
        let (along, _) = lie.along();
        let total = along.of(board);
        if length <= Length::ZERO {
            return Err(CutError::NonPositiveSetting(length));
        }
        // The blade's stop-side face crosses the fence `length` from the stop, at the board's
        // far end, and swings out from there at the miter angle. The blade's center is out in
        // front of the fence along it.
        let (cos, sin) = self.miter.unit();
        let toward_stop = lie.direction_onto_board(sin, cos);
        let outward = lie.direction_onto_board(-cos, sin);
        let (l, w) = lie.point_onto_board(board, span, total - length);
        let at = |direction: Direction| Length::from_nm(direction.dot(l, w) as i64);
        let face = at(toward_stop);
        reaches_blade([(board, toward_stop, face)], length, self.miter)?;
        let kerf_width = scaled(self.blade.kerf, toward_stop);
        let (out, height) = self.center(bottom);
        let middle = at(outward) + scaled(out, outward);
        let kerf = Kerf::new(
            Across::Face(toward_stop),
            (face - kerf_width, face),
            outward,
            (middle, middle),
            self.blade.radius(),
            if lie.up().1 {
                board.thickness() - height
            } else {
                height
            },
        );
        if !board.touched_by(&kerf) {
            return Ok(Pass::Missed);
        }
        let tool = Tool::MiterSaw {
            saw: *self,
            lie: Lie::Board(lie),
            length,
            bottom,
        };
        let (kerfed, pieces) = board.sawn(kerf, &Operation::new(tool));
        if let [whole] = pieces.as_slice()
            && whole.board == kerfed
        {
            return Ok(Pass::Kerfed(kerfed));
        }
        // Along the fence, a slanted kerf is wider than the blade.
        let along_fence = Length::from_nm(
            (self.blade.kerf.as_nm() as f64 / self.miter.radians().cos()).round() as i64,
        );
        let slot = (total - length - along_fence, total - length);
        let waste = (slot.1.min(total) - slot.0.max(Length::ZERO)).max(Length::ZERO);
        let toward_fence = |piece: &Piece| on_table(piece, across, span, across_backward).1;
        // Each piece is wholly on one side of the slot, so any corner of it says which.
        let slot_middle = 2 * face.as_nm() as i128 - kerf_width.as_nm() as i128;
        let measured = |piece: &Piece| {
            let [l, w, _] = piece.board.corners()[0];
            2 * toward_stop.dot(piece.l + l, piece.w + w) > slot_middle
        };
        Ok(Pass::Cut(sort_out(pieces, measured, toward_fence, waste)))
    }

    /// Brings the blade down on a glue-up, the way `chop` does on a board, with `lie` laying its
    /// axes onto the saw's: along the fence toward the stop, across the table toward the fence,
    /// and up, as `x`, `y`, `z`. Every board in it has to lie flat.
    ///
    /// Panics if the miter is past `max_miter`.
    pub fn chop_glue_up(
        &self,
        glue_up: &Assembly,
        lie: Rotation,
        length: Length,
        bottom: Length,
    ) -> Result<GlueUpPass, CutError> {
        assert!(
            self.miter.abs() <= self.max_miter,
            "the table turns {} either way, not {}",
            self.max_miter,
            self.miter
        );
        let laid = Laid::new(glue_up, lie)?;
        let (total, span) = (laid.extent[0], laid.extent[1]);
        if length <= Length::ZERO {
            return Err(CutError::NonPositiveSetting(length));
        }
        // As for a board: the blade's stop-side face crosses the fence `length` from the stop,
        // and its center is out in front of the fence along it.
        let (cos, sin) = self.miter.unit();
        let (out, height) = self.center(bottom);
        let at = [total - length, span];
        let path = Path {
            tool: Tool::MiterSaw {
                saw: *self,
                lie: Lie::GlueUp(lie),
                length,
                bottom,
            },
            across: (cos, sin),
            face: at,
            kerf: self.blade.kerf,
            through: at,
            travel: (out, out),
            height,
            radius: self.blade.radius(),
            measured_ahead: true,
        };
        reaches_blade(laid.slot_faces(&path), length, self.miter)?;
        Ok(laid.cut(&path))
    }

    /// Chops a glue-up with a full stroke, through the table.
    pub fn crosscut_glue_up(
        &self,
        glue_up: &Assembly,
        lie: Rotation,
        length: Length,
    ) -> Result<GlueUpPass, CutError> {
        self.chop_glue_up(glue_up, lie, length, self.stroke.1)
    }

    /// Chops a board standing on an edge, square only: with its length along the fence, the
    /// blade comes down through its width; turned with its length across the saw, parallel to
    /// its faces.
    fn chop_on_edge(
        &self,
        board: &Board,
        lie: Orientation,
        length: Length,
        bottom: Length,
    ) -> Result<Pass, CutError> {
        if self.miter != Angle::ZERO {
            return Err(CutError::NotModeled(
                "the table has to be square to cut a board standing on edge",
            ));
        }
        let (across, across_backward) = lie.across();
        let (along, along_backward) = lie.along();
        let (up, up_backward) = lie.up();
        let (span, total) = (across.of(board), along.of(board));
        if length <= Length::ZERO {
            return Err(CutError::NonPositiveSetting(length));
        }
        if length >= total {
            return Err(CutError::MissesBoard {
                setting: length,
                available: total,
            });
        }
        let slot = (total - length - self.blade.kerf, total - length);
        let (near, far) = Orientation::onto_board(slot, total, along_backward);
        // Where the blade's center is on the board: out in front of the fence, and up.
        let (out, height) = self.center(bottom);
        let (out, _) = Orientation::onto_board((span - out, span - out), span, across_backward);
        let height = if up_backward {
            up.of(board) - height
        } else {
            height
        };
        let radius = self.blade.radius();
        let kerf = match along {
            Axis::Length => Kerf::new(
                Across::Face(Direction::LENGTH),
                (near, far),
                Direction::WIDTH,
                (height, height),
                radius,
                out,
            ),
            _ => Kerf::new(
                Across::Thickness,
                (near, far),
                Direction::of(across),
                (out, out),
                radius,
                height,
            ),
        };
        if !board.touched_by(&kerf) {
            return Ok(Pass::Missed);
        }
        let tool = Tool::MiterSaw {
            saw: *self,
            lie: Lie::Board(lie),
            length,
            bottom,
        };
        let (kerfed, pieces) = board.sawn(kerf, &Operation::new(tool));
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

/// `length` times the scale of `direction`, as positions along it are measured.
/// Errs if none of `board` is beyond `face`, a tilted blade's face, the way it faces: the
/// setting's too long. The error says the setting that would just reach it: moving the
/// setting moves the face square to itself by `cosines` as much.
fn reaches_beyond(
    board: &Board,
    face: Edge,
    setting: Length,
    cosines: f64,
) -> Result<(), CutError> {
    if board.reaches_beyond(face) {
        return Ok(());
    }
    let normal = ((face.out.scale_squared() + i128::from(face.lean).pow(2)) as f64).sqrt();
    let furthest = board
        .corners()
        .into_iter()
        .map(|[l, w, t]| face.out.dot(l, w) as f64 + face.lean as f64 * t.as_nm() as f64)
        .fold(f64::MIN, f64::max);
    let short = (face.at as f64 - furthest) / normal / cosines;
    Err(CutError::MissesBoard {
        setting,
        available: setting - Length::from_nm(short.round() as i64),
    })
}

/// Errs if the blade would pass wholly beyond the boards: the setting's too long. Each board
/// comes with the blade's stop-side face, at a position along a direction pointing back toward
/// the stop, in its own coordinates. A slanted blade can still cross a mitered end even with
/// the setting past the box around the board, so this looks at the boards' corners, exactly.
/// The error says the setting that would just reach them: moving the setting moves the blade's
/// face square to itself by the cosine of `angle`.
fn reaches_blade<'a>(
    boards: impl IntoIterator<Item = (&'a Board, Direction, Length)>,
    setting: Length,
    angle: Angle,
) -> Result<(), CutError> {
    let mut nearest = f64::INFINITY;
    for (board, toward_stop, face) in boards {
        if board.reaches_below(toward_stop, face) {
            return Ok(());
        }
        let furthest = board
            .face()
            .iter()
            .map(|region| region.solid(board.thickness()).extent(toward_stop).0)
            .fold(f64::INFINITY, f64::min);
        nearest = nearest.min((furthest - face.as_nm() as f64) / toward_stop.scale());
    }
    let short = nearest / angle.radians().cos();
    Err(CutError::MissesBoard {
        setting,
        available: setting - Length::from_nm(short.round() as i64),
    })
}

pub(crate) fn scaled(length: Length, direction: Direction) -> Length {
    Length::from_nm((length.as_nm() as f64 * direction.scale()).round() as i64)
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
/// measured side of the slot, the offcut the biggest on the other, and any more are loose. If
/// no wood is on the measured side, the blade only trimmed the board: the biggest piece is
/// the piece, and there's no offcut.
fn sort_out(
    pieces: Vec<Piece>,
    measured: impl Fn(&Piece) -> bool,
    toward_fence: impl Fn(&Piece) -> Length + Copy,
    waste: Length,
) -> Cut {
    let (mut near, mut far): (Vec<Piece>, Vec<Piece>) = pieces.into_iter().partition(measured);
    if near.is_empty() {
        std::mem::swap(&mut near, &mut far);
    }
    let mut piece = take_biggest(&mut near, toward_fence).expect("a cut leaves some wood");
    let mut offcut = take_biggest(&mut far, toward_fence);
    near.append(&mut far);
    piece.board.set_side(Side::Measured);
    if let Some(offcut) = &mut offcut {
        offcut.board.set_side(Side::Offcut);
    }
    Cut {
        piece,
        offcut,
        waste,
        loose: near,
    }
}

/// How much room a piece takes up, as the box around it.
fn size(piece: &Piece) -> i128 {
    let board = &piece.board;
    [board.length(), board.width(), board.thickness()]
        .into_iter()
        .map(|side| side.as_nm() as i128)
        .product()
}

/// Removes and returns the biggest piece, the one nearest the fence among equals.
fn take_biggest(pieces: &mut Vec<Piece>, toward_fence: impl Fn(&Piece) -> Length) -> Option<Piece> {
    let index = (0..pieces.len()).max_by_key(|&i| (size(&pieces[i]), toward_fence(&pieces[i])))?;
    Some(pieces.remove(index))
}
