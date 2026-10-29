//! Wood made again with a pass made another way: a strip ripped wider, and every piece cut from
//! it, and the frame they were glued into back together; a side cut longer, and a frame that no
//! longer closes; a change that stops a cut from cutting refused; and joints taken apart.

mod common;

use std::sync::Arc;

use common::CutApart;
use woodsim::{
    Angle, Assembly, Blade, Board, Fastener, Length, Mating, MiterSaw, Operation, Orientation,
    Refusal, Remake, Surface, TableSaw, Tool, surfaces,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

fn table_saw(fence: Length) -> TableSaw {
    TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence,
        height: inches(1),
        miter_gauge: None,
        gauge_angle: Angle::ZERO,
        tilt: Angle::ZERO,
    }
}

fn mitered(hundredths: i32) -> MiterSaw {
    MiterSaw {
        miter: Angle::from_hundredths(hundredths),
        ..MiterSaw::twelve_inch()
    }
}

/// The mitered end of `board` at its `l = 0` end, or its other end.
fn miter(board: &Board, far_end: bool) -> Surface {
    surfaces(board)
        .into_iter()
        .find(|surface| match surface {
            Surface::Side(edge) => edge.out.w != 0 && (edge.out.l > 0) == far_end,
            _ => false,
        })
        .expect("a mitered end")
}

/// Puts `piece`'s `surface` against `onto`, face up and flush.
fn add(assembly: &mut Assembly, onto: (usize, Surface), piece: &Board, surface: Surface) {
    let other = Assembly::of(piece.clone());
    let mating = (0..4)
        .map(|turn| Mating {
            turn,
            slide: [Length::ZERO; 2],
        })
        .find(|&mating| {
            let motion = assembly.mating(onto, &other, (0, surface), mating).unwrap();
            motion.rotation.apply([0.0, 0.0, 1.0])[2] > 0.999
        })
        .expect("a way that keeps it face up");
    assert!(assembly.attach(onto, &other, (0, surface), mating, Fastener::Glue));
}

/// A frame of four pieces 5" long point to long point, mitered 45° at both ends, made the way
/// you'd make it: a 48" 1x4 ripped to a 1-1/2" strip, and from the strip, for each piece, the
/// end trimmed to a miter and the piece cut off it; then the four glued into a frame.
fn frame() -> Assembly {
    let stock = Board::new(inches(48), fraction(7, 2), fraction(3, 4));
    let mut strip = table_saw(fraction(3, 2))
        .rip(&stock, Orientation::default())
        .cut_apart()
        .piece
        .board;
    let mut pieces = Vec::new();
    for _ in 0..4 {
        let trimmed = mitered(-4500)
            .crosscut(&strip, Orientation::default(), inches(2))
            .cut_apart()
            .offcut
            .unwrap()
            .board;
        let cut = mitered(4500)
            .crosscut(&trimmed, Orientation::default(), inches(5))
            .cut_apart();
        pieces.push(cut.piece.board);
        strip = cut.offcut.unwrap().board;
    }
    let mut frame = Assembly::of(pieces[0].clone());
    for piece in &pieces[1..] {
        let last = frame.members().len() - 1;
        let onto = (last, miter(&frame.members()[last].board, true));
        add(&mut frame, onto, piece, miter(piece, false));
    }
    assert_eq!(
        frame.joints().len(),
        4,
        "the last piece is glued at both its corners"
    );
    frame
}

/// The `n`th pass `board` went through, from its stock on.
fn pass(board: &Board, n: usize) -> Arc<Operation> {
    Arc::clone(&board.origin().operations()[n])
}

/// Which pieces each joint holds together, in order.
fn between(assembly: &Assembly) -> Vec<[usize; 2]> {
    assembly
        .joints()
        .iter()
        .map(|joint| joint.between.map(|(member, _)| member))
        .collect()
}

/// The numbers of the joints, in order.
fn numbers(assembly: &Assembly) -> Vec<u64> {
    assembly.joints().iter().map(|joint| joint.number).collect()
}

#[test]
fn a_frame_ripped_wider_goes_back_together_every_corner_closed() {
    let frame = frame();
    let rip = pass(&frame.members()[0].board, 0);
    let Tool::TableSaw { saw, lie, travel } = rip.tool else {
        panic!("ripped on the table saw");
    };
    let wider = Tool::TableSaw {
        saw: TableSaw {
            fence: inches(2),
            ..saw
        },
        lie,
        travel,
    };
    let remade = Remake::new(&rip, wider)
        .assembly(&frame)
        .unwrap()
        .expect("every piece came from the rip");
    for (member, was) in remade.members().iter().zip(frame.members()) {
        let board = &member.board;
        assert_eq!(board.width(), inches(2));
        assert_eq!(board.length(), was.board.length());
        assert_eq!(board.origin().replay(), *board);
        // The same passes, numbered as they were: still one rip for all four.
        let (now, then) = (board.origin().operations(), was.board.origin().operations());
        assert_eq!(
            now.iter().map(|pass| pass.number).collect::<Vec<_>>(),
            then.iter().map(|pass| pass.number).collect::<Vec<_>>()
        );
        assert!(Arc::ptr_eq(&now[0], &pass(&remade.members()[0].board, 0)));
        assert!(board.origin().stock().same(was.board.origin().stock()));
    }
    // Every corner closes: joints between the same pieces, numbered as they were, all flush,
    // where the wider pieces' miters are now.
    assert_eq!(numbers(&remade), numbers(&frame));
    assert_eq!(between(&remade), between(&frame));
    for joint in remade.joints() {
        let contact = remade
            .fit()
            .into_iter()
            .find(|contact| contact.between == joint.between)
            .expect("the surfaces still meet");
        assert!(contact.is_tight(), "{contact:?}");
    }
    // The first piece stays where it was.
    assert_eq!(remade.members()[0].at, frame.members()[0].at);
}

#[test]
fn a_side_cut_longer_leaves_the_last_corner_open() {
    let frame = frame();
    // Its own cut is its last: the passes before it cut the pieces before it.
    let side = &frame.members()[1].board;
    let cut = pass(side, side.origin().operations().len() - 1);
    let Tool::MiterSaw {
        saw, lie, bottom, ..
    } = cut.tool
    else {
        panic!("cut on the miter saw");
    };
    let longer = Tool::MiterSaw {
        saw,
        lie,
        length: inches(6),
        bottom,
    };
    let mut remake = Remake::new(&cut, longer);
    let remade = remake.assembly(&frame).unwrap().unwrap();
    assert_eq!(remade.members()[1].board.length(), inches(6));
    // Its offcut was cut into the pieces after it, but they're the same length as before:
    // only where they came from on the strip moved.
    for member in [2, 3] {
        assert_eq!(
            remade.members()[member].board.length(),
            frame.members()[member].board.length()
        );
    }
    assert_eq!(remade.joints().len(), 3, "{:?}", remade.joints());
    let unchanged = remake.board(&frame.members()[0].board).unwrap();
    assert_eq!(unchanged, None, "the first piece was cut before it");
}

#[test]
fn a_rip_that_no_longer_cuts_through_is_refused() {
    let frame = frame();
    let rip = pass(&frame.members()[0].board, 0);
    let Tool::TableSaw { saw, lie, travel } = rip.tool else {
        panic!("ripped on the table saw");
    };
    let lower = Tool::TableSaw {
        saw: TableSaw {
            height: fraction(1, 2),
            ..saw
        },
        lie,
        travel,
    };
    let refused = Remake::new(&rip, lower)
        .board(&frame.members()[0].board)
        .unwrap_err();
    assert_eq!(
        refused,
        Refusal::Pass(rip.number, "it doesn't cut it apart".to_string())
    );
}

#[test]
fn a_glue_up_put_back_together_as_it_was_is_the_same() {
    let frame = frame();
    let boards = frame
        .members()
        .iter()
        .map(|member| member.board.clone())
        .collect();
    let rebuilt = frame.rebuilt(boards).unwrap();
    assert_eq!(rebuilt.joints(), frame.joints());
    assert_eq!(numbers(&rebuilt), numbers(&frame));
    for (now, was) in rebuilt.members().iter().zip(frame.members()) {
        assert_eq!(now.rotation, was.rotation);
        for k in 0..3 {
            assert!(
                (now.at[k] - was.at[k]).as_nm().abs() <= 2,
                "{now:?} {was:?}"
            );
        }
    }
}

#[test]
fn a_corner_taken_apart_leaves_a_frame_in_one_piece_and_two_in_two() {
    let frame = frame();
    // The first piece's two corners: to the second piece, and to the last.
    let corners: Vec<usize> = (0..frame.joints().len())
        .filter(|&j| frame.joints()[j].between.iter().any(|&(m, _)| m == 0))
        .collect();
    assert_eq!(corners.len(), 2);
    let one = frame.taken_apart(&corners[..1]);
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].joints().len(), 3);
    assert_eq!(one[0].members(), frame.members());
    let two = frame.taken_apart(&corners);
    assert_eq!(two.len(), 2);
    assert_eq!(two[0].members(), &frame.members()[..1]);
    assert!(two[0].joints().is_empty());
    assert_eq!(two[1].members(), &frame.members()[1..]);
    assert_eq!(two[1].joints().len(), 2);
    // Each joint left is between the same boards as before.
    for joint in two[1].joints() {
        let [(a, _), (b, _)] = joint.between;
        assert!(frame.joints().iter().any(|was| {
            was.between[0].0 == a + 1 && was.between[1].0 == b + 1 && was.number == joint.number
        }));
    }
}
