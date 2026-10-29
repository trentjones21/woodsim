//! The table saw's blade tilted: it leans away from the fence going up, about where it meets
//! the table, so the fence and the gauge's stop still measure to there. Pieces come out
//! beveled; no wood is made or lost; and every piece replays from its stock.

mod common;

use common::CutApart;
use proptest::prelude::*;
use woodsim::{
    Angle, Assembly, Blade, Board, Cut, CutError, Kerf, Length, Orientation, Pass, Piece, Rotation,
    Source, TableSaw,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

const CUBIC_INCH: f64 = 25_400_000.0 * 25_400_000.0 * 25_400_000.0;

/// A 1x6, 24" long: 3/4" x 5-1/2" x 24".
fn one_by_six() -> Board {
    Board::new(inches(24), fraction(11, 2), fraction(3, 4))
}

/// A 10" table saw with its blade 1" up, tilted `tilt` hundredths of a degree, the rip fence
/// `fence` from it.
fn tilted(fence: Length, tilt: i32) -> TableSaw {
    TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence,
        height: inches(1),
        miter_gauge: None,
        gauge_angle: Angle::ZERO,
        tilt: Angle::from_hundredths(tilt),
    }
}

/// The same, with the miter gauge's stop at `stop`, the gauge turned `angle` hundredths.
fn gauge(stop: Length, angle: i32, tilt: i32) -> TableSaw {
    TableSaw {
        miter_gauge: Some(stop),
        gauge_angle: Angle::from_hundredths(angle),
        ..tilted(inches(6), tilt)
    }
}

/// Turned a quarter turn, across the table, to be crosscut against the gauge.
fn across() -> Orientation {
    Orientation::default().turned_left()
}

fn cubic_inches(volume: f64) -> f64 {
    volume / CUBIC_INCH
}

/// The slot a cut left, from where its piece came from.
fn slot(cut: &Cut) -> Kerf {
    match cut.piece.board.origin().source() {
        Source::Sawn { kerf, .. } => *kerf,
        Source::Stock { .. } => panic!("a cut piece came from a cut"),
    }
}

fn parts(cut: &Cut) -> Vec<&Piece> {
    std::iter::once(&cut.piece)
        .chain(&cut.offcut)
        .chain(&cut.loose)
        .collect()
}

/// Checks on a grid through `board` that wherever it had wood the cut's slot didn't take,
/// exactly one of its parts has it, and nowhere else does any.
fn parts_hold_its_wood(board: &Board, cut: &Cut, steps: i64) -> Result<(), TestCaseError> {
    let kerf = slot(cut);
    // Off from any whole fraction differently on each axis, so no point lands on a face.
    let at = |size: Length, i: i64, off: i64| {
        Length::from_nm(size.as_nm() * (2 * i + 1) / (2 * steps) + off)
    };
    for i in 0..steps {
        for j in 0..steps {
            for k in 0..steps {
                let (l, w, t) = (
                    at(board.length(), i, 7),
                    at(board.width(), j, 13),
                    at(board.thickness(), k, 3),
                );
                let holding = parts(cut)
                    .into_iter()
                    .filter(|part| part.board.has_wood_in(l - part.l, w - part.w, t - part.t))
                    .count();
                let wood = board.has_wood_in(l, w, t) && !kerf.contains(l, w, t);
                prop_assert_eq!(holding, usize::from(wood), "at {}, {}, {}", l, w, t);
            }
        }
    }
    Ok(())
}

/// Whether the piece has wood at `(l, w, t)` of the board it was cut from.
fn has_wood(piece: &Piece, l: Length, w: Length, t: Length) -> bool {
    piece
        .board
        .has_wood_in(l - piece.l, w - piece.w, t - piece.t)
}

#[test]
fn square_the_blade_cuts_as_it_always_has() {
    let cut = tilted(inches(2), 0)
        .rip(&one_by_six(), Orientation::default())
        .cut_apart();
    assert_eq!(cut.piece.board.width(), inches(2));
    assert!(cut.piece.board.face().iter().all(|region| !region.leans()));
}

#[test]
fn tilted_the_piece_against_the_fence_is_the_setting_wide_where_it_lay_on_the_table() {
    let board = one_by_six();
    let cut = tilted(inches(2), 4500)
        .rip(&board, Orientation::default())
        .cut_apart();
    let piece = &cut.piece;
    // The board's `w = width` edge rides the fence. The blade leans away from it going up, so
    // the piece is 2" wide on the bottom face and 3/4" wider on top.
    assert_eq!(piece.board.width(), fraction(11, 4));
    assert_eq!(piece.board.thickness(), fraction(3, 4));
    let low = fraction(1, 64);
    let high = fraction(3, 4) - low;
    let near_fence = fraction(11, 2) - inches(2);
    assert!(has_wood(piece, inches(12), near_fence + low, low));
    assert!(!has_wood(piece, inches(12), near_fence - low, low));
    assert!(has_wood(
        piece,
        inches(12),
        near_fence - fraction(1, 2),
        high
    ));
    assert!((cubic_inches(piece.board.volume()) - 24.0 * (2.0 + 2.75) / 2.0 * 0.75).abs() < 1e-6);
    // The slot is 1/8" square to its faces, wider across the table.
    let across = 0.125 / 45f64.to_radians().cos();
    assert!((cut.waste.as_nm() as f64 - across * 25_400_000.0).abs() <= 1.0);
    let slot = 24.0 * across * 0.75;
    let offcut = cut.offcut.as_ref().unwrap();
    let total = cubic_inches(piece.board.volume() + offcut.board.volume()) + slot;
    assert!((total - 24.0 * 5.5 * 0.75).abs() < 1e-6, "{total}");
    for part in parts(&cut) {
        assert_eq!(part.board.origin().replay(), part.board);
    }
    parts_hold_its_wood(&board, &cut, 9).unwrap();
}

#[test]
fn turned_over_the_bevel_runs_the_other_way_through_the_board() {
    let board = one_by_six();
    let up = tilted(inches(2), 3000)
        .rip(&board, Orientation::default())
        .cut_apart();
    let over = tilted(inches(2), 3000)
        .rip(&board, Orientation::default().flipped())
        .cut_apart();
    assert_ne!(up.piece.board, over.piece.board);
    assert!((up.piece.board.volume() / over.piece.board.volume() - 1.0).abs() < 1e-12);
    for cut in [&up, &over] {
        for part in parts(cut) {
            assert_eq!(part.board.origin().replay(), part.board);
        }
        parts_hold_its_wood(&board, cut, 7).unwrap();
    }
}

#[test]
fn with_the_gauge_the_piece_against_the_stop_is_the_setting_long_on_the_table() {
    let board = Board::new(inches(24), fraction(7, 2), fraction(3, 4));
    let cut = gauge(inches(10), 0, 4500).rip(&board, across()).cut_apart();
    let piece = &cut.piece.board;
    // Leaning away from the fence, toward the stop, the blade takes 3/4" more off the top.
    assert_eq!(piece.length(), inches(10));
    assert!(piece.miters().is_empty());
    assert!((cubic_inches(piece.volume()) - (10.0 + 9.25) / 2.0 * 3.5 * 0.75).abs() < 1e-6);
    for part in parts(&cut) {
        assert_eq!(part.board.origin().replay(), part.board);
    }
    parts_hold_its_wood(&board, &cut, 9).unwrap();
}

#[test]
fn a_compound_cut_is_mitered_and_beveled_and_the_set_length_on_the_table() {
    let board = Board::new(inches(24), fraction(7, 2), fraction(3, 4));
    let cut = gauge(inches(10), 3000, 4500)
        .rip(&board, across())
        .cut_apart();
    let piece = &cut.piece.board;
    // The long point is on the bottom face, along the gauge, 10" from the stop.
    assert_eq!(piece.length(), inches(10));
    assert_eq!(piece.miters(), [Angle::from_degrees(30)]);
    assert!(piece.face().iter().all(|region| region.leans()));
    for part in parts(&cut) {
        assert_eq!(part.board.origin().replay(), part.board);
    }
    parts_hold_its_wood(&board, &cut, 9).unwrap();
}

#[test]
fn with_the_stop_past_the_board_a_tilted_blade_still_takes_the_top_corner() {
    let board = Board::new(inches(10), fraction(7, 2), fraction(3, 4));
    // At 45°, the stop-side face comes 3/4" back toward the stop by the top of the board.
    let cut = gauge(inches(10) + fraction(1, 4), 0, 4500)
        .rip(&board, across())
        .cut_apart();
    assert_eq!(cut.piece.board.length(), inches(10));
    assert!(cut.piece.board.volume() < board.volume());
    parts_hold_its_wood(&board, &cut, 9).unwrap();
    // Further out, it misses; it would just reach at 10-3/4".
    match gauge(inches(11), 0, 4500).rip(&board, across()) {
        Err(CutError::MissesBoard { setting, available }) => {
            assert_eq!(setting, inches(11));
            assert!(
                (available - (inches(10) + fraction(3, 4))).as_nm().abs() <= 1,
                "{available}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn what_a_tilted_blade_cannot_cut_yet() {
    let board = one_by_six();
    let not_modeled = |pass: Result<Pass, CutError>| matches!(pass, Err(CutError::NotModeled(_)));
    // A bevel that doesn't go through, the rounded bottom of a tilted groove.
    let low = TableSaw {
        height: fraction(1, 2),
        ..tilted(inches(2), 4500)
    };
    assert!(not_modeled(low.rip(&board, Orientation::default())));
    // A board on edge.
    let on_edge = Orientation::default().tipped();
    assert!(not_modeled(
        tilted(fraction(3, 8), 4500).rip(&board, on_edge)
    ));
    // A glue-up.
    let glue_up = Assembly::of(board.clone());
    assert!(matches!(
        tilted(inches(2), 4500).rip_glue_up(&glue_up, Rotation::NONE),
        Err(CutError::NotModeled(_))
    ));
}

proptest! {
    #[test]
    fn a_tilted_cut_loses_just_the_wood_in_its_slot_and_replays(
        tilt in 1..=4500i32,
        guide in prop_oneof![
            (1..44i64).prop_map(|fence| (fraction(fence, 8), None)),
            (1..24 * 8i64, -6000..=6000i32)
                .prop_map(|(stop, angle)| (fraction(stop, 8), Some(angle))),
        ],
        flipped in any::<bool>(),
        end_for_end in any::<bool>(),
    ) {
        let board = one_by_six();
        let (setting, angle) = guide;
        let (saw, lie) = match angle {
            None => (tilted(setting, tilt), Orientation::default()),
            Some(angle) => (gauge(setting, angle, tilt), across()),
        };
        let lie = if flipped { lie.flipped() } else { lie };
        let lie = if end_for_end { lie.turned_left().turned_left() } else { lie };
        match saw.rip(&board, lie) {
            Ok(Pass::Cut(cut)) => {
                let volume: f64 = parts(&cut).iter().map(|part| part.board.volume()).sum();
                prop_assert!(volume < board.volume());
                for part in parts(&cut) {
                    prop_assert_eq!(&part.board.origin().replay(), &part.board);
                }
                parts_hold_its_wood(&board, &cut, 6)?;
            }
            Ok(Pass::Kerfed(kerfed)) => {
                prop_assert_eq!(&kerfed.origin().replay(), &kerfed);
            }
            Ok(Pass::Missed) | Err(CutError::MissesBoard { .. }) => {}
            other => prop_assert!(false, "{:?}", other),
        }
    }
}
