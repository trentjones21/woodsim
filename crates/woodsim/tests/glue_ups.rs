//! Cutting glue-ups on the saws: each board gets the slot the blade cuts through it, and the
//! pieces stay fastened wherever they still meet flush. A glue-up of one board cuts exactly like
//! the board.

mod common;

use common::CutApart;

use proptest::prelude::*;
use woodsim::{
    Angle, Assembly, Blade, Board, Fastener, GlueUpCut, GlueUpPass, Length, Mating, MiterSaw,
    Orientation, Pass, Piece, Rotation, Surface, TableSaw, surfaces,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

fn table_saw(fence: Length, gauge: Option<Length>, height: Length) -> TableSaw {
    TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence,
        height,
        miter_gauge: gauge,
        gauge_angle: woodsim::Angle::ZERO,
    }
}

fn mitered(hundredths: i32) -> MiterSaw {
    MiterSaw {
        miter: Angle::from_hundredths(hundredths),
        ..MiterSaw::twelve_inch()
    }
}

/// Every way a board lies flat: turned, and face up or down.
fn flat_ways() -> Vec<Orientation> {
    let mut ways = Vec::new();
    for flipped in [false, true] {
        let mut lie = if flipped {
            Orientation::default().flipped()
        } else {
            Orientation::default()
        };
        for _ in 0..4 {
            ways.push(lie);
            lie = lie.turned_left();
        }
    }
    ways
}

/// A glue-up's only board, and where its corner is, if it has just one.
fn single(part: &Assembly) -> Option<(Board, [Length; 3])> {
    match part.members() {
        [member] if member.rotation == Rotation::NONE => Some((member.board.clone(), member.at)),
        _ => None,
    }
}

fn as_piece(piece: &Piece) -> (Board, [Length; 3]) {
    (piece.board.clone(), [piece.l, piece.w, piece.t])
}

/// Whether cutting a glue-up of one board came out the same as cutting the board.
fn same(board: Result<Pass, woodsim::CutError>, glue_up: Result<GlueUpPass, woodsim::CutError>) {
    match (board, glue_up) {
        (Ok(Pass::Missed), Ok(GlueUpPass::Missed)) => {}
        (Ok(Pass::Kerfed(board)), Ok(GlueUpPass::Kerfed(part))) => {
            assert_eq!(single(&part), Some((board, [Length::ZERO; 3])));
        }
        (Ok(Pass::Cut(cut)), Ok(GlueUpPass::Cut(glued))) => {
            assert_eq!(single(&glued.piece), Some(as_piece(&cut.piece)));
            assert_eq!(
                glued.offcut.as_ref().map(|part| single(part).unwrap()),
                cut.offcut.as_ref().map(as_piece)
            );
            let mut loose: Vec<_> = glued
                .loose
                .iter()
                .map(|part| single(part).unwrap())
                .collect();
            let mut expected: Vec<_> = cut.loose.iter().map(as_piece).collect();
            let key = |(board, at): &(Board, [Length; 3])| (format!("{board:?}"), *at);
            loose.sort_by_key(key);
            expected.sort_by_key(key);
            assert_eq!(loose, expected);
        }
        (Err(a), Err(b)) => assert_eq!(a, b),
        (a, b) => panic!("the board gave {a:?} but the glue-up {b:?}"),
    }
}

#[test]
fn a_glue_up_of_one_board_rips_and_crosscuts_like_the_board_on_the_table_saw() {
    let board = Board::new(inches(24), fraction(7, 2), fraction(3, 4));
    for lie in flat_ways() {
        let glue_up = Assembly::of(board.clone());
        for fence in [fraction(1, 2), inches(2), inches(3), inches(30)] {
            for height in [inches(1), fraction(1, 4)] {
                let saw = table_saw(fence, None, height);
                same(
                    saw.rip(&board, lie),
                    saw.rip_glue_up(&glue_up, lie.rotation()),
                );
                // Part way along, a stopped cut.
                same(
                    saw.feed(&board, lie, -inches(5), inches(7)),
                    saw.feed_glue_up(&glue_up, lie.rotation(), -inches(5), inches(7)),
                );
            }
            let gauge = table_saw(inches(6), Some(fence), inches(1));
            same(
                gauge.rip(&board, lie),
                gauge.rip_glue_up(&glue_up, lie.rotation()),
            );
        }
    }
}

#[test]
fn a_glue_up_of_one_board_chops_like_the_board_on_the_miter_saw() {
    let board = Board::new(inches(24), fraction(5, 2), fraction(3, 4));
    for lie in flat_ways() {
        let glue_up = Assembly::of(board.clone());
        for miter in [-4500, -3000, 0, 2250, 4500] {
            let saw = mitered(miter);
            for length in [inches(6), inches(12), inches(30)] {
                // All the way down, and stopped short a quarter inch above the table.
                for bottom in [saw.stroke.1, fraction(1, 4)] {
                    same(
                        saw.chop(&board, lie, length, bottom),
                        saw.chop_glue_up(&glue_up, lie.rotation(), length, bottom),
                    );
                }
            }
        }
    }
}

#[test]
fn a_glue_up_of_one_slotted_board_sets_the_same_strips_loose() {
    // A stopped rip 2" in from the fence edge, through the first 12" or so of a 1x6.
    let stock = Board::new(inches(24), fraction(11, 2), fraction(3, 4));
    let slotted = match table_saw(inches(2), None, inches(1)).feed(
        &stock,
        Orientation::default(),
        -inches(5),
        inches(8),
    ) {
        Ok(Pass::Kerfed(board)) => board,
        other => panic!("{other:?}"),
    };
    // Crosscut through the slotted end, the strip beside the slot falls loose.
    for lie in flat_ways() {
        let board_cut = mitered(0).crosscut(&slotted, lie, inches(18));
        if lie == Orientation::default() {
            assert!(matches!(&board_cut, Ok(Pass::Cut(cut)) if !cut.loose.is_empty()));
        }
        same(
            board_cut,
            mitered(0).crosscut_glue_up(&Assembly::of(slotted.clone()), lie.rotation(), inches(18)),
        );
    }
}

/// The edge of `board` along its length, at `w = width` or `w = 0`.
fn edge(board: &Board, far: bool) -> Surface {
    surfaces(board)
        .into_iter()
        .find(|surface| {
            matches!(surface, Surface::Side(edge) if edge.out.l == 0 && (edge.out.w > 0) == far)
        })
        .unwrap()
}

/// Puts `next`'s `surface` against `onto`, face up and flush at the least sides.
fn glue(glue_up: &mut Assembly, onto: (usize, Surface), next: &Board, surface: Surface) {
    let other = Assembly::of(next.clone());
    let mating = (0..4)
        .map(|turn| Mating {
            turn,
            slide: [Length::ZERO; 2],
        })
        .find(|&mating| {
            let motion = glue_up.mating(onto, &other, (0, surface), mating).unwrap();
            motion.rotation.apply([0.0, 0.0, 1.0])[2] > 0.999
        })
        .unwrap();
    assert!(glue_up.attach(onto, &other, (0, surface), mating, Fastener::Glue));
}

/// Boards `widths` wide, 24" long and 3/4" thick, glued edge to edge into a panel, each next
/// one on the far edge of the last.
fn panel(widths: &[Length]) -> Assembly {
    let board = |width: Length| Board::new(inches(24), width, fraction(3, 4));
    let mut panel = Assembly::of(board(widths[0]));
    for (k, &width) in widths.iter().enumerate().skip(1) {
        let last = &panel.members()[k - 1].board.clone();
        glue(
            &mut panel,
            (k - 1, edge(last, true)),
            &board(width),
            edge(&board(width), false),
        );
    }
    panel
}

fn cut_apart(pass: Result<GlueUpPass, woodsim::CutError>) -> GlueUpCut {
    match pass {
        Ok(GlueUpPass::Cut(cut)) => cut,
        other => panic!("expected the glue-up cut apart, got {other:?}"),
    }
}

#[test]
fn a_panel_crosscut_on_the_miter_saw_comes_apart_into_two_panels() {
    let panel = panel(&[fraction(5, 2), fraction(5, 2)]);
    let cut = cut_apart(mitered(0).crosscut_glue_up(&panel, Rotation::NONE, inches(12)));

    // Each side is two boards still glued edge to edge.
    for part in [&cut.piece, cut.offcut.as_ref().unwrap()] {
        assert_eq!(part.members().len(), 2);
        assert_eq!(part.joints().len(), 1);
        assert!(part.fit().iter().all(|contact| contact.is_tight()));
    }
    assert!(cut.loose.is_empty());
    // The piece against the stop is 12" long; the offcut is what's left past the kerf.
    let length = |part: &Assembly| {
        let (least, most) = part.bounds()[0];
        most - least
    };
    assert_eq!(length(&cut.piece), inches(12));
    assert_eq!(
        length(cut.offcut.as_ref().unwrap()),
        inches(12) - Blade::FULL_KERF
    );
}

#[test]
fn ripping_a_panel_through_one_board_leaves_the_rest_glued_and_a_strip_loose() {
    // Two 4" boards: an 8" panel. With the fence 5" from the blade, the piece against the fence
    // is the far board and 1" of the near one, still glued to it.
    let panel = panel(&[inches(4), inches(4)]);
    let saw = table_saw(inches(5), None, inches(1));
    let cut = cut_apart(saw.rip_glue_up(&panel, Rotation::NONE));

    assert_eq!(cut.piece.members().len(), 2);
    assert_eq!(cut.piece.joints().len(), 1);
    let mut widths: Vec<Length> = cut
        .piece
        .members()
        .iter()
        .map(|member| member.board.width())
        .collect();
    widths.sort();
    assert_eq!(widths, [inches(1), inches(4)]);
    // The offcut is all that's left of the near board, on its own.
    let offcut = cut.offcut.unwrap();
    assert_eq!(offcut.members().len(), 1);
    assert_eq!(
        offcut.members()[0].board.width(),
        inches(3) - Blade::FULL_KERF
    );
}

#[test]
fn a_frame_ripped_down_the_middle_comes_apart_into_two_halves() {
    // Four 1x2 sides butted into a square 10" across: two long ones, two short ones between.
    let long = Board::new(inches(10), fraction(3, 2), fraction(3, 4));
    let short = Board::new(inches(7), fraction(3, 2), fraction(3, 4));
    let end = |board: &Board, far: bool| {
        surfaces(board)
            .into_iter()
            .find(|surface| {
                matches!(surface, Surface::Side(edge) if edge.out.w == 0 && (edge.out.l > 0) == far)
            })
            .unwrap()
    };
    let mut frame = Assembly::of(long.clone());
    // The short sides stand on the long one's far edge at each end, square to it.
    let on_edge = (0, edge(&long, true));
    let other = Assembly::of(short.clone());
    for flush_far in [false, true] {
        let turn = (0..4)
            .find(|&turn| {
                let motion = frame
                    .mating(
                        on_edge,
                        &other,
                        (0, end(&short, false)),
                        Mating {
                            turn,
                            slide: [Length::ZERO; 2],
                        },
                    )
                    .unwrap();
                motion.rotation.apply([0.0, 0.0, 1.0])[2] > 0.999
            })
            .unwrap();
        let stops = frame
            .stops(on_edge, &other, (0, end(&short, false)), turn)
            .unwrap();
        let slide = [stops[0][if flush_far { 2 } else { 0 }], stops[1][0]];
        assert!(frame.attach(
            on_edge,
            &other,
            (0, end(&short, false)),
            Mating { turn, slide },
            Fastener::Glue
        ));
    }
    // The other long side across their far ends: it's glued to both.
    let across = (1, end(&short, true));
    let other = Assembly::of(long.clone());
    let turn = (0..4)
        .find(|&turn| {
            let motion = frame
                .mating(
                    across,
                    &other,
                    (0, edge(&long, false)),
                    Mating {
                        turn,
                        slide: [Length::ZERO; 2],
                    },
                )
                .unwrap();
            motion.rotation.apply([0.0, 0.0, 1.0])[2] > 0.999
        })
        .unwrap();
    let stops = frame
        .stops(across, &other, (0, edge(&long, false)), turn)
        .unwrap();
    // Flush with the frame's outside along the length.
    let fits = |slide| {
        let mut closed = frame.clone();
        closed.attach(
            across,
            &other,
            (0, edge(&long, false)),
            Mating { turn, slide },
            Fastener::Glue,
        );
        closed.joints().len() == frame.joints().len() + 2
    };
    let slide = [0, 2]
        .into_iter()
        .map(|k| [stops[0][k], stops[1][0]])
        .find(|&slide| fits(slide))
        .expect("a slide that meets both short sides");
    frame.attach(
        across,
        &other,
        (0, edge(&long, false)),
        Mating { turn, slide },
        Fastener::Glue,
    );
    assert_eq!(frame.joints().len(), 4);

    // Ripped with the long sides running along the table, the blade cuts through both short
    // sides and the frame comes apart into two U shapes.
    let lie = Rotation::quarter_turns(2, 1);
    let saw = table_saw(inches(5), None, inches(1));
    let cut = cut_apart(saw.rip_glue_up(&frame, lie));
    for half in [&cut.piece, cut.offcut.as_ref().unwrap()] {
        assert_eq!(half.members().len(), 3, "{half:?}");
        assert_eq!(half.joints().len(), 2);
    }
    assert!(cut.loose.is_empty());
}

#[test]
fn a_blade_lower_than_the_panel_grooves_every_board_and_it_holds_together() {
    // Crosswise, with the gauge: a dado across both boards.
    let panel = panel(&[fraction(5, 2), fraction(5, 2)]);
    let lie = Orientation::default().turned_left().rotation();
    let saw = table_saw(inches(6), Some(inches(6)), fraction(1, 4));
    let Ok(GlueUpPass::Kerfed(dadoed)) = saw.rip_glue_up(&panel, lie) else {
        panic!("expected a dado");
    };
    assert_eq!(dadoed.members().len(), 2);
    assert_eq!(dadoed.joints().len(), 1);
    for member in dadoed.members() {
        assert_eq!(member.board.kerfs().len(), 1);
    }
}

#[test]
fn a_glue_up_with_a_board_standing_up_is_not_cut() {
    let base = Board::new(inches(12), inches(4), fraction(3, 4));
    let upright = Board::new(inches(6), inches(4), fraction(3, 4));
    let end = surfaces(&upright)
        .into_iter()
        .find(|surface| matches!(surface, Surface::Side(edge) if edge.out.l > 0))
        .unwrap();
    let mut glue_up = Assembly::of(base);
    let other = Assembly::of(upright);
    assert!(glue_up.attach(
        (0, Surface::Top),
        &other,
        (0, end),
        Mating::default(),
        Fastener::Screws
    ));
    let saw = table_saw(inches(2), None, inches(1));
    assert!(matches!(
        saw.rip_glue_up(&glue_up, Rotation::NONE),
        Err(woodsim::CutError::NotModeled(_))
    ));
}

#[test]
fn a_blade_that_misses_the_glue_up_leaves_it_alone() {
    let panel = panel(&[inches(2), inches(2)]);
    let saw = table_saw(inches(2), None, inches(1));
    // Stopped short of the panel's leading end.
    assert_eq!(
        saw.feed_glue_up(&panel, Rotation::NONE, -inches(12), -inches(6)),
        Ok(GlueUpPass::Missed)
    );
}

/// Ten inches, in nanometers.
const TEN_INCHES: i64 = 10 * 25_400_000;

proptest! {
    #[test]
    fn ripping_a_panel_conserves_its_width(
        first in 25_400_000i64..TEN_INCHES / 2,
        second in 25_400_000i64..TEN_INCHES / 2,
        fence in 1i64..TEN_INCHES,
    ) {
        let widths = [Length::from_nm(first), Length::from_nm(second)];
        let panel = panel(&widths);
        let saw = table_saw(Length::from_nm(fence), None, inches(1));
        let span = widths[0] + widths[1];
        match saw.rip_glue_up(&panel, Rotation::NONE) {
            Ok(GlueUpPass::Cut(cut)) => {
                let parts: Vec<&Assembly> =
                    [&cut.piece].into_iter().chain(&cut.offcut).chain(&cut.loose).collect();
                // Every board still runs the full length; across, the pieces and the kerf add
                // back up to the panel.
                let mut across = Length::ZERO;
                for part in &parts {
                    for member in part.members() {
                        prop_assert_eq!(member.board.length(), inches(24));
                        across = across + member.board.width();
                    }
                }
                let slot = (span - saw.fence - saw.blade.kerf, span - saw.fence);
                let waste = slot.1.min(span) - slot.0.max(Length::ZERO);
                prop_assert_eq!(across + waste, span);
                // The piece against the fence is exactly as wide as the fence setting.
                let (least, most) = cut.piece.bounds()[1];
                prop_assert_eq!(most - least, saw.fence);
            }
            Ok(GlueUpPass::Kerfed(_)) => {
                // Only when the slot cuts nothing but one board's edge off, to sawdust.
                prop_assert!(span - saw.fence <= saw.blade.kerf);
            }
            other => {
                prop_assert!(saw.fence >= span, "{:?}", other);
            }
        }
    }

    #[test]
    fn crosscutting_a_panel_conserves_each_boards_length(
        length in 25_400_000i64..23 * 25_400_000,
    ) {
        let panel = panel(&[fraction(5, 2), fraction(5, 2)]);
        let saw = mitered(0);
        let cut = match saw.crosscut_glue_up(&panel, Rotation::NONE, Length::from_nm(length)) {
            Ok(GlueUpPass::Cut(cut)) => cut,
            other => return Err(TestCaseError::fail(format!("{other:?}"))),
        };
        let parts: Vec<&Assembly> =
            [&cut.piece].into_iter().chain(&cut.offcut).chain(&cut.loose).collect();
        // Every board's pieces and one kerf add back up to its 24".
        let total: Length = parts
            .iter()
            .flat_map(|part| part.members())
            .map(|member| member.board.length())
            .sum();
        prop_assert_eq!(total + saw.blade.kerf + saw.blade.kerf, inches(48));
        prop_assert_eq!(cut.piece.bounds()[0].1 - cut.piece.bounds()[0].0, Length::from_nm(length));
    }
}

#[test]
fn an_octagon_ripped_across_the_middle_comes_apart_into_two_halves() {
    // Eight 22.5° frame pieces, 6" long point to point, glued around: half of them lie at an
    // angle to the saw, so the blade crosses them on the slant.
    let stock = Board::new(inches(48), fraction(3, 2), fraction(3, 4));
    let trimmed = mitered(-2250)
        .crosscut(&stock, Orientation::default(), inches(2))
        .cut_apart()
        .offcut
        .unwrap()
        .board;
    let piece = mitered(2250)
        .crosscut(&trimmed, Orientation::default(), inches(6))
        .cut_apart()
        .piece
        .board;
    let miter = |board: &Board, far: bool| {
        surfaces(board)
            .into_iter()
            .find(|surface| {
                matches!(surface, Surface::Side(edge) if edge.out.w != 0 && (edge.out.l > 0) == far)
            })
            .unwrap()
    };
    let mut octagon = Assembly::of(piece.clone());
    for last in 0..7 {
        let onto = (last, miter(&octagon.members()[last].board, true));
        glue(&mut octagon, onto, &piece, miter(&piece, false));
    }
    assert_eq!(octagon.joints().len(), 8);

    let (least, most) = octagon.bounds()[1];
    let fence = Length::from_nm((most - least).as_nm() / 2);
    let saw = table_saw(fence, None, inches(1));
    let cut = cut_apart(saw.rip_glue_up(&octagon, Rotation::NONE));
    assert!(cut.loose.is_empty());
    let halves = [&cut.piece, cut.offcut.as_ref().unwrap()];
    // The blade crossed the ring twice, splitting two pieces: ten boards in all, every joint
    // left between them tight.
    assert_eq!(
        halves
            .iter()
            .map(|half| half.members().len())
            .sum::<usize>(),
        10
    );
    for half in halves {
        assert!(half.fit().iter().all(|contact| contact.is_tight()));
        assert_eq!(half.joints().len(), half.members().len() - 1);
    }
    // The half against the fence is as wide as the setting, to within a few nanometers.
    let (least, most) = cut.piece.bounds()[1];
    assert!(
        ((most - least) - fence).as_nm().abs() < 10,
        "{}",
        most - least
    );
}
