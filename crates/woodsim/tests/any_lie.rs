//! One way of cutting for everything: a board or a glue-up, lying any of the 24 square ways on
//! either saw (on edge, on end, upside down), cut with the blade at any angle and tilt, stopped
//! or all the way through, keeps exactly the wood the blade didn't take, in exactly one piece,
//! and every piece replays from its stock.

use proptest::prelude::*;
use woodsim::{
    Angle, Assembly, Blade, Board, CutError, Fastener, GlueUpPass, Length, Mating, Member,
    MiterSaw, Rotation, Source, Surface, TableSaw, surfaces,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

/// Every square way to lie on a saw: each of a board's six sides down, turned each of four
/// ways about the saw's up.
fn square_lies() -> Vec<Rotation> {
    let mut lies = Vec::new();
    for (axis, turns) in [(0, 0), (0, 1), (0, 2), (0, 3), (1, 1), (1, 3)] {
        for spin in 0..4 {
            lies.push(Rotation::quarter_turns(axis, turns).then(Rotation::quarter_turns(2, spin)));
        }
    }
    lies
}

/// A board some eighths of an inch long, wide, and thick.
fn board() -> impl Strategy<Value = Board> {
    (32..=240i64, 8..=64i64, 2..=16i64).prop_map(|(length, width, thickness)| {
        Board::new(
            fraction(length, 8),
            fraction(width, 8),
            fraction(thickness, 8),
        )
    })
}

/// An L: a board standing on edge along the edge of a flat one the same size, glued, their
/// lengths the same way.
fn l_of(board: &Board) -> Assembly {
    let edge = surfaces(board)
        .into_iter()
        .find(|surface| matches!(surface, Surface::Side(edge) if edge.out.l == 0 && edge.out.w < 0))
        .expect("a board has an edge");
    (0..4)
        .find_map(|turn| {
            let mut corner = Assembly::of(board.clone());
            let mating = Mating {
                turn,
                ..Mating::default()
            };
            corner.attach(
                (0, Surface::Top),
                &Assembly::of(board.clone()),
                (0, edge),
                mating,
                Fastener::Glue,
            );
            let [base, upright] = [0, 1].map(|k| corner.members()[k].axes()[0]);
            let along: f64 = (0..3).map(|k| base[k] * upright[k]).sum();
            (along > 0.99).then_some(corner)
        })
        .expect("a way around with their lengths alike")
}

/// One board on its own, or two glued into an L.
fn glue_up() -> impl Strategy<Value = Assembly> {
    (board(), any::<bool>()).prop_map(
        |(board, l)| {
            if l { l_of(&board) } else { Assembly::of(board) }
        },
    )
}

/// A table saw with a 10" blade: against the fence `fence` from it, or the miter gauge's stop
/// `stop` from it turned `angle` hundredths of a degree, tilted `tilt` hundredths, `height` up.
fn table_saw(guide: (Length, Option<i32>), tilt: i32, height: Length) -> TableSaw {
    let (setting, angle) = guide;
    TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence: setting,
        height,
        miter_gauge: angle.map(|_| setting),
        gauge_angle: Angle::from_hundredths(angle.unwrap_or(0)),
        tilt: Angle::from_hundredths(tilt),
    }
}

/// Where `x`, a point in the glue-up's frame, is on `member`'s board.
fn on(member: &Member, x: [Length; 3]) -> [Length; 3] {
    let off = [0, 1, 2].map(|k| (x[k] - member.at[k]).as_nm() as f64);
    member
        .rotation
        .inverse()
        .apply(off)
        .map(|nm| Length::from_nm(nm.round() as i64))
}

/// The boards a pass left, where they are in the glue-up's frame.
fn left(before: &Assembly, pass: &GlueUpPass) -> Vec<Member> {
    match pass {
        GlueUpPass::Missed => before.members().to_vec(),
        GlueUpPass::Kerfed(after) => after.members().to_vec(),
        GlueUpPass::Cut(cut) => std::iter::once(&cut.piece)
            .chain(&cut.offcut)
            .chain(&cut.loose)
            .flat_map(|part| part.members().to_vec())
            .collect(),
    }
}

/// Checks what a pass left of `before`: every board replays from its stock and sits where it
/// was on the board it came from, and, on a grid through each of the glue-up's boards,
/// wherever there was wood the blade's slot didn't take, exactly one board the pass left has
/// it, and nowhere else does any.
fn holds_its_wood(before: &Assembly, pass: &GlueUpPass) -> Result<(), TestCaseError> {
    let after = left(before, pass);
    let mut slot = None;
    for member in &after {
        prop_assert_eq!(&member.board.origin().replay(), &member.board);
        let Source::Sawn { from, kerf, at, .. } = member.board.origin().source() else {
            continue;
        };
        // The board it was sawn from, where it is now.
        let corner = member.point(Length::ZERO, Length::ZERO, Length::ZERO);
        let was = before.members().iter().find(|was| {
            was.rotation == member.rotation
                && was.point(at[0], at[1], at[2]) == corner
                && from.replay() == was.board
        });
        prop_assert!(was.is_some(), "a piece moved from where it was cut");
        slot = slot.or(was.map(|was| (was.clone(), *kerf)));
    }
    let in_slot = |x: [Length; 3]| {
        slot.as_ref().is_some_and(|(was, kerf)| {
            let [l, w, t] = on(was, x);
            kerf.contains(l, w, t)
        })
    };
    const STEPS: i64 = 5;
    // Off from any whole fraction differently on each axis, so no point lands on a face.
    let at = |size: Length, i: i64, off: i64| {
        Length::from_nm(size.as_nm() * (2 * i + 1) / (2 * STEPS) + off)
    };
    for member in before.members() {
        let board = &member.board;
        for i in 0..STEPS {
            for j in 0..STEPS {
                for k in 0..STEPS {
                    let (l, w, t) = (
                        at(board.length(), i, 7),
                        at(board.width(), j, 13),
                        at(board.thickness(), k, 3),
                    );
                    let x = member.point(l, w, t);
                    let wood = board.has_wood_in(l, w, t) && !in_slot(x);
                    let holding = after
                        .iter()
                        .filter(|piece| {
                            let [l, w, t] = on(piece, x);
                            piece.board.has_wood_in(l, w, t)
                        })
                        .count();
                    prop_assert_eq!(holding, usize::from(wood), "at {:?}", x);
                }
            }
        }
    }
    Ok(())
}

/// Checks a pass's result: anything it left holds the wood, and it only refuses a cut the saw
/// can't make, never a way of lying.
fn check(before: &Assembly, pass: Result<GlueUpPass, CutError>) -> Result<(), TestCaseError> {
    match pass {
        Ok(pass) => holds_its_wood(before, &pass),
        Err(CutError::MissesBoard { .. }) => Ok(()),
        Err(other) => {
            prop_assert!(false, "{}", other);
            Ok(())
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn the_table_saw_cuts_anything_lying_any_way_at_any_angle_and_tilt(
        glue_up in glue_up(),
        lie in 0..24usize,
        guide in prop_oneof![
            (1..=48i64).prop_map(|fence| (fraction(fence, 8), None)),
            (1..=48i64, -6000..=6000i32)
                .prop_map(|(stop, angle)| (fraction(stop, 8), Some(angle))),
        ],
        tilt in prop_oneof![Just(0), 0..=4500i32],
        height in 1..=25i64,
        stopped in proptest::option::of(0.0..1.0f64),
    ) {
        let saw = table_saw(guide, tilt, fraction(height, 8));
        let lie = square_lies()[lie];
        let pass = match stopped {
            None => saw.rip_glue_up(&glue_up, lie),
            Some(part) => {
                // Stopped somewhere along the first 40" of the way through.
                let radius = saw.blade.radius();
                let end = Length::from_nm((inches(40).as_nm() as f64 * part) as i64) - radius;
                saw.feed_glue_up(&glue_up, lie, -radius, end)
            }
        };
        check(&glue_up, pass)?;
    }

    #[test]
    fn the_miter_saw_cuts_anything_lying_any_way_at_any_angle(
        glue_up in glue_up(),
        lie in 0..24usize,
        miter in prop_oneof![Just(0), -5000..=5000i32],
        length in 1..=48i64,
        bottom in proptest::option::of(-8..=24i64),
    ) {
        let saw = MiterSaw {
            miter: Angle::from_hundredths(miter),
            ..MiterSaw::twelve_inch()
        };
        let lie = square_lies()[lie];
        let length = fraction(length, 8);
        let pass = match bottom {
            None => saw.crosscut_glue_up(&glue_up, lie, length),
            Some(bottom) => saw.chop_glue_up(&glue_up, lie, length, fraction(bottom, 8)),
        };
        check(&glue_up, pass)?;
    }
}

#[test]
fn there_are_24_square_ways_to_lie() {
    let lies = square_lies();
    for (i, a) in lies.iter().enumerate() {
        // Each axis onto another, one way or the other.
        for column in a.columns() {
            let ones = column
                .iter()
                .filter(|v| (v.abs() - 1.0).abs() < 1e-12)
                .count();
            assert_eq!(ones, 1, "{column:?}");
        }
        for b in &lies[..i] {
            assert_ne!(a.columns(), b.columns());
        }
    }
    assert_eq!(lies.len(), 24);
}
