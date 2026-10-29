//! Boards standing on an end: a tenon's cheek kerfed into an end at the table saw, a short
//! block resawn through its length, and a block chopped at the miter saw.

mod common;

use common::CutApart;

use woodsim::{
    Angle, Assembly, Blade, Board, CutError, Fastener, Length, Mating, MiterSaw, Orientation, Pass,
    Surface, TableSaw,
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
        tilt: woodsim::Angle::ZERO,
    }
}

/// Standing on its `l = 0` end, its `t = 0` face against the fence.
fn on_end() -> Orientation {
    Orientation::default().stood()
}

fn kerfed(pass: Result<Pass, CutError>) -> Board {
    match pass {
        Ok(Pass::Kerfed(board)) => board,
        other => panic!("expected a slot, got {other:?}"),
    }
}

#[test]
fn on_end_against_the_fence_the_blade_cuts_a_cheek_into_the_end() {
    let board = Board::new(inches(24), inches(3), fraction(3, 4));
    // A 1/4" cheek, 1-1/2" deep, the way a tenon starts.
    let slotted = kerfed(table_saw(fraction(1, 4), fraction(3, 2)).rip(&board, on_end()));
    let slot = slotted.kerfs()[0];
    assert_eq!((slot.near, slot.far), (fraction(1, 4), fraction(3, 8)));
    let size = slotted.slot(&slot);
    // In through the end it stood on.
    assert!(matches!(
        size.entry,
        Surface::Side(end) if end.out.l < 0 && end.out.w == 0 && end.lean == 0
    ));
    assert_eq!(size.width, Blade::FULL_KERF);
    assert_eq!(size.depth, fraction(3, 2));
    assert!(size.on_entry.unwrap().end_to_end);
    assert!(size.on_exit.is_none());
    // Wood beside the cheek, none in it, and the far end untouched.
    assert!(slotted.has_wood_in(inches(1), inches(1), fraction(1, 8)));
    assert!(!slotted.has_wood_in(inches(1), inches(1), fraction(5, 16)));
    assert!(slotted.has_wood_in(inches(2), inches(1), fraction(5, 16)));
    assert_eq!(slotted.to_string(), board.to_string());
    assert_eq!(slotted.origin().replay(), slotted);
}

#[test]
fn on_end_a_block_shorter_than_the_blade_is_high_is_resawn_through_its_length() {
    let block = Board::new(inches(2), inches(3), fraction(3, 4));
    let cut = table_saw(fraction(1, 4), fraction(25, 8))
        .rip(&block, on_end())
        .cut_apart();
    assert_eq!(cut.piece.board.to_string(), "1/4\" x 3\" x 2\"");
    let offcut = cut.offcut.unwrap();
    assert_eq!(offcut.board.to_string(), "3/8\" x 3\" x 2\"");
    // The piece rode the fence on the `t = 0` face; the offcut is past the kerf.
    assert_eq!(cut.piece.t, Length::ZERO);
    assert_eq!(offcut.t, fraction(3, 8));
    assert_eq!(
        cut.piece.board.thickness() + offcut.board.thickness() + cut.waste,
        block.thickness()
    );
    assert_eq!(cut.piece.board.origin().replay(), cut.piece.board);
    assert_eq!(offcut.board.origin().replay(), offcut.board);
}

#[test]
fn on_end_the_miter_saw_chops_down_through_its_length() {
    let block = Board::new(inches(3), inches(2), fraction(3, 4));
    // Standing on end, its width runs along the fence: the stop measures 1" of it.
    let cut = MiterSaw::twelve_inch()
        .crosscut(&block, on_end(), inches(1))
        .cut_apart();
    assert_eq!(cut.piece.board.to_string(), "3/4\" x 1\" x 3\"");
    let offcut = cut.offcut.unwrap().board;
    assert_eq!(offcut.width(), inches(1) - Blade::FULL_KERF);
    assert_eq!(cut.piece.board.origin().replay(), cut.piece.board);
}

/// A board with a scarf cut on its end: stood on end against the fence, the blade tilted 15° and
/// set just off the fence, so the cut slopes up across its whole thickness.
fn scarfed() -> Board {
    let saw = TableSaw {
        tilt: Angle::from_degrees(15),
        ..table_saw(fraction(1, 64), inches(3))
    };
    let board = Board::new(inches(24), inches(3), fraction(3, 4));
    saw.rip(&board, on_end()).cut_apart().piece.board
}

#[test]
fn a_scarf_cut_on_end_glues_slope_to_slope_into_one_long_board() {
    let board = scarfed();
    // Its end slopes off 15° from its faces, a taper rather than a bevel.
    let [(slope, angle)] = board.tapers()[..] else {
        panic!("expected one sloping face, got {:?}", board.tapers());
    };
    assert_eq!(angle, Angle::from_degrees(15));
    assert!(board.bevels().is_empty());
    let mut scarf = Assembly::of(board.clone());
    assert!(scarf.attach(
        (0, Surface::Side(slope)),
        &Assembly::of(board.clone()),
        (0, Surface::Side(slope)),
        Mating::default(),
        Fastener::Glue,
    ));
    // End to end, the second turned over, glued flush all across the slope.
    let [a, b] = [0, 1].map(|k| scarf.members()[k].axes());
    let dot = |x: [f64; 3], y: [f64; 3]| (0..3).map(|k| x[k] * y[k]).sum::<f64>();
    assert!(dot(a[0], b[0]) < -0.999);
    assert!(dot(a[2], b[2]) < -0.999);
    assert_eq!(scarf.joints().len(), 1);
    assert!(scarf.fit().iter().all(|contact| contact.is_tight()));
    // They overlap by the slope, 3/4" up at 15°, about 2.74" (less the 1/64" tips), and lie
    // as thick as one board and a tip.
    let [(start, end), _, (bottom, top)] = scarf.bounds();
    let overlap = 48.0 - (end - start).as_nm() as f64 / 25_400_000.0;
    let slope_length = (0.75 - 1.0 / 64.0) / 15f64.to_radians().tan();
    assert!((overlap - slope_length).abs() < 1e-3, "{overlap}");
    assert_eq!(top - bottom, fraction(49, 64));
}
