//! The table saw's miter gauge turned to an angle: the blade crosses the board slanted, and the
//! piece against the stop is the set length along the gauge, to its long point. No wood is made
//! or lost, and every piece replays from its stock.

mod common;

use common::CutApart;
use proptest::prelude::*;
use woodsim::{Angle, Blade, Board, CutError, Length, Orientation, Pass, Piece, TableSaw};

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

/// A table saw with its miter gauge in use, the stop at `stop`, turned `hundredths` of a degree.
fn gauge(stop: Length, hundredths: i32) -> TableSaw {
    TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence: inches(6),
        height: inches(1),
        miter_gauge: Some(stop),
        gauge_angle: Angle::from_hundredths(hundredths),
        tilt: Angle::ZERO,
    }
}

/// Turned a quarter turn, across the table, to be crosscut against the gauge.
fn across() -> Orientation {
    Orientation::default().turned_left()
}

/// A 14" piece of a 1x4, cut against the gauge turned 45°: one end square, the other mitered,
/// its long point on the edge that was against the gauge.
fn mitered_piece() -> Board {
    gauge(inches(14), 4500)
        .rip(&one_by_four(), across())
        .cut_apart()
        .piece
        .board
}

/// The four ways a board lies flat across the table: as it was, turned over, end for end, and
/// both.
fn every_way_across() -> [Orientation; 4] {
    let end_for_end = across().turned_left().turned_left();
    [
        across(),
        across().flipped(),
        end_for_end,
        end_for_end.flipped(),
    ]
}

/// How much of `board` has wood along the line `w` across its width, in nanometers.
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

/// How much wood `piece` has along the line `w` across the board it was cut from.
fn along(piece: &Piece, w: Length) -> f64 {
    wood_along(&piece.board, w - piece.w)
}

#[test]
fn square_the_gauge_cuts_as_it_always_has() {
    let square = gauge(inches(10), 0)
        .rip(&one_by_four(), across())
        .cut_apart();
    assert_eq!(square.piece.board.length(), inches(10));
    assert!(square.piece.board.miters().is_empty());
}

#[test]
fn turned_45_degrees_the_piece_is_the_set_length_to_its_long_point() {
    let cut = gauge(inches(10), 4500)
        .rip(&one_by_four(), across())
        .cut_apart();
    let piece = &cut.piece.board;
    assert_eq!(piece.miters(), [Angle::from_degrees(45)]);
    assert_eq!(piece.length(), inches(10));
    // Along one edge it's the full 10"; along the other, 3-1/2" shorter.
    let edges = [Length::ZERO, piece.width()].map(|w| wood_along(piece, w));
    let (long, short) = (edges[0].max(edges[1]), edges[0].min(edges[1]));
    assert!((long - inches(10).as_nm() as f64).abs() <= 1.0, "{edges:?}");
    assert!(
        (short - fraction(13, 2).as_nm() as f64).abs() <= 1.0,
        "{edges:?}"
    );
    assert_eq!(piece.origin().replay(), *piece);
}

#[test]
fn the_other_way_the_miter_slants_the_other_way() {
    let toward = gauge(inches(10), 3000)
        .rip(&one_by_four(), across())
        .cut_apart();
    let away = gauge(inches(10), -3000)
        .rip(&one_by_four(), across())
        .cut_apart();
    assert_eq!(toward.piece.board.miters(), [Angle::from_degrees(30)]);
    assert_eq!(away.piece.board.miters(), [Angle::from_degrees(30)]);
    assert_ne!(toward.piece.board, away.piece.board);
}

#[test]
fn a_glue_up_waits_for_a_square_gauge() {
    let glue_up = woodsim::Assembly::of(one_by_four());
    assert!(matches!(
        gauge(inches(10), 4500).rip_glue_up(&glue_up, woodsim::Rotation::NONE),
        Err(CutError::NotModeled(_))
    ));
}

#[test]
fn with_the_stop_at_a_mitered_pieces_length_the_slanted_blade_still_crosses_it() {
    // The second miter of a frame's side is cut with the stop where it was, the first miter's
    // long point against it: the blade comes in at the far end's corner and slants across.
    let piece = mitered_piece();
    let near = |nm: f64, length: Length| (nm - length.as_nm() as f64).abs() <= 4.0;
    let mut shapes = Vec::new();
    for lie in every_way_across() {
        let shape = match gauge(inches(14), 4500).rip(&piece, lie) {
            // Lying as it was cut, the blade runs right along the miter that's there.
            Err(CutError::MissesBoard { available, .. }) => {
                assert_eq!(available, inches(14));
                "missed"
            }
            Ok(Pass::Cut(cut)) => {
                for part in std::iter::once(&cut.piece).chain(&cut.offcut) {
                    assert_eq!(part.board.origin().replay(), part.board);
                }
                let board = &cut.piece.board;
                let length = board.length().as_nm() as f64;
                let mut edges = [Length::ZERO, board.width()].map(|w| wood_along(board, w));
                edges.sort_by(f64::total_cmp);
                if near(length, inches(14))
                    && near(edges[0], inches(7))
                    && near(edges[1], inches(14))
                {
                    // Turned over and end for end: long point to long point on one edge.
                    assert_eq!(board.miters(), [Angle::from_degrees(45); 2]);
                    "frame side"
                } else if near(length, inches(14))
                    && edges.iter().all(|&e| near(e, fraction(21, 2)))
                {
                    // End for end: the two miters slant the same way.
                    assert_eq!(board.miters(), [Angle::from_degrees(45); 2]);
                    "parallelogram"
                } else if near(length, fraction(49, 4))
                    && edges.iter().all(|&e| near(e, fraction(21, 2)))
                {
                    // Turned over: the blade takes the long point off, to a point mid-width.
                    "pointed"
                } else {
                    panic!("{lie:?} left {board:?} with edges {edges:?}");
                }
            }
            other => panic!("{lie:?} gave {other:?}"),
        };
        shapes.push(shape);
    }
    shapes.sort();
    assert_eq!(shapes, ["frame side", "missed", "parallelogram", "pointed"]);
}

proptest! {
    #[test]
    fn at_any_angle_the_gauge_leaves_the_set_length_and_loses_no_wood(
        hundredths in -6000..=6000i32,
        stop in 1..23 * 64i64,
    ) {
        let board = one_by_four();
        let stop = fraction(stop, 64);
        let cut = match gauge(stop, hundredths).rip(&board, across()) {
            Ok(Pass::Cut(cut)) => cut,
            other => return Err(TestCaseError::fail(format!("{other:?}"))),
        };
        // Along the gauge, one of the board's edges, the piece is as long as the stop is set;
        // and along it the piece, the offcut, and the sawdust add back up to the board. (Along
        // the other edge, a slot that runs off the board's end takes less.)
        let exact = |w: Length| (along(&cut.piece, w) - stop.as_nm() as f64).abs() <= 1.0;
        let Some(w) = [Length::ZERO, board.width()].into_iter().find(|&w| exact(w)) else {
            return Err(TestCaseError::fail("the piece isn't the set length along either edge"));
        };
        let total = along(&cut.piece, w)
            + cut.offcut.as_ref().map_or(0.0, |offcut| along(offcut, w))
            + cut.waste.as_nm() as f64;
        prop_assert!((total - board.length().as_nm() as f64).abs() <= 2.0, "{} nm", total);
        for piece in std::iter::once(&cut.piece).chain(&cut.offcut).chain(&cut.loose) {
            prop_assert_eq!(&piece.board.origin().replay(), &piece.board);
        }
    }

    #[test]
    fn cutting_a_mitered_piece_again_any_way_round_loses_no_more_than_the_kerf(
        first in -6000..=6000i32,
        hundredths in -6000..=6000i32,
        lie in 0..4usize,
        stop in 1..20 * 64i64,
    ) {
        let piece = gauge(inches(14), first)
            .rip(&one_by_four(), across())
            .cut_apart()
            .piece
            .board;
        let lie = every_way_across()[lie];
        let stop = fraction(stop, 64);
        match gauge(stop, hundredths).rip(&piece, lie) {
            Ok(Pass::Cut(cut)) => {
                // Along any line down the piece's length, what's left adds up to what was
                // there, less no more than the slot is wide along it.
                let slot = Blade::FULL_KERF.as_nm() as f64
                    / Angle::from_hundredths(hundredths).radians().cos();
                let parts: Vec<&Piece> =
                    std::iter::once(&cut.piece).chain(&cut.offcut).chain(&cut.loose).collect();
                for w in [fraction(1, 64), fraction(7, 4), piece.width() - fraction(1, 64)] {
                    let before = wood_along(&piece, w);
                    let after: f64 = parts.iter().map(|part| along(part, w)).sum();
                    prop_assert!(after <= before + 2.0, "{} grew to {} at {}", before, after, w);
                    prop_assert!(before - after <= slot + 2.0, "{} fell to {} at {}", before, after, w);
                }
                for part in parts {
                    prop_assert_eq!(&part.board.origin().replay(), &part.board);
                }
            }
            // Grazing a corner, the blade turns a sliver narrower than itself to sawdust, and
            // the board keeps its outline with the slot in it.
            Ok(Pass::Kerfed(kerfed)) => {
                prop_assert_eq!(kerfed.face(), piece.face());
                prop_assert_eq!(kerfed.kerfs().len(), piece.kerfs().len() + 1);
            }
            // The blade can pass through the gap where a miter left the stop's end short.
            Ok(Pass::Missed) => {}
            Err(CutError::MissesBoard { available, .. }) => {
                // Where the error says the board reaches, the blade would just reach it.
                prop_assert!(available <= stop);
                let nearer = available - fraction(1, 64);
                if nearer > Length::ZERO {
                    let pass = gauge(nearer, hundredths).rip(&piece, lie);
                    prop_assert!(
                        matches!(pass, Ok(Pass::Cut(_) | Pass::Kerfed(_))),
                        "{:?}",
                        pass
                    );
                }
            }
            other => prop_assert!(false, "{:?}", other),
        }
    }
}
