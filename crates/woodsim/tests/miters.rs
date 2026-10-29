//! The miter saw's table turned to an angle: mitered ends, pieces mitered at both ends for a
//! frame, how much narrower a board it reaches across, and slanted scars.

mod common;

use common::CutApart;

use proptest::prelude::*;
use woodsim::{
    Angle, Blade, Board, CutError, Length, MiterSaw, Orientation, Pass, Piece, TableSaw,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

/// A 1x4, 24" long: 3/4" x 3-1/2" x 24".
fn one_by_four() -> Board {
    Board::new(inches(24), fraction(7, 2), fraction(3, 4))
}

fn mitered(degrees: i32) -> MiterSaw {
    MiterSaw {
        miter: Angle::from_degrees(degrees),
        ..MiterSaw::twelve_inch()
    }
}

/// How much of `board` has wood along the line `w` across its width, in nanometers: the line
/// through each of its face's regions, added up.
fn wood_along(board: &Board, w: Length) -> f64 {
    board
        .face()
        .iter()
        .map(|region| {
            let (mut low, mut high) = (f64::MIN, f64::MAX);
            for edge in region.edges() {
                let (l, rest) = (
                    edge.out.l as f64,
                    edge.at as f64 - edge.out.w as f64 * w.as_nm() as f64,
                );
                match l.partial_cmp(&0.0) {
                    Some(std::cmp::Ordering::Greater) => high = high.min(rest / l),
                    Some(std::cmp::Ordering::Less) => low = low.max(rest / l),
                    _ if rest < 0.0 => return 0.0,
                    _ => {}
                }
            }
            (high - low).max(0.0)
        })
        .sum()
}

fn assert_near(actual: f64, expected: Length, nm: f64) {
    assert!(
        (actual - expected.as_nm() as f64).abs() <= nm,
        "{actual} nm is not {expected:?}"
    );
}

#[test]
fn a_miter_toward_the_stop_leaves_the_long_point_at_the_fence() {
    let cut = mitered(45)
        .crosscut(&one_by_four(), Orientation::default(), inches(12))
        .cut_apart();
    let piece = &cut.piece.board;

    // Along the fence, the `w = width` edge, the piece is exactly as long as the stop is set;
    // 45° across the 3-1/2" width, its front edge is 3-1/2" shorter.
    assert_eq!(piece.length(), inches(12));
    assert_near(wood_along(piece, piece.width()), inches(12), 0.0);
    assert_near(wood_along(piece, Length::ZERO), fraction(17, 2), 0.0);
    assert_eq!(piece.miters(), [Angle::from_degrees(45)]);
    // The corner at the front of the mitered end is gone; the long point is still there.
    let nudge = Length::from_nm(1_000);
    assert!(!piece.has_wood_at(nudge, nudge));
    assert!(piece.has_wood_at(nudge + nudge, piece.width() - nudge));

    // The offcut's end is mitered the other way: long at the front, short at the fence.
    let offcut = &cut.offcut.unwrap().board;
    assert_eq!(offcut.miters(), [Angle::from_degrees(45)]);
    assert!(wood_along(offcut, Length::ZERO) > wood_along(offcut, offcut.width()));
}

#[test]
fn along_the_fence_a_slanted_kerf_takes_more_wood() {
    let cut = mitered(45)
        .crosscut(&one_by_four(), Orientation::default(), inches(12))
        .cut_apart();
    // 1/8" square to the blade is 1/8" x the square root of 2 along the fence.
    assert_eq!(cut.waste, Length::from_nm(4_490_128));
    let offcut = cut.offcut.unwrap().board;
    let fence = one_by_four().width();
    let total = wood_along(&cut.piece.board, fence) + wood_along(&offcut, fence);
    assert_near(total + cut.waste.as_nm() as f64, inches(24), 1.0);
}

#[test]
fn a_frame_piece_is_mitered_at_both_ends_long_point_to_long_point() {
    // Swung 45° away from the stop, a cut near the stop leaves the long offcut with its end's
    // long point at the fence.
    let trimmed = mitered(-45)
        .crosscut(&one_by_four(), Orientation::default(), inches(2))
        .cut_apart()
        .offcut
        .unwrap()
        .board;
    assert_eq!(trimmed.miters(), [Angle::from_degrees(45)]);

    // Slid back against the stop, that long point touches it. Swung 45° toward the stop, the
    // cut leaves 10" along the fence, long point to long point.
    let frame = mitered(45)
        .crosscut(&trimmed, Orientation::default(), inches(10))
        .cut_apart()
        .piece
        .board;
    assert_eq!(frame.length(), inches(10));
    assert_eq!(frame.miters(), [Angle::from_degrees(45); 2]);
    assert_near(wood_along(&frame, frame.width()), inches(10), 0.0);
    assert_near(wood_along(&frame, Length::ZERO), inches(3), 0.0);
}

#[test]
fn with_the_stop_at_the_boards_length_a_miter_still_takes_the_corner_off() {
    let square = MiterSaw::twelve_inch()
        .crosscut(&one_by_four(), Orientation::default(), inches(10))
        .cut_apart()
        .piece
        .board;
    // Swung toward the stop, the blade comes in at the corner on the fence and slants back
    // across the board, taking the front corner off.
    let cut = mitered(45)
        .crosscut(&square, Orientation::default(), inches(10))
        .cut_apart();
    let piece = &cut.piece.board;
    assert_eq!(piece.length(), inches(10));
    assert_eq!(piece.miters(), [Angle::from_degrees(45)]);
    assert_near(wood_along(piece, piece.width()), inches(10), 0.0);
    assert_near(wood_along(piece, Length::ZERO), fraction(13, 2), 0.0);
    // Swung away from it, the blade runs off the end without touching it.
    assert_eq!(
        mitered(-45).crosscut(&square, Orientation::default(), inches(10)),
        Err(CutError::MissesBoard {
            setting: inches(10),
            available: inches(10),
        })
    );
}

#[test]
fn a_slanted_end_is_within_a_few_nanometers_of_where_the_angle_puts_it() {
    let degrees = Angle::from_hundredths(2250);
    let saw = MiterSaw {
        miter: degrees,
        ..MiterSaw::twelve_inch()
    };
    let piece = saw
        .crosscut(&one_by_four(), Orientation::default(), inches(12))
        .cut_apart()
        .piece
        .board;
    let shorter = fraction(7, 2).as_nm() as f64 * degrees.radians().tan();
    assert_near(wood_along(&piece, Length::ZERO) + shorter, inches(12), 10.0);
    assert_eq!(piece.miters(), [degrees]);
}

#[test]
fn turned_to_an_angle_the_blade_reaches_across_less() {
    let one_by_eight = Board::new(inches(24), fraction(29, 4), fraction(3, 4));
    let saw = mitered(45);
    // 7-1/2" times the cosine of 45°, to the 64th below.
    assert_eq!(saw.widest(), fraction(339, 64));
    // The blade goes through as far as it reaches along its slant, short of the board's front
    // edge, so the board stays in one piece.
    let slotted = match saw.crosscut(&one_by_eight, Orientation::default(), inches(12)) {
        Ok(Pass::Kerfed(board)) => board,
        other => panic!("expected a cut stopped short of the front edge, got {other:?}"),
    };
    let slot = slotted.slot(&slotted.kerfs()[0]);
    assert!(!slot.bottom.unwrap().end_to_end);
    assert!(slotted.miters().is_empty());
    assert!(matches!(
        mitered(30).crosscut(&one_by_eight, Orientation::default(), inches(12)),
        Ok(Pass::Kerfed(_))
    ));
    // At 15°, it just reaches across a 1x8.
    assert_eq!(mitered(15).widest(), fraction(29, 4));
    mitered(15)
        .crosscut(&one_by_eight, Orientation::default(), inches(12))
        .cut_apart();
}

#[test]
fn stopped_short_at_an_angle_it_scars_the_board_on_the_slant() {
    let board = match mitered(45)
        .chop(
            &one_by_four(),
            Orientation::default(),
            inches(12),
            fraction(1, 2),
        )
        .unwrap()
    {
        Pass::Kerfed(board) => board,
        other => panic!("expected a scar, got {other:?}"),
    };
    let scar = board.kerfs()[0];
    assert_eq!(scar.slant(), Angle::from_degrees(45));
    assert_eq!(scar.width(), Blade::FULL_KERF);
    let size = board.slot(&scar);
    assert_eq!(size.width, Blade::FULL_KERF);
    assert!(size.bottom.is_none());
    let top = size.top.unwrap();
    assert!(!top.end_to_end);
    // A 1/4"-deep scar from a 12" blade is about 3-7/16" long, centered about 3" out from the
    // fence along its slant, and the board's front edge is 4.95" out along it at 45°.
    assert_eq!(top.length.rounded_to(fraction(1, 32)), fraction(55, 16));
    // The board is still whole and square, with the scar in it.
    assert_eq!(board.to_string(), "3/4\" x 3-1/2\" x 24\"");
    assert!(board.miters().is_empty());
}

#[test]
fn ripping_a_mitered_board_leaves_strips_mitered_the_same() {
    let frame = mitered(45)
        .crosscut(&one_by_four(), Orientation::default(), inches(12))
        .cut_apart()
        .piece
        .board;
    let saw = TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence: fraction(3, 2),
        height: inches(1),
        miter_gauge: None,
        gauge_angle: woodsim::Angle::ZERO,
        tilt: woodsim::Angle::ZERO,
    };
    let cut = saw.rip(&frame, Orientation::default()).cut_apart();
    assert_eq!(cut.piece.board.width(), fraction(3, 2));
    assert_eq!(cut.piece.board.miters(), [Angle::from_degrees(45)]);
    assert_eq!(
        cut.offcut.unwrap().board.miters(),
        [Angle::from_degrees(45)]
    );
}

/// Whether `piece`, with its corner at `corner` on the original board, has wood at `(l, w)`
/// of the original board.
fn wood_at(piece: &Board, corner: (Length, Length), (l, w): (Length, Length)) -> bool {
    piece.has_wood_at(l - corner.0, w - corner.1)
}

proptest! {
    #[test]
    fn a_miter_at_any_angle_leaves_the_set_length_along_the_fence(
        hundredths in -5000..=5000i32,
        length in 1..23 * 64i64,
    ) {
        let saw = MiterSaw {
            miter: Angle::from_hundredths(hundredths),
            ..MiterSaw::twelve_inch()
        };
        let board = one_by_four();
        let length = fraction(length, 64);
        let cut = match saw.crosscut(&board, Orientation::default(), length) {
            Ok(Pass::Cut(cut)) => cut,
            other => return Err(TestCaseError::fail(format!("{other:?}"))),
        };
        // Along the fence, the board's `w = width` edge, the piece is as long as the stop is
        // set; and the piece, the offcut, and the sawdust add back up to the board.
        let fence = |piece: &Piece| wood_along(&piece.board, board.width() - piece.w);
        prop_assert!((fence(&cut.piece) - length.as_nm() as f64).abs() <= 1.0);
        let total = fence(&cut.piece) + cut.offcut.as_ref().map_or(0.0, fence) + cut.waste.as_nm() as f64;
        prop_assert!((total - board.length().as_nm() as f64).abs() <= 2.0, "{} nm", total);
    }

    #[test]
    fn mitering_never_puts_the_same_wood_in_two_pieces(
        passes in prop::collection::vec(
            (-50..=50i32, 1..22 * 4i64, 0u8..4, any::<bool>()),
            1..5,
        ),
    ) {
        let original = Board::new(inches(24), fraction(7, 2), fraction(3, 4));
        let mut board = original.clone();
        let mut corner = (Length::ZERO, Length::ZERO);
        let mut set_aside: Vec<(Board, (Length, Length))> = Vec::new();
        for (degrees, length, turns, flipped) in passes {
            let saw = mitered(degrees);
            let mut lie = if flipped { Orientation::default().flipped() } else { Orientation::default() };
            for _ in 0..turns {
                lie = lie.turned_left();
            }
            match saw.crosscut(&board, lie, fraction(length, 4)) {
                Ok(Pass::Cut(cut)) => {
                    let at = |piece: &Piece| (corner.0 + piece.l, corner.1 + piece.w);
                    for other in cut.offcut.iter().chain(&cut.loose) {
                        set_aside.push((other.board.clone(), at(other)));
                    }
                    corner = at(&cut.piece);
                    board = cut.piece.board;
                }
                // A cut through a corner beyond a mitered end can take only sawdust.
                Ok(Pass::Kerfed(slotted)) => board = slotted,
                Ok(Pass::Missed) | Err(_) => {}
            }
        }
        set_aside.push((board, corner));

        let step = fraction(1, 8);
        let nudge = Length::from_nm(12_345);
        let mut l = nudge;
        while l < original.length() {
            let mut w = nudge;
            while w < original.width() {
                let holders = set_aside.iter().filter(|(piece, at)| wood_at(piece, *at, (l, w))).count();
                prop_assert!(holders <= 1, "{} pieces have wood at ({}, {})", holders, l, w);
                w = w + step;
            }
            l = l + step;
        }
    }
}
