use std::fmt;

use crate::{
    Angle, Assembly, Board, Direction, Length, Orientation, Piece, Rotation,
    glue_up::{GlueUpPass, Laid, Sweep, alone, level},
    history::{Lie, Tool},
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
    /// With no wood there, as when the blade is right up against the fence or the stop, it's
    /// what's left on the blade's other side, and `offcut` is `None`.
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
    /// The fence or cut length is negative. Zero puts the blade right up against the fence or
    /// the stop.
    NegativeSetting(Length),
    /// The setting is at or past the board's far edge, so the blade never touches it.
    MissesBoard { setting: Length, available: Length },
    /// The blade would take all the wood there is: a board no wider than the kerf, the blade
    /// right up against the fence or the stop.
    AllSawdust,
}

impl fmt::Display for CutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CutError::NegativeSetting(setting) => {
                write!(f, "the saw can't be set to {setting}")
            }
            CutError::AllSawdust => write!(f, "the blade would turn all of it to sawdust"),
            CutError::MissesBoard { setting, available } => write!(
                f,
                "the blade misses the board: set to {setting}, but the board is only {available}"
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
    /// the setting, where it lay on the table.
    ///
    /// Panics if `start` is past `end`.
    pub fn feed(
        &self,
        board: &Board,
        lie: Orientation,
        start: Length,
        end: Length,
    ) -> Result<Pass, CutError> {
        let alone = alone(board);
        let laid = Laid::new(&alone, lie.rotation());
        let (sweep, waste) = self.sweep(&laid, Lie::Board(lie), (start, end))?;
        Ok(one_board(laid.cut(&sweep)?, waste))
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
        let alone = alone(board);
        self.reach_of(&Laid::new(&alone, lie.rotation()))
    }

    /// How far what's laid on the saw reaches along the blade's travel.
    fn reach_of(&self, laid: &Laid) -> Length {
        let travel = self.travel();
        let along: Vec<i128> = laid
            .box_corners()
            .into_iter()
            .map(|[x, y, _]| travel.dot(x, y))
            .collect();
        let (low, high) = (along.iter().min().unwrap(), along.iter().max().unwrap());
        Length::from_nm(((high - low) as f64 / travel.scale()).ceil() as i64)
    }

    /// How far the miter gauge's head is turned, if it's guiding the board.
    fn angle(&self) -> Angle {
        match self.miter_gauge {
            Some(_) => self.gauge_angle,
            None => Angle::ZERO,
        }
    }

    /// The way the blade's center travels along the table, as the board sees it: straight
    /// along it, or turned with the gauge's head. Whole numbers along the table and across it.
    fn travel(&self) -> Direction {
        let (cos, sin) = self.angle().unit();
        Direction::new(cos, sin)
    }

    /// Where the blade sweeps through what `laid` lays on the table, in its frame, and where
    /// the slot is along the setting's way across the table, for what a board loses to
    /// sawdust. The blade stands square to the table, or leans away from the fence going up,
    /// about where it meets the table; the gauge turns the board, so the blade crosses it at
    /// the gauge's angle.
    fn sweep(
        &self,
        laid: &Laid,
        lie: Lie,
        (start, end): (Length, Length),
    ) -> Result<(Sweep, Length), CutError> {
        assert!(
            start <= end,
            "the blade's travel runs backward: {start} to {end}"
        );
        assert!(
            self.gauge_angle.abs() <= MAX_GAUGE,
            "the gauge turns {MAX_GAUGE} either way, not {}",
            self.gauge_angle
        );
        assert!(
            Angle::ZERO <= self.tilt && self.tilt <= MAX_TILT,
            "the blade tilts from square to {MAX_TILT}, not {}",
            self.tilt
        );
        let [depth, span, _] = laid.extent;
        let (cos, sin) = self.angle().unit();
        // Toward the fence, across the blade's path, tilted.
        let (out, lean) = Direction::new(-sin, cos).tilted(self.tilt);
        let normal = [out.l, out.w, lean];
        let size = ((out.scale_squared() + i128::from(lean).pow(2)) as f64).sqrt();
        let upright = out.scale() / size;
        // Across the table, square to the path, the slot is wider by as much as it leans.
        let across = self.blade.kerf.as_nm() as f64 / upright;
        let (face, measured_ahead, setting, slot) = match self.miter_gauge {
            // The stop-side face crosses the gauge's face, at the board's trailing side, `stop`
            // from the stop, where the board lies on the table.
            Some(stop) => {
                let along_gauge = across / self.angle().radians().cos();
                (
                    [depth, stop, Length::ZERO],
                    false,
                    stop,
                    (stop, stop + Length::from_nm(along_gauge.round() as i64)),
                )
            }
            // The fence-side face is `fence` from the fence, where the board lies on the table.
            None => {
                let face = span - self.fence;
                (
                    [Length::ZERO, face, Length::ZERO],
                    true,
                    self.fence,
                    (face - Length::from_nm(across.round() as i64), face),
                )
            }
        };
        if setting < Length::ZERO {
            return Err(CutError::NegativeSetting(setting));
        }
        // The blade's travel, from the board's leading corner.
        let travel = self.travel();
        let lead = laid
            .box_corners()
            .into_iter()
            .map(|[x, y, _]| travel.dot(x, y))
            .min()
            .expect("a box has corners");
        let lead = Length::from_nm(lead as i64);
        let from = |at: Length| {
            lead + Length::from_nm((at.as_nm() as f64 * travel.scale()).round() as i64)
        };
        // Its teeth reach `height` up, at the top of a circle tilted from upright; its center
        // is on the plane halfway between its faces.
        let radius = self.blade.radius();
        let height =
            self.height - Length::from_nm((radius.as_nm() as f64 * upright).round() as i64);
        let through = middle_at(normal, face, self.blade.kerf, measured_ahead, height);
        let sweep = Sweep {
            tool: Tool::TableSaw {
                saw: *self,
                lie,
                travel: (start, end),
            },
            normal,
            face,
            kerf: self.blade.kerf,
            measured_ahead,
            along: [travel.l, travel.w, 0],
            travel: (from(start), from(end)),
            through,
            radius,
            split: None,
        };
        reaches(
            laid,
            &sweep,
            setting,
            self.angle().radians().cos() * upright,
        )?;
        let waste = (slot.1.min(span) - slot.0.max(Length::ZERO)).max(Length::ZERO);
        Ok((sweep, waste))
    }

    /// Feeds a glue-up past the blade, the way `feed` does a board, with `lie` laying its axes
    /// onto the table's: along it, across it toward the fence, and up, as `x`, `y`, `z` (as a
    /// board's `Orientation::rotation` lays a board).
    ///
    /// Panics if `start` is past `end`.
    pub fn feed_glue_up(
        &self,
        glue_up: &Assembly,
        lie: Rotation,
        start: Length,
        end: Length,
    ) -> Result<GlueUpPass, CutError> {
        let laid = Laid::new(glue_up, lie);
        let (sweep, _) = self.sweep(&laid, Lie::GlueUp(lie), (start, end))?;
        laid.cut(&sweep)
    }

    /// Pushes a glue-up, lying as `lie` lays it, all the way past the blade.
    pub fn rip_glue_up(&self, glue_up: &Assembly, lie: Rotation) -> Result<GlueUpPass, CutError> {
        let radius = self.blade.radius();
        let reach = self.reach_of(&Laid::new(glue_up, lie));
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
        let alone = alone(board);
        let laid = Laid::new(&alone, lie.rotation());
        let tool = Tool::MiterSaw {
            saw: *self,
            lie: Lie::Board(lie),
            length,
            bottom,
        };
        let (sweep, waste) = self.sweep(&laid, tool, length, bottom)?;
        Ok(one_board(laid.cut(&sweep)?, waste))
    }

    /// Where the blade sweeps through what `laid` lays on the saw, in its frame: along the fence
    /// toward the stop, across the table toward the fence, and up. Its stop-side face crosses
    /// the fence `length` from the stop, at the far end of what's on it, and swings out from
    /// there at the miter angle; its center is out in front of the fence along it, where the
    /// arm brings it down to. Also where the slot is along the fence, for what a board loses to
    /// sawdust.
    fn sweep(
        &self,
        laid: &Laid,
        tool: Tool,
        length: Length,
        bottom: Length,
    ) -> Result<(Sweep, Length), CutError> {
        assert!(
            self.miter.abs() <= self.max_miter,
            "the table turns {} either way, not {}",
            self.max_miter,
            self.miter
        );
        if length < Length::ZERO {
            return Err(CutError::NegativeSetting(length));
        }
        let [total, span, _] = laid.extent;
        let (cos, sin) = self.miter.unit();
        let toward_stop = Direction::new(cos, sin);
        let outward = Direction::new(sin, -cos);
        let face = [total - length, span, Length::ZERO];
        let (out, height) = self.center(bottom);
        let at = outward.dot(face[0], face[1]) + scaled(out, outward).as_nm() as i128;
        let at = Length::from_nm(at as i64);
        let reach = out.as_nm() as f64 / outward.scale();
        let through = [
            face[0] + Length::from_nm((outward.l as f64 * reach).round() as i64),
            face[1] + Length::from_nm((outward.w as f64 * reach).round() as i64),
            height,
        ];
        let sweep = Sweep {
            tool,
            normal: level(toward_stop),
            face,
            kerf: self.blade.kerf,
            measured_ahead: true,
            along: level(outward),
            travel: (at, at),
            through,
            radius: self.blade.radius(),
            split: None,
        };
        reaches(laid, &sweep, length, self.miter.radians().cos())?;
        // Along the fence, a slanted kerf is wider than the blade.
        let along_fence = Length::from_nm(
            (self.blade.kerf.as_nm() as f64 / self.miter.radians().cos()).round() as i64,
        );
        let slot = (total - length - along_fence, total - length);
        let waste = (slot.1.min(total) - slot.0.max(Length::ZERO)).max(Length::ZERO);
        Ok((sweep, waste))
    }

    /// Brings the blade down on a glue-up, the way `chop` does on a board, with `lie` laying its
    /// axes onto the saw's: along the fence toward the stop, across the table toward the fence,
    /// and up, as `x`, `y`, `z`.
    ///
    /// Panics if the miter is past `max_miter`.
    pub fn chop_glue_up(
        &self,
        glue_up: &Assembly,
        lie: Rotation,
        length: Length,
        bottom: Length,
    ) -> Result<GlueUpPass, CutError> {
        let laid = Laid::new(glue_up, lie);
        let tool = Tool::MiterSaw {
            saw: *self,
            lie: Lie::GlueUp(lie),
            length,
            bottom,
        };
        let (sweep, _) = self.sweep(&laid, tool, length, bottom)?;
        laid.cut(&sweep)
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

/// Errs if the blade's measured face is past all of what's laid on the saw: the setting's too
/// long. The error says the setting that would just reach it: moving the setting moves the
/// face square to itself by `cosines` as much.
fn reaches(laid: &Laid, sweep: &Sweep, setting: Length, cosines: f64) -> Result<(), CutError> {
    match laid.short_of(sweep) {
        None => Ok(()),
        Some(short) => Err(CutError::MissesBoard {
            setting,
            available: setting - Length::from_nm((short / cosines).round() as i64),
        }),
    }
}

/// A point on the plane halfway between a blade's faces, `height` up, square to `face` across
/// the faces: where its center's path is.
fn middle_at(
    normal: [i64; 3],
    face: [Length; 3],
    kerf: Length,
    measured_ahead: bool,
    height: Length,
) -> [Length; 3] {
    let n = normal.map(|v| v as f64);
    let size = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    let across = n[0].hypot(n[1]);
    let half = kerf.as_nm() as f64 / 2.0 * if measured_ahead { -1.0 } else { 1.0 };
    // Over the face's point on the table, then across to the middle at that height.
    let off = (half * size - n[2] * height.as_nm() as f64) / across;
    [
        face[0] + Length::from_nm((off * n[0] / across).round() as i64),
        face[1] + Length::from_nm((off * n[1] / across).round() as i64),
        height,
    ]
}

/// What a pass through a glue-up of one board did to the board.
pub(crate) fn one_board(pass: GlueUpPass, waste: Length) -> Pass {
    let piece = |part: Assembly| {
        let member = &part.members()[0];
        Piece {
            board: member.board.clone(),
            l: member.at[0],
            w: member.at[1],
            t: member.at[2],
        }
    };
    match pass {
        GlueUpPass::Missed => Pass::Missed,
        GlueUpPass::Kerfed(part) => Pass::Kerfed(part.members()[0].board.clone()),
        GlueUpPass::Cut(cut) => Pass::Cut(Cut {
            piece: piece(cut.piece),
            offcut: cut.offcut.map(piece),
            waste,
            loose: cut.loose.into_iter().map(piece).collect(),
        }),
    }
}

pub(crate) fn scaled(length: Length, direction: Direction) -> Length {
    Length::from_nm((length.as_nm() as f64 * direction.scale()).round() as i64)
}
