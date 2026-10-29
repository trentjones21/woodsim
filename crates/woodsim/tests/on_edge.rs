//! Boards standing on an edge: resawing on the table saw, in one pass or two from opposite
//! edges, and chopping on the miter saw.

mod common;

use common::CutApart;

use proptest::prelude::*;
use woodsim::{
    Across, Angle, Axis, Blade, Board, CutError, Length, MiterSaw, Orientation, Pass, Piece,
    TableSaw,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

/// A table saw with a 10" full-kerf blade, its fence at `fence` and blade `height` up.
fn table_saw(fence: Length, height: Length) -> TableSaw {
    TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence,
        height,
        miter_gauge: None,
        gauge_angle: woodsim::Angle::ZERO,
    }
}

/// As high as the table saw's blade goes: 3-1/8".
fn all_the_way_up() -> Length {
    fraction(25, 8)
}

/// Standing on its `w = 0` edge, its `t = 0` face against the fence.
fn on_edge() -> Orientation {
    Orientation::default().tipped()
}

/// On its other edge, with the same face against the fence: turned end for end and flipped.
fn on_other_edge() -> Orientation {
    on_edge().flipped().turned_left().turned_left()
}

fn kerfed(pass: Result<Pass, CutError>) -> Board {
    match pass {
        Ok(Pass::Kerfed(board)) => board,
        other => panic!("expected a slot, got {other:?}"),
    }
}

#[test]
fn on_edge_the_board_rides_the_fence_on_a_face() {
    assert_eq!(on_edge().across(), (Axis::Thickness, true));
    assert_eq!(on_edge().up(), (Axis::Width, false));
    // On the other edge, the same face is toward the fence.
    assert_eq!(on_other_edge().across(), (Axis::Thickness, true));
    assert_eq!(on_other_edge().up(), (Axis::Width, true));
}

#[test]
fn resawing_a_narrow_board_splits_its_thickness() {
    let board = Board::new(inches(24), inches(3), fraction(3, 4));
    let cut = table_saw(fraction(5, 16), all_the_way_up())
        .rip(&board, on_edge())
        .cut_apart();

    // Against the fence, 5/16" thick; the rest past the blade, less a kerf.
    assert_eq!(cut.piece.board.to_string(), "5/16\" x 3\" x 24\"");
    assert_eq!(cut.piece.t, Length::ZERO);
    let offcut = cut.offcut.unwrap();
    assert_eq!(offcut.board.to_string(), "5/16\" x 3\" x 24\"");
    assert_eq!(offcut.t, fraction(7, 16));
    assert_eq!(cut.waste, Blade::FULL_KERF);
    assert!(cut.piece.board.kerfs().is_empty() && offcut.board.kerfs().is_empty());
}

#[test]
fn a_blade_lower_than_the_board_is_wide_leaves_a_slot_in_its_edge() {
    let board = Board::new(inches(24), fraction(11, 2), fraction(3, 4));
    let slotted = kerfed(table_saw(fraction(5, 16), inches(3)).rip(&board, on_edge()));

    let slot = slotted.kerfs()[0];
    assert_eq!(slot.across, Across::Thickness);
    assert_eq!((slot.near, slot.far), (fraction(5, 16), fraction(7, 16)));
    let size = slotted.slot(&slot);
    assert_eq!(size.width, Blade::FULL_KERF);
    assert_eq!(size.depth, inches(3));
    assert!(size.bottom.unwrap().end_to_end);
    assert!(size.top.is_none());
    assert_eq!(slotted.to_string(), board.to_string());
}

#[test]
fn two_passes_from_opposite_edges_that_meet_resaw_a_wide_board() {
    let board = Board::new(inches(24), fraction(11, 2), fraction(3, 4));
    let saw = table_saw(fraction(5, 16), inches(3));
    let once = kerfed(saw.rip(&board, on_edge()));
    let cut = saw.rip(&once, on_other_edge()).cut_apart();

    // 3" up from each edge of a 5-1/2" board meets in the middle.
    assert_eq!(cut.piece.board.to_string(), "5/16\" x 5-1/2\" x 24\"");
    assert_eq!(
        cut.offcut.unwrap().board.to_string(),
        "5/16\" x 5-1/2\" x 24\""
    );
}

#[test]
fn passes_that_do_not_meet_leave_the_board_whole() {
    let board = Board::new(inches(24), inches(7), fraction(3, 4));
    let saw = table_saw(fraction(5, 16), inches(3));
    let once = kerfed(saw.rip(&board, on_edge()));
    let twice = kerfed(saw.rip(&once, on_other_edge()));
    // A 1" web is left between the slots.
    assert_eq!(twice.kerfs().len(), 2);
    assert!(twice.has_wood_in(inches(12), fraction(7, 2), fraction(3, 8)));
    assert!(!twice.has_wood_in(inches(12), inches(1), fraction(3, 8)));
}

#[test]
fn a_groove_through_the_resawn_layer_splits_it_into_strips() {
    let board = Board::new(inches(24), inches(3), fraction(3, 4));
    // A 1/4"-deep groove from below, 1" from the fence.
    let grooved = kerfed(table_saw(inches(1), fraction(1, 4)).rip(&board, Orientation::default()));
    // Resawn on edge, bottom face to the fence, the layer against the fence is 3/16" thick, so
    // the groove goes through it.
    let cut = table_saw(fraction(3, 16), all_the_way_up())
        .rip(&grooved, on_edge())
        .cut_apart();

    let thin = |piece: &Piece| piece.board.thickness() == fraction(3, 16);
    let strips: Vec<&Piece> = std::iter::once(&cut.piece)
        .chain(&cut.loose)
        .filter(|piece| thin(piece))
        .collect();
    assert_eq!(strips.len(), 2);
    let mut widths: Vec<Length> = strips.iter().map(|strip| strip.board.width()).collect();
    widths.sort();
    assert_eq!(widths, [inches(1), fraction(15, 8)]);
    // The thick layer above keeps none of the groove.
    let offcut = cut.offcut.unwrap();
    assert_eq!(offcut.board.thickness(), fraction(7, 16));
    assert!(offcut.board.kerfs().is_empty());
}

#[test]
fn on_edge_the_board_has_to_run_lengthwise_along_the_table_saw() {
    let board = Board::new(inches(24), inches(3), fraction(3, 4));
    assert!(matches!(
        table_saw(inches(6), all_the_way_up()).rip(&board, on_edge().turned_left()),
        Err(CutError::NotModeled(_))
    ));
}

#[test]
fn the_miter_saw_chops_down_through_a_board_on_edge() {
    let board = Board::new(inches(24), fraction(7, 2), fraction(3, 4));
    let cut = MiterSaw::twelve_inch()
        .crosscut(&board, on_edge(), inches(12))
        .cut_apart();
    assert_eq!(cut.piece.board.to_string(), "3/4\" x 3-1/2\" x 12\"");
    assert_eq!(
        cut.offcut.unwrap().board.length(),
        inches(12) - Blade::FULL_KERF
    );
}

#[test]
fn the_miter_saw_only_chops_a_board_on_edge_square() {
    let board = Board::new(inches(24), fraction(7, 2), fraction(3, 4));
    let saw = MiterSaw {
        miter: Angle::from_degrees(45),
        ..MiterSaw::twelve_inch()
    };
    assert!(matches!(
        saw.crosscut(&board, on_edge(), inches(12)),
        Err(CutError::NotModeled(_))
    ));
}

#[test]
fn stopped_short_on_edge_the_miter_saw_scars_the_top_edge() {
    let board = Board::new(inches(24), fraction(7, 2), fraction(3, 4));
    // The blade's lowest point stops 3" up a board 3-1/2" tall.
    let scarred = kerfed(MiterSaw::twelve_inch().chop(&board, on_edge(), inches(12), inches(3)));
    // Still wood near the bottom edge under the cut, none at the top.
    let at_cut = inches(12) - fraction(1, 16);
    assert!(scarred.has_wood_in(at_cut, inches(1), fraction(3, 8)));
    assert!(!scarred.has_wood_in(at_cut, fraction(55, 16), fraction(3, 8)));
}

proptest! {
    #[test]
    fn resawing_conserves_the_thickness(
        fence in 1..12 * 16i64,
        thickness in 2 * 16..2 * 64i64,
    ) {
        let thickness = fraction(thickness, 64);
        let fence = fraction(fence, 64);
        let board = Board::new(inches(24), inches(3), thickness);
        match table_saw(fence, all_the_way_up()).rip(&board, on_edge()) {
            Ok(Pass::Cut(cut)) => {
                let offcut = cut.offcut.as_ref().map_or(Length::ZERO, |offcut| offcut.board.thickness());
                prop_assert_eq!(cut.piece.board.thickness() + offcut + cut.waste, thickness);
                prop_assert_eq!(cut.piece.board.thickness(), fence);
            }
            Ok(pass) => prop_assert!(false, "a full pass left {:?}", pass),
            Err(error) => prop_assert!(fence >= thickness, "{}", error),
        }
    }

    #[test]
    fn cutting_on_edge_and_flat_never_puts_the_same_wood_in_two_pieces(
        passes in prop::collection::vec(
            (0u8..4, 0u8..4, 1..16i64, 4..26i64, 0..2 * 24i64, any::<bool>()),
            1..5,
        ),
    ) {
        let original = Board::new(inches(24), fraction(9, 2), fraction(3, 4));
        let mut board = original.clone();
        let mut corner = (Length::ZERO, Length::ZERO, Length::ZERO);
        let mut set_aside: Vec<(Board, (Length, Length, Length))> = Vec::new();
        for (rolls, turns, fence, height, length, miter_saw) in passes {
            let flat = Orientation::default();
            let mut lie = [flat, flat.tipped(), flat.flipped(), flat.flipped().tipped()][rolls as usize];
            for _ in 0..turns {
                lie = lie.turned_left();
            }
            let pass = if miter_saw {
                MiterSaw::twelve_inch().crosscut(&board, lie, fraction(length, 2))
            } else {
                table_saw(fraction(fence, 16), fraction(height, 8)).rip(&board, lie)
            };
            match pass {
                Ok(Pass::Kerfed(slotted)) => board = slotted,
                Ok(Pass::Cut(cut)) => {
                    let at = |piece: &Piece| (corner.0 + piece.l, corner.1 + piece.w, corner.2 + piece.t);
                    for other in cut.offcut.iter().chain(&cut.loose) {
                        set_aside.push((other.board.clone(), at(other)));
                    }
                    corner = at(&cut.piece);
                    board = cut.piece.board;
                }
                Ok(Pass::Missed) | Err(_) => {}
            }
        }
        set_aside.push((board, corner));

        let nudge = Length::from_nm(12_345);
        let mut l = nudge;
        while l < original.length() {
            let mut w = nudge;
            while w < original.width() {
                let mut t = nudge;
                while t < original.thickness() {
                    let holders = set_aside
                        .iter()
                        .filter(|(piece, at)| piece.has_wood_in(l - at.0, w - at.1, t - at.2))
                        .count();
                    prop_assert!(holders <= 1, "{} pieces have wood at ({}, {}, {})", holders, l, w, t);
                    t = t + fraction(1, 16);
                }
                w = w + fraction(1, 4);
            }
            l = l + fraction(1, 2);
        }
    }
}
