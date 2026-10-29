//! Wood made again with a pass made another way: a strip ripped wider, and every piece cut from
//! it, and the frame they were glued into back together; a side cut longer, and a frame that no
//! longer closes; a change that stops a cut from cutting refused; and joints taken apart. A pass
//! through a glue-up is made again the same way, on the glue-up made again: a panel crosscut
//! longer, or ripped wider before it was glued up; read back from a recipe, or duplicated. And
//! what a pass left, made again as that very pass, for plans to draw.

mod common;

use std::sync::Arc;

use common::CutApart;
use woodsim::{
    Angle, Assembly, Blade, Board, Fastener, GlueUpCut, GlueUpPass, Length, Mating, MiterSaw,
    Operation, Orientation, Recipe, Refusal, Remake, Rotation, Surface, TableSaw, Tool, Wood,
    surfaces,
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

/// The `w = 0` edge of `board`, or its other one.
fn edge(board: &Board, far: bool) -> Surface {
    surfaces(board)
        .into_iter()
        .find(|surface| matches!(surface, Surface::Side(edge) if edge.out.l == 0 && (edge.out.w > 0) == far))
        .expect("an edge")
}

/// A 48" 1x6 ripped to a 2" strip, and the strip glued edge to edge with what's left of the
/// board, face up, flush at their ends.
fn panel() -> Assembly {
    let stock = Board::new(inches(48), fraction(11, 2), fraction(3, 4));
    let cut = table_saw(inches(2))
        .rip(&stock, Orientation::default())
        .cut_apart();
    let (strip, rest) = (cut.piece.board, cut.offcut.expect("the rest").board);
    let mut panel = Assembly::of(strip.clone());
    add(
        &mut panel,
        (0, edge(&strip, true)),
        &rest,
        edge(&rest, false),
    );
    panel
}

/// `glue_up` crosscut square at the miter saw, `length` from the stop.
fn crosscut(glue_up: &Assembly, length: Length) -> GlueUpCut {
    match mitered(0).crosscut_glue_up(glue_up, Rotation::NONE, length) {
        Ok(GlueUpPass::Cut(cut)) => cut,
        other => panic!("expected the glue-up cut apart, got {other:?}"),
    }
}

/// The last pass that cut `board`.
fn last(board: &Board) -> Arc<Operation> {
    board.origin().operations().pop().expect("a cut board")
}

/// `tool` set to cut `length` from the stop.
fn at_length(tool: Tool, length: Length) -> Tool {
    let Tool::MiterSaw {
        saw, lie, bottom, ..
    } = tool
    else {
        panic!("cut on the miter saw");
    };
    Tool::MiterSaw {
        saw,
        lie,
        length,
        bottom,
    }
}

/// Checks every joint in `assembly` is flush all across.
fn closed(assembly: &Assembly) {
    for joint in assembly.joints() {
        let contact = assembly
            .fit()
            .into_iter()
            .find(|contact| contact.between == joint.between)
            .expect("the surfaces still meet");
        assert!(contact.is_tight(), "{contact:?}");
    }
}

#[test]
fn a_crosscut_through_a_glue_up_made_longer_comes_out_longer_still_glued() {
    let cut = crosscut(&panel(), inches(10));
    let pass = last(&cut.piece.members()[0].board);
    let mut remake = Remake::new(&pass, at_length(pass.tool, inches(12)));
    let piece = remake
        .assembly(&cut.piece)
        .unwrap()
        .expect("the crosscut cut both boards");
    for (member, was) in piece.members().iter().zip(cut.piece.members()) {
        assert_eq!(member.board.length(), inches(12));
        assert_eq!(member.board.width(), was.board.width());
        assert_eq!(member.board.origin().replay(), member.board);
    }
    // Still one pass through both boards, numbered as it was, and they're still glued.
    let [a, b] = [0, 1].map(|k| last(&piece.members()[k].board));
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(a.number, pass.number);
    assert_eq!(numbers(&piece), numbers(&cut.piece));
    closed(&piece);
    // The offcut, from the same pass made again, is shorter by as much.
    let was = cut.offcut.expect("an offcut");
    let offcut = remake.assembly(&was).unwrap().unwrap();
    for (member, was) in offcut.members().iter().zip(was.members()) {
        assert_eq!(member.board.length(), was.board.length() - inches(2));
    }
    assert!(Arc::ptr_eq(&last(&offcut.members()[0].board), &a));
}

#[test]
fn a_rip_before_a_glue_up_was_crosscut_can_be_made_wider() {
    let cut = crosscut(&panel(), inches(10));
    let rip = pass(&cut.piece.members()[0].board, 0);
    let Tool::TableSaw { saw, lie, travel } = rip.tool else {
        panic!("ripped on the table saw");
    };
    let wider = Tool::TableSaw {
        saw: TableSaw {
            fence: fraction(5, 2),
            ..saw
        },
        lie,
        travel,
    };
    let piece = Remake::new(&rip, wider)
        .assembly(&cut.piece)
        .unwrap()
        .expect("both boards came from the rip");
    let widths: Vec<Length> = piece
        .members()
        .iter()
        .map(|member| member.board.width())
        .collect();
    let rest = fraction(11, 2) - fraction(5, 2) - Blade::FULL_KERF;
    assert_eq!(widths, [fraction(5, 2), rest]);
    for member in piece.members() {
        assert_eq!(member.board.length(), inches(10));
        assert_eq!(member.board.origin().replay(), member.board);
    }
    assert_eq!(numbers(&piece), numbers(&cut.piece));
    closed(&piece);
}

#[test]
fn a_pass_through_a_glue_up_made_again_as_it_was_changes_nothing() {
    let cut = crosscut(&panel(), inches(10));
    let pass = last(&cut.piece.members()[0].board);
    let made = Remake::new(&pass, pass.tool)
        .assembly(&cut.piece)
        .unwrap()
        .unwrap();
    assert_eq!(made.joints(), cut.piece.joints());
    assert_eq!(numbers(&made), numbers(&cut.piece));
    for (now, was) in made.members().iter().zip(cut.piece.members()) {
        assert_eq!(now.board, was.board);
        assert_eq!(now.rotation, was.rotation);
        for k in 0..3 {
            assert!(
                (now.at[k] - was.at[k]).as_nm().abs() <= 2,
                "{now:?} {was:?}"
            );
        }
        let (now, was) = (now.board.origin(), was.board.origin());
        assert_eq!(now.side(), was.side());
        assert_eq!(now.at_on_stock(), was.at_on_stock());
    }
}

#[test]
fn what_a_pass_through_a_glue_up_left_comes_out_again_cut_by_that_pass() {
    let cut = crosscut(&panel(), inches(10));
    let pass = last(&cut.piece.members()[0].board);
    let left = pass.left().expect("it kept what it went through").unwrap();
    assert_eq!(left, GlueUpPass::Cut(cut.clone()));
    let was = [&cut.piece, cut.offcut.as_ref().expect("the rest")];
    for (now, was) in left.parts().into_iter().zip(was) {
        for (now, was) in now.members().iter().zip(was.members()) {
            assert_eq!(now.at, was.at);
            // The same piece of the same board, cut by the very same pass.
            assert!(Arc::ptr_eq(&last(&now.board), &pass));
            let (now, was) = (now.board.origin(), was.board.origin());
            assert_eq!(format!("{now:?}"), format!("{was:?}"));
            assert_eq!(now.at_on_stock(), was.at_on_stock());
        }
    }
    // A pass that didn't keep what it went through can't.
    let forgot = Arc::new(Operation {
        wood: None,
        ..(*pass).clone()
    });
    assert!(forgot.left().is_none());
}

#[test]
fn a_rip_along_one_board_of_a_glue_up_made_wider_keeps_the_other_glued_on() {
    // The fence is at the rest's free edge: the rip cuts only it.
    let Ok(GlueUpPass::Cut(cut)) = table_saw(inches(1)).rip_glue_up(&panel(), Rotation::NONE)
    else {
        panic!("expected the panel ripped apart");
    };
    assert_eq!(cut.piece.members().len(), 1);
    let was = cut.offcut.expect("the rest of the panel");
    assert_eq!(was.members().len(), 2);
    let pass = last(&cut.piece.members()[0].board);
    let Tool::TableSaw { saw, lie, travel } = pass.tool else {
        panic!("ripped on the table saw");
    };
    let wider = Tool::TableSaw {
        saw: TableSaw {
            fence: fraction(5, 4),
            ..saw
        },
        lie,
        travel,
    };
    let mut remake = Remake::new(&pass, wider);
    let strip = remake.assembly(&cut.piece).unwrap().unwrap();
    assert_eq!(strip.members()[0].board.width(), fraction(5, 4));
    let rest = remake.assembly(&was).unwrap().unwrap();
    // The board the blade missed is just as it was, still glued to what's left of the other.
    let (missed, ripped) = (&rest.members()[0].board, &rest.members()[1].board);
    assert!(missed.origin().same(was.members()[0].board.origin()));
    assert_eq!(missed.width(), inches(2));
    assert_eq!(
        ripped.width(),
        was.members()[1].board.width() - fraction(1, 4)
    );
    assert_eq!(numbers(&rest), numbers(&was));
    closed(&rest);
}

#[test]
fn a_cut_through_a_glue_up_that_would_miss_or_cut_another_board_is_refused() {
    // A 12" board glued along a 24" one's edge, flush with its end against the stop.
    let long = Board::new(inches(24), inches(2), fraction(3, 4));
    let short = Board::new(inches(12), inches(2), fraction(3, 4));
    let mut glue_up = Assembly::of(long.clone());
    add(
        &mut glue_up,
        (0, edge(&long, true)),
        &short,
        edge(&short, false),
    );
    let cut_by = |board: &Board, pass: &Arc<Operation>| {
        board
            .origin()
            .operations()
            .iter()
            .any(|op| Arc::ptr_eq(op, pass))
    };

    let both = crosscut(&glue_up, inches(6));
    let pass = last(&both.piece.members()[0].board);
    assert!(
        both.piece
            .members()
            .iter()
            .all(|member| cut_by(&member.board, &pass))
    );
    let refused = Remake::new(&pass, at_length(pass.tool, inches(18)))
        .assembly(&both.piece)
        .unwrap_err();
    let why = "it misses a board of the glue-up it cut".to_string();
    assert_eq!(refused, Refusal::Pass(pass.number, why));

    // 18" from the stop, the blade misses the short board, on the stop's side of it.
    let one = crosscut(&glue_up, inches(18));
    let pass = last(&one.piece.members()[0].board);
    let missed = &one.piece.members()[1].board;
    assert!(!cut_by(missed, &pass) && *missed == short);
    let refused = Remake::new(&pass, at_length(pass.tool, inches(6)))
        .board(&one.piece.members()[0].board)
        .unwrap_err();
    let why = "it cuts a board of the glue-up it missed".to_string();
    assert_eq!(refused, Refusal::Pass(pass.number, why));
}

#[test]
fn a_glue_up_read_back_can_have_its_crosscut_made_another_way() {
    let cut = crosscut(&panel(), inches(10));
    let text = Recipe {
        wood: Wood::GlueUp(cut.piece.clone()),
        saw: None,
    }
    .write();
    let lines = |kind: &str| text.lines().filter(|line| line.starts_with(kind)).count();
    assert_eq!((lines("on "), lines("on-joint ")), (2, 1), "{text}");
    let Wood::GlueUp(read) = Recipe::read(&text).unwrap().wood else {
        panic!("a glue-up reads back as a glue-up");
    };
    let pass = last(&read.members()[0].board);
    assert_eq!(pass.wood.as_ref().map(|wood| wood.members().len()), Some(2));
    let longer = at_length(pass.tool, inches(12));
    let piece = Remake::new(&pass, longer).assembly(&read).unwrap().unwrap();
    for member in piece.members() {
        assert_eq!(member.board.length(), inches(12));
    }
    closed(&piece);

    // Written before passes kept what they went through, it doesn't know what was on the saw.
    let old: String = text
        .lines()
        .filter(|line| !line.starts_with("on"))
        .map(|line| format!("{line}\n"))
        .collect::<String>()
        .replacen(&format!("version {}", Recipe::VERSION), "version 1", 1);
    let Wood::GlueUp(read) = Recipe::read(&old).unwrap().wood else {
        panic!("a glue-up reads back as a glue-up");
    };
    assert_eq!(read, cut.piece);
    let pass = last(&read.members()[0].board);
    assert!(pass.wood.is_none());
    let refused = Remake::new(&pass, longer).assembly(&read).unwrap_err();
    assert_eq!(refused, Refusal::GlueUpPass(pass.number));
}

#[test]
fn a_glue_up_cut_from_another_duplicates_with_what_was_on_the_saw() {
    let cut = crosscut(&panel(), inches(10));
    let copy = cut.piece.duplicate();
    let (pass, was) = (
        last(&copy.members()[0].board),
        last(&cut.piece.members()[0].board),
    );
    assert!(Arc::ptr_eq(&pass, &last(&copy.members()[1].board)));
    assert!(pass.number > was.number);
    // The panel it was cut from is copied too: boards of its own, its joint numbered anew.
    let (wood, original) = (pass.wood.as_ref().unwrap(), was.wood.as_ref().unwrap());
    for (now, then) in wood.members().iter().zip(original.members()) {
        assert_eq!(now.board, then.board);
        let stocks = (now.board.origin().stock(), then.board.origin().stock());
        assert!(!stocks.0.same(stocks.1));
    }
    assert!(wood.joints()[0].number > was.number);
    // And the copy's crosscut can be made another way.
    let piece = Remake::new(&pass, at_length(pass.tool, inches(12)))
        .assembly(&copy)
        .unwrap()
        .unwrap();
    for member in piece.members() {
        assert_eq!(member.board.length(), inches(12));
    }
}
