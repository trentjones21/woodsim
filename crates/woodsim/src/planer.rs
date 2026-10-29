//! A thickness planer: a cutterhead above a bed, the bottom of its knives' circle set the
//! thickness wood's to come out, and rollers that carry the wood through under it. Whatever
//! the wood rests on lies flat on the bed, and the knives take everything above the setting
//! off its top, so it comes out that thick, its top parallel to what it rested on.
//!
//! As the jointer's is (`jointer.rs`), the cutterhead is a cylinder as long as its knives,
//! square across the bed, cutting as a round blade as wide as the knives would, here from
//! above: a slot whose faces are the knives' ends, its blade the knives' circle, its bottom
//! the setting above the bed. Fed all the way through, that slot takes all the wood above the
//! setting, and the wood is trimmed there (`Board::pieces`). Every size of planer is the same
//! machine with other numbers: how wide its knives are, how thick and thin it goes, and how
//! much it takes in a pass.

use crate::{
    Assembly, Board, CutError, Length, Orientation, Pass, Rotation,
    glue_up::{GlueUpPass, Laid, Sweep, alone},
    history::{Lie, Tool},
    saw::one_board,
};

/// A thickness planer, what it is and how it's set. Heights are above the bed, where the
/// wood's lowest point rests; across the bed is square to the way the wood's fed, along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Planer {
    /// The circle the knives' edges turn in, across.
    pub cutterhead: Length,
    /// How long the knives are, across the bed: the widest wood that goes through.
    pub knives: Length,
    /// How thin it planes, the head lowered as far as it goes, and how thick wood it takes,
    /// raised as far.
    pub thinnest: Length,
    pub thickest: Length,
    /// The most it takes off in one pass: wood thicker than that over the setting won't go
    /// through.
    pub deepest: Length,
    /// How high the bottom of the knives' circle is above the bed: how thick a pass leaves the
    /// wood, from `thinnest` to `thickest`.
    pub thickness: Length,
}

impl Planer {
    /// A 13" benchtop planer: a 2" cutterhead, from 1/8" to 6" thick, taking up to 1/8" a
    /// pass, set to 3/4".
    pub fn thirteen_inch() -> Planer {
        Planer {
            cutterhead: Length::from_inches(2),
            knives: Length::from_inches(13),
            thinnest: Length::from_inch_fraction(1, 8),
            thickest: Length::from_inches(6),
            deepest: Length::from_inch_fraction(1, 8),
            thickness: Length::from_inch_fraction(3, 4),
        }
    }

    /// The radius of the knives' circle.
    pub fn radius(&self) -> Length {
        Length::from_nm(self.cutterhead.as_nm() / 2)
    }

    /// Feeds `board`, lying as `lie` says, under the cutterhead while the cutterhead's center
    /// travels from `start` to `end` along it, measured from its leading end, as
    /// `TableSaw::feed` has it. All the way through, it comes out `thickness` thick where it
    /// lay on the bed, the measured piece; stopped partway, it's left in one piece with a step
    /// in its top. Refuses wood too wide to go through (`TooWide`), or thicker over the setting
    /// than a pass takes (`TooDeep`).
    ///
    /// Panics if `start` is past `end`, or the head's set past where it goes.
    pub fn feed(
        &self,
        board: &Board,
        lie: Orientation,
        start: Length,
        end: Length,
    ) -> Result<Pass, CutError> {
        let alone = alone(board);
        let laid = Laid::new(&alone, lie.rotation());
        let sweep = self.sweep(&laid, Lie::Board(lie), (start, end))?;
        let waste = (laid.extent[2] - self.thickness).max(Length::ZERO);
        Ok(one_board(laid.cut(&sweep)?, waste))
    }

    /// Feeds `board`, lying as `lie` says, all the way through.
    pub fn plane(&self, board: &Board, lie: Orientation) -> Result<Pass, CutError> {
        let radius = self.radius();
        self.feed(board, lie, -radius, self.reach(board, lie) + radius)
    }

    /// How far `board`, lying as `lie` says, reaches along the bed.
    pub fn reach(&self, board: &Board, lie: Orientation) -> Length {
        let alone = alone(board);
        Laid::new(&alone, lie.rotation()).extent[0]
    }

    /// How much a pass takes off `board`, lying as `lie` says: how far it reaches up from the
    /// bed past the setting, or nothing.
    pub fn takes(&self, board: &Board, lie: Orientation) -> Length {
        self.takes_glue_up(&alone(board), lie.rotation())
    }

    /// How much a pass takes off a glue-up, lying as `lie` lays it, as `takes` has it.
    pub fn takes_glue_up(&self, glue_up: &Assembly, lie: Rotation) -> Length {
        let laid = Laid::new(glue_up, lie);
        (laid.extent[2] - self.thickness).max(Length::ZERO)
    }

    /// Feeds a glue-up under the cutterhead, the way `feed` does a board, with `lie` laying its
    /// axes on the bed (along it, across it, and up, as `x`, `y`, `z`).
    ///
    /// Panics if `start` is past `end`, or the head's set past where it goes.
    pub fn feed_glue_up(
        &self,
        glue_up: &Assembly,
        lie: Rotation,
        start: Length,
        end: Length,
    ) -> Result<GlueUpPass, CutError> {
        let laid = Laid::new(glue_up, lie);
        let sweep = self.sweep(&laid, Lie::GlueUp(lie), (start, end))?;
        laid.cut(&sweep)
    }

    /// Feeds a glue-up, lying as `lie` lays it, all the way through.
    pub fn plane_glue_up(&self, glue_up: &Assembly, lie: Rotation) -> Result<GlueUpPass, CutError> {
        let radius = self.radius();
        let reach = Laid::new(glue_up, lie).extent[0];
        self.feed_glue_up(glue_up, lie, -radius, reach + radius)
    }

    /// Where the knives sweep through what `laid` lays on the bed, in its frame: along the bed,
    /// across it, and up, from the near corner of the box it fills. The wood goes through
    /// wherever it fits across the bed, so the knives' ends are either side of it.
    fn sweep(
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
            self.thinnest <= self.thickness && self.thickness <= self.thickest,
            "the head goes from {} to {}, not {}",
            self.thinnest,
            self.thickest,
            self.thickness
        );
        let [_, across, high] = laid.extent;
        if across > self.knives {
            return Err(CutError::TooWide {
                across,
                widest: self.knives,
            });
        }
        if high - self.thickness > self.deepest {
            return Err(CutError::TooDeep {
                off: high - self.thickness,
                most: self.deepest,
            });
        }
        let radius = self.radius();
        let end_of_knives = Length::from_nm((across - self.knives).as_nm() / 2);
        Ok(Sweep {
            tool: Tool::Planer {
                planer: *self,
                lie,
                travel: (start, end),
            },
            normal: [0, 1, 0],
            face: [Length::ZERO, end_of_knives, Length::ZERO],
            kerf: self.knives,
            measured_ahead: false,
            along: [1, 0, 0],
            travel: (start, end),
            through: [Length::ZERO, end_of_knives, self.thickness + radius],
            radius,
            // What's left is below the knives, the setting thick: the measured piece.
            split: Some(([0, 0, 1], self.thickness)),
        })
    }
}
