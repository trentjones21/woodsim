//! One way of cutting for everything: a board or a glue-up, lying any of the 24 square ways on
//! either saw or the jointer (on edge, on end, upside down), cut with the blade at any angle and
//! tilt, or the jointer's fence at any tilt, stopped or all the way through, keeps exactly the
//! wood the blade didn't take, in exactly one piece, and every piece replays from its stock.

use proptest::prelude::*;
use woodsim::{
    Angle, Assembly, Blade, Board, CutError, Fastener, GlueUpPass, Jointer, Kerf, Length, Mating,
    Member, MiterSaw, Orientation, Rotation, Source, Surface, TableSaw, surfaces,
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

/// `board` with another like it glued onto a bevel ripped along its edge, `tilt` hundredths of
/// a degree from square: the other board stands off at an angle, turned out of square.
fn angled(board: &Board, tilt: i32) -> Assembly {
    let saw = TableSaw {
        tilt: Angle::from_hundredths(tilt),
        ..table_saw(
            (Length::from_nm(board.width().as_nm() / 2), None),
            0,
            inches(3),
        )
    };
    let Ok(GlueUpPass::Cut(cut)) = saw.rip_glue_up(&Assembly::of(board.clone()), Rotation::NONE)
    else {
        panic!("a rip down the middle cuts it apart");
    };
    let beveled = cut.piece.members()[0].board.clone();
    let (bevel, _) = beveled.bevels()[0];
    let mut glue_up = Assembly::of(beveled);
    assert!(glue_up.attach(
        (0, Surface::Side(bevel)),
        &Assembly::of(board.clone()),
        (0, Surface::Bottom),
        Mating::default(),
        Fastener::Glue,
    ));
    glue_up
}

/// One board on its own, two glued into an L, or one glued onto another's bevel at an angle.
fn glue_up() -> impl Strategy<Value = Assembly> {
    (board(), 0..3u8, 100..=4500i32).prop_map(|(board, kind, tilt)| match kind {
        0 => Assembly::of(board),
        1 => l_of(&board),
        _ => angled(&board, tilt),
    })
}

/// How it lies on the saw: one of the 24 square ways, or turned any way at all, about an axis
/// pointing `turn` around and `rise` up, by `angle`, all in hundredths of a degree.
fn lie() -> impl Strategy<Value = Rotation> {
    prop_oneof![
        (0..24usize).prop_map(|k| square_lies()[k]),
        (0..36_000i32, -9_000..=9_000i32, 0..36_000i32).prop_map(|(turn, rise, angle)| {
            let (turn, rise) = (
                (turn as f64 / 100.0).to_radians(),
                (rise as f64 / 100.0).to_radians(),
            );
            let axis = [turn.cos() * rise.cos(), turn.sin() * rise.cos(), rise.sin()];
            Rotation::about(axis, Angle::from_hundredths(angle))
        }),
    ]
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

/// Which of `before`'s boards a pass left nothing of: none of what it left came from them, and
/// they're not left as they were.
fn gone(before: &Assembly, pass: &GlueUpPass) -> Vec<usize> {
    let after = left(before, pass);
    (0..before.members().len())
        .filter(|&index| {
            let was = &before.members()[index];
            !after.iter().any(|member| {
                member == was
                    || matches!(member.board.origin().source(), Source::Sawn { from, .. }
                        if from.replay() == was.board && member.rotation == was.rotation)
            })
        })
        .collect()
}

/// Checks what a pass left of `before`: every board replays from its stock and sits where it
/// was on the board it came from, and, on a grid through each of the glue-up's boards,
/// wherever there was wood the blade's slot didn't take, exactly one board the pass left has
/// it, and nowhere else does any. A board the pass left nothing of went all to sawdust.
fn holds_its_wood(before: &Assembly, pass: &GlueUpPass) -> Result<(), TestCaseError> {
    let gone = gone(before, pass);
    let after = left(before, pass);
    // The slot the blade cut in each of the glue-up's boards it touched, in that board's own
    // whole numbers.
    let mut slots: Vec<Option<Kerf>> = vec![None; before.members().len()];
    for member in &after {
        prop_assert_eq!(&member.board.origin().replay(), &member.board);
        // Left as it was, the blade didn't touch it, though it may have been sawn before.
        if before.members().contains(member) {
            continue;
        }
        let Source::Sawn { from, kerf, at, .. } = member.board.origin().source() else {
            continue;
        };
        // The board it was sawn from, where it is now.
        let corner = member.point(Length::ZERO, Length::ZERO, Length::ZERO);
        let was = before.members().iter().position(|was| {
            was.rotation == member.rotation
                && was.point(at[0], at[1], at[2]) == corner
                && from.replay() == was.board
        });
        prop_assert!(was.is_some(), "a piece moved from where it was cut");
        slots[was.unwrap()] = Some(*kerf);
    }
    const STEPS: i64 = 5;
    // Off from any whole fraction differently on each axis, so no point lands on a face.
    let at = |size: Length, i: i64, off: i64| {
        Length::from_nm(size.as_nm() * (2 * i + 1) / (2 * STEPS) + off)
    };
    let nm = Length::from_nm;
    // How many of the glue-up's boards have wood at `x` the blade didn't take: one at most,
    // since they don't overlap, though one's box can hold another's wood.
    let wood_at = |x: [Length; 3]| {
        before
            .members()
            .iter()
            .zip(&slots)
            .enumerate()
            .filter(|(index, _)| !gone.contains(index))
            .map(|(_, member)| member)
            .filter(|(member, slot)| {
                let [l, w, t] = on(member, x);
                member.board.has_wood_in(l, w, t)
                    && !slot.is_some_and(|kerf| kerf.contains(l, w, t))
            })
            .count()
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
                    // Where wood starts or stops, turning a point into a board turned off
                    // square can land it a nanometer either side, so only points clear of
                    // every face are checked.
                    let wood = wood_at(x);
                    let near = (0..3).any(|axis| {
                        [-25, 25].into_iter().any(|off| {
                            let mut y = x;
                            y[axis] = y[axis] + nm(off);
                            wood_at(y) != wood
                        })
                    });
                    if near {
                        continue;
                    }
                    let holding = after
                        .iter()
                        .filter(|piece| {
                            let [l, w, t] = on(piece, x);
                            piece.board.has_wood_in(l, w, t)
                        })
                        .count();
                    prop_assert_eq!(holding, wood, "at {:?}", x);
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
        lie in lie(),
        guide in prop_oneof![
            (0..=48i64).prop_map(|fence| (fraction(fence, 8), None)),
            (0..=48i64, -6000..=6000i32)
                .prop_map(|(stop, angle)| (fraction(stop, 8), Some(angle))),
        ],
        tilt in prop_oneof![Just(0), 0..=4500i32],
        height in 1..=25i64,
        stopped in proptest::option::of(0.0..1.0f64),
    ) {
        let saw = table_saw(guide, tilt, fraction(height, 8));
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
        lie in lie(),
        miter in prop_oneof![Just(0), -5000..=5000i32],
        length in 0..=48i64,
        bottom in proptest::option::of(-8..=24i64),
    ) {
        let saw = MiterSaw {
            miter: Angle::from_hundredths(miter),
            ..MiterSaw::twelve_inch()
        };
        let length = fraction(length, 8);
        let pass = match bottom {
            None => saw.crosscut_glue_up(&glue_up, lie, length),
            Some(bottom) => saw.chop_glue_up(&glue_up, lie, length, fraction(bottom, 8)),
        };
        check(&glue_up, pass)?;
    }

    #[test]
    fn the_jointer_joints_anything_lying_any_way_with_its_fence_at_any_tilt(
        glue_up in glue_up(),
        lie in lie(),
        depth in 1..=32i64,
        fence in 0..=48i64,
        tilt in prop_oneof![Just(0), 0..=4500i32],
        stopped in proptest::option::of(0.0..1.0f64),
    ) {
        let jointer = Jointer {
            depth: fraction(depth, 64),
            fence: fraction(fence, 8),
            tilt: Angle::from_hundredths(tilt),
            ..Jointer::six_inch()
        };
        let pass = match stopped {
            None => jointer.joint_glue_up(&glue_up, lie),
            Some(part) => {
                // Stopped somewhere along the first 40" of the way across.
                let radius = jointer.radius();
                let end = Length::from_nm((inches(40).as_nm() as f64 * part) as i64) - radius;
                jointer.feed_glue_up(&glue_up, lie, -radius, end)
            }
        };
        // A thin enough board, lying low enough, can go entirely to chips: all of it, or some
        // of a glue-up's boards, which lay within the depth of cut of the bottom of it all.
        if let Ok(pass) = &pass {
            let lie = jointer.leaned(lie);
            let heights = |member: &Member| {
                member
                    .board
                    .corners()
                    .into_iter()
                    .map(|[l, w, t]| lie.apply(member.point(l, w, t).map(|x| x.as_nm() as f64))[2])
                    .collect::<Vec<f64>>()
            };
            let bottom = glue_up.members().iter().flat_map(heights).fold(f64::MAX, f64::min);
            for index in gone(&glue_up, pass) {
                let top = heights(&glue_up.members()[index]).into_iter().fold(f64::MIN, f64::max);
                prop_assert!(top <= bottom + jointer.depth.as_nm() as f64 + 1.0);
            }
        }
        if pass != Err(CutError::AllSawdust) {
            check(&glue_up, pass)?;
        }
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

#[test]
fn a_board_on_its_own_can_lie_every_square_way() {
    // Tipped, stood, flipped, and turned, a board lies each way a glue-up can.
    let flat = Orientation::default();
    let mut found = Vec::new();
    for mut lie in [
        flat,
        flat.tipped(),
        flat.flipped(),
        flat.flipped().tipped(),
        flat.stood(),
        flat.stood().flipped(),
    ] {
        for _ in 0..4 {
            found.push(lie.rotation().columns());
            lie = lie.turned_left();
        }
    }
    for lie in square_lies() {
        assert!(found.contains(&lie.columns()), "{:?}", lie.columns());
    }
}
