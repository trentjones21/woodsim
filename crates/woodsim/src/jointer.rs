//! A jointer: a cutterhead spinning in the gap between two tables, the top of its knives'
//! circle level with the outfeed table and the infeed table lower by the depth of cut. Wood fed
//! across it against the fence comes off that much thinner wherever it lay on the table: a face,
//! an edge, or whatever it rests on. With the fence tilted, wood leans with it on its lowest
//! edge, and each pass takes a little more off that edge until it's beveled.
//!
//! The cutterhead is a cylinder as long as the knives, square across the tables, so it cuts as
//! a round blade as wide as the knives would: a slot whose faces are the knives' end and, past
//! the fence, wherever, and whose blade is the knives' circle. Fed all the way across, that
//! slot takes all the wood below the outfeed table, and the wood is trimmed there
//! (`Board::pieces`); stopped partway, it leaves a step, curved as the circle is. Wood that
//! reaches past the knives' end isn't cut there, so a board wider than the fence is from their
//! end comes out rabbeted along its edge against the fence.

use crate::{
    Angle, Assembly, Board, CutError, Length, Orientation, Pass, Rotation,
    glue_up::{GlueUpPass, Laid, Sweep, alone},
    history::{Lie, Tool},
    saw::one_board,
};

/// As far as the fence tilts, its top leaning out over the tables.
pub const MAX_FENCE_TILT: Angle = Angle::from_degrees(45);

/// The knives run on past the fence, under it, at least this far: no wood is ever there, so
/// the slot just takes in everything against the fence.
const BEHIND_FENCE: Length = Length::from_inches(1);

/// A jointer, as it's set. Heights are above the infeed table, where the wood's lowest point
/// rests; across the tables is toward the fence, along them the way the wood's fed. The fence
/// stands on the outfeed table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Jointer {
    /// The circle the knives' edges turn in, across.
    pub cutterhead: Length,
    /// How long the knives are, across the tables: the widest it joints in one pass.
    pub knives: Length,
    /// How far the infeed table is below the outfeed table and the top of the knives' circle:
    /// how much a pass takes off.
    pub depth: Length,
    /// How far the fence is from the knives' end, across the tables, where it meets the outfeed
    /// table: how much of the knives cuts. Wood reaching past their end isn't cut there.
    pub fence: Length,
    /// How far the fence tilts from square to the tables, its top leaning out over them, up to
    /// `MAX_FENCE_TILT`. Wood lying against it leans with it, until it rests on the tables on
    /// its lowest edge.
    pub tilt: Angle,
}

impl Jointer {
    /// A 6" jointer with a 2-1/2" cutterhead, its fence all the way back, set to take 1/32".
    pub fn six_inch() -> Jointer {
        Jointer {
            cutterhead: Length::from_inch_fraction(5, 2),
            knives: Length::from_inches(6),
            depth: Length::from_inch_fraction(1, 32),
            fence: Length::from_inches(6),
            tilt: Angle::ZERO,
        }
    }

    /// The radius of the knives' circle.
    pub fn radius(&self) -> Length {
        Length::from_nm(self.cutterhead.as_nm() / 2)
    }

    /// How wood that lies against the fence as `lie` lays it (along the tables, across them
    /// toward the fence, and up, as `x`, `y`, `z`) lies once it leans with the fence: turned
    /// about the tables' length, so what was toward the fence faces it, tilted.
    pub fn leaned(&self, lie: Rotation) -> Rotation {
        if self.tilt == Angle::ZERO {
            return lie;
        }
        lie.then(Rotation::about([1.0, 0.0, 0.0], self.tilt))
    }

    /// Feeds `board`, lying against the fence as `lie` says, across the cutterhead while the
    /// cutterhead's center travels from `start` to `end` along it, measured from its leading
    /// end, as `TableSaw::feed` has it. All the way across, the board comes out `depth`
    /// thinner where it lay on the table, the rest of it the piece and no offcut; stopped
    /// partway, or reaching past the knives' end, it's left in one piece with a step in it.
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
        let laid = Laid::new(&alone, self.leaned(lie.rotation()));
        let sweep = self.sweep(&laid, Lie::Board(lie), (start, end))?;
        let waste = self.depth.min(laid.extent[2]);
        Ok(one_board(laid.cut(&sweep)?, waste))
    }

    /// Feeds `board`, lying as `lie` says, all the way across the cutterhead.
    pub fn joint(&self, board: &Board, lie: Orientation) -> Result<Pass, CutError> {
        let radius = self.radius();
        self.feed(board, lie, -radius, self.reach(board, lie) + radius)
    }

    /// How far `board`, lying as `lie` says, reaches along the tables.
    pub fn reach(&self, board: &Board, lie: Orientation) -> Length {
        let alone = alone(board);
        Laid::new(&alone, self.leaned(lie.rotation())).extent[0]
    }

    /// Where the knives end, across the tables from the near side of the box what lies against
    /// the fence as `lie` lays it fills, leaning with it: `fence` in from where the fence meets
    /// the outfeed table. Square, the fence stands at the wood's far side; tilted, it leans on
    /// the wood above where it meets the tables, and on down past the outfeed table to the
    /// infeed table, lower by `depth`, where the wood rests.
    pub fn knives_end(&self, glue_up: &Assembly, lie: Rotation) -> Length {
        self.knives_end_of(&Laid::new(glue_up, self.leaned(lie)))
    }

    fn knives_end_of(&self, laid: &Laid) -> Length {
        if self.tilt == Angle::ZERO {
            return laid.extent[1] - self.fence;
        }
        // The fence's face, square to the way it leans, through where the wood reaches it.
        let (sin, cos) = self.tilt.radians().sin_cos();
        let against = laid.furthest([0.0, cos, sin]);
        let on_outfeed = (against - sin * self.depth.as_nm() as f64) / cos;
        Length::from_nm(on_outfeed.round() as i64) - self.fence
    }

    /// Feeds a glue-up across the cutterhead, the way `feed` does a board, with `lie` laying its
    /// axes against the fence (along the tables, across them toward the fence, and up, as `x`,
    /// `y`, `z`) before it leans with the fence.
    ///
    /// Panics if `start` is past `end`.
    pub fn feed_glue_up(
        &self,
        glue_up: &Assembly,
        lie: Rotation,
        start: Length,
        end: Length,
    ) -> Result<GlueUpPass, CutError> {
        let laid = Laid::new(glue_up, self.leaned(lie));
        let sweep = self.sweep(&laid, Lie::GlueUp(lie), (start, end))?;
        laid.cut(&sweep)
    }

    /// Feeds a glue-up, lying as `lie` lays it, all the way across the cutterhead.
    pub fn joint_glue_up(&self, glue_up: &Assembly, lie: Rotation) -> Result<GlueUpPass, CutError> {
        let radius = self.radius();
        let reach = Laid::new(glue_up, self.leaned(lie)).extent[0];
        self.feed_glue_up(glue_up, lie, -radius, reach + radius)
    }

    /// Where the knives sweep through what `laid` lays on the tables, in its frame: along the
    /// tables, across them toward the fence, and up, from the near corner of the box it fills.
    /// The fence stands, or leans, against the wood's far side, and meets the tables where the
    /// knives' cutting starts `fence` back toward their end.
    pub(crate) fn sweep(
        &self,
        laid: &Laid,
        lie: Lie,
        (start, end): (Length, Length),
    ) -> Result<Sweep, CutError> {
        assert!(
            start <= end,
            "the cutterhead's travel runs backward: {start} to {end}"
        );
        assert!(
            Angle::ZERO <= self.tilt && self.tilt <= MAX_FENCE_TILT,
            "the fence tilts from square to {MAX_FENCE_TILT}, not {}",
            self.tilt
        );
        for setting in [self.depth, self.fence] {
            if setting < Length::ZERO {
                return Err(CutError::NegativeSetting(setting));
            }
        }
        let end_of_knives = self.knives_end_of(laid);
        let radius = self.radius();
        Ok(Sweep {
            tool: Tool::Jointer {
                jointer: *self,
                lie,
                travel: (start, end),
            },
            normal: [0, 1, 0],
            face: [Length::ZERO, end_of_knives, Length::ZERO],
            kerf: self.fence + BEHIND_FENCE,
            measured_ahead: false,
            along: [1, 0, 0],
            travel: (start, end),
            through: [Length::ZERO, end_of_knives, self.depth - radius],
            radius,
            split: Some(([0, 0, 1], self.depth)),
        })
    }
}
