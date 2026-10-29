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
    }
}

/// Turned a quarter turn, across the table, to be crosscut against the gauge.
fn across() -> Orientation {
    Orientation::default().turned_left()
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
}
