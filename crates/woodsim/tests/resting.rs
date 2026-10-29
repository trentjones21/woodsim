//! Resting what's on a saw on a surface you pick, with another against the fence: any way at all,
//! the square ways and the ways a bevel or a glue-up at an angle sits.

mod common;

use common::CutApart;

use woodsim::{
    Angle, Assembly, Blade, Board, Fastener, Length, Lie, Mating, Orientation, Pass, Recipe,
    Surface, TableSaw, Tool, Unsteady, Wood, surfaces,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

fn one_by_six() -> Board {
    Board::new(inches(24), fraction(11, 2), fraction(3, 4))
}

/// A table saw with a 10" blade 4" up, its fence `fence` from it, tilted `tilt` degrees.
fn table_saw(fence: Length, tilt: i32) -> TableSaw {
    TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence,
        height: inches(4),
        miter_gauge: None,
        gauge_angle: Angle::ZERO,
        tilt: Angle::from_degrees(tilt),
    }
}

/// The way `surface` of `board` faces out of its wood, in the board's own axes.
fn facing(surface: Surface) -> [f64; 3] {
    match surface {
        Surface::Bottom => [0.0, 0.0, -1.0],
        Surface::Top => [0.0, 0.0, 1.0],
        Surface::Side(edge) | Surface::Slot(edge) => {
            let v = [edge.out.l as f64, edge.out.w as f64, edge.lean as f64];
            let size = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            v.map(|c| c / size)
        }
    }
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[test]
fn a_board_rests_on_any_face_edge_or_end_as_it_lies_each_square_way() {
    let board = one_by_six();
    let alone = Assembly::of(board.clone());
    let flat = Orientation::default();
    for mut lie in [
        flat,
        flat.tipped(),
        flat.flipped(),
        flat.flipped().tipped(),
        flat.stood(),
        flat.stood().flipped(),
    ] {
        for _ in 0..4 {
            // Whichever surface lies on the table, and whichever faces the fence.
            let way = |toward: [f64; 3]| {
                surfaces(&board)
                    .into_iter()
                    .find(|&surface| dot(lie.rotation().apply(facing(surface)), toward) > 0.999)
                    .expect("a box has a surface every square way")
            };
            let (down, fence) = (way([0.0, 0.0, -1.0]), way([0.0, 1.0, 0.0]));
            assert_eq!(alone.resting((0, down), (0, fence)), Ok(lie), "{lie:?}");
            lie = lie.turned_left();
        }
    }
}

#[test]
fn it_wont_rest_where_it_would_rock_or_meet_the_fence_first() {
    let board = one_by_six();
    let alone = Assembly::of(board.clone());
    // `board`'s edge facing `w`'s way across its width.
    let edge = |board: &Board, w: i64| {
        surfaces(board)
            .into_iter()
            .find(|surface| matches!(surface, Surface::Side(edge) if edge.out.l == 0 && edge.out.w.signum() == w))
            .unwrap()
    };
    assert_eq!(
        alone.resting((0, Surface::Bottom), (0, Surface::Top)),
        Err(Unsteady::FacesUp)
    );
    // A 1x2 glued on top along one edge: it won't rest on the 1x6's top, which the 1x2 stands
    // on, nor go against the fence by the 1x2's inner edge, with the 1x6 reaching past it.
    let mut stacked = Assembly::of(board.clone());
    let one_by_two = Board::new(inches(24), fraction(3, 2), fraction(3, 4));
    assert!(stacked.attach(
        (0, Surface::Top),
        &Assembly::of(one_by_two.clone()),
        (0, Surface::Bottom),
        Mating::default(),
        Fastener::Glue,
    ));
    assert_eq!(
        stacked.resting((0, Surface::Top), (0, edge(&board, 1))),
        Err(Unsteady::Rocks)
    );
    let inner = [edge(&one_by_two, 1), edge(&one_by_two, -1)]
        .into_iter()
        .find(|&side| stacked.resting((0, Surface::Bottom), (1, side)) == Err(Unsteady::Blocked));
    assert!(
        inner.is_some(),
        "one of the 1x2's edges is inside the 1x6's"
    );
    assert!(
        stacked
            .resting((0, Surface::Bottom), (0, edge(&board, 1)))
            .is_ok()
    );
}

#[test]
fn a_board_rests_on_its_bevel_and_rips_square_to_it() {
    // A 30° bevel along one edge, then the board laid on it, an edge to the fence.
    let beveled = table_saw(inches(4), 30)
        .rip(&one_by_six(), Orientation::default())
        .cut_apart()
        .piece
        .board;
    let [(bevel, _)] = beveled.bevels()[..] else {
        panic!("expected one bevel");
    };
    let alone = Assembly::of(beveled.clone());
    let lie = surfaces(&beveled)
        .into_iter()
        .find_map(|fence| alone.resting((0, Surface::Side(bevel)), (0, fence)).ok())
        .expect("some surface can go to the fence");
    assert!(!lie.is_square());
    // The whole bevel is down on the table.
    let lowest = beveled
        .corners()
        .into_iter()
        .map(|[l, w, t]| lie.rotation().apply([l, w, t].map(|v| v.as_nm() as f64))[2])
        .fold(f64::MAX, f64::min);
    for polygon in woodsim::outline(&beveled, Surface::Side(bevel)) {
        for [l, w, t] in polygon {
            let z = lie.rotation().apply([l, w, t].map(|v| v.as_nm() as f64))[2];
            assert!((z - lowest).abs() < 2.0, "{z} above {lowest}");
        }
    }
    // Ripped resting on it, the blade cuts square to the bevel.
    let cut = table_saw(inches(1), 0).rip(&beveled, lie).cut_apart();
    for part in std::iter::once(&cut.piece).chain(&cut.offcut) {
        assert_eq!(part.board.origin().replay(), part.board);
        let [(_, angle)] = part.board.bevels()[..] else {
            continue;
        };
        // What was square to the face is now 30° off; the new cut, square to the bevel, is
        // 60° off the face, a taper or a bevel as it leans.
        assert!(angle == Angle::from_degrees(30) || angle == Angle::from_degrees(60));
    }
    let new_face = cut
        .piece
        .board
        .bevels()
        .into_iter()
        .chain(cut.piece.board.tapers())
        .map(|(edge, _)| facing(Surface::Side(edge)))
        .find(|&n| dot(n, facing(Surface::Side(bevel))).abs() < 1e-6);
    assert!(
        new_face.is_some(),
        "the piece has a face square to the bevel: {:?}",
        cut.piece.board.face()
    );
}

#[test]
fn a_slot_in_a_board_resting_on_its_bevel_is_measured_from_the_bevel() {
    // Copied from the game: a 1x3 with a 30° bevel, resting on it with its bottom face to the
    // fence, partway through a rip with the blade all the way up.
    let copied = "\
        # Woodshop Simulator, copied: on the table saw.\n\
        stock s0 609600000 63500000 19050000\n\
        pass p0 1 table-saw 254000000 3175000 38100000 50800000 none 0 3000 board \
            1099511627776 0 0 0 1099511627776 0 0 0 1099511627776 -127000000 736600000\n\
        sawn s1 s0 p0 0 8388608 4843165 182316545036014 213070643200000 1 0 0 127000000 \
            776113999901651 -127000000 736600000 piece 1 0 14401477 0 measured\n\
        board s1\n\
        saw table-saw 254000000 3175000 25400000 79375000 none 0 0 board 1099511627776 0 0 0 \
            549755806488 952205005683 0 -952205005683 549755806488 -152400000 203200000\n";
    let recipe = Recipe::read(copied).unwrap();
    let Wood::Board(board) = recipe.wood else {
        panic!("a board");
    };
    let Some(Tool::TableSaw {
        saw,
        lie: Lie::Board(lie),
        travel,
    }) = recipe.saw
    else {
        panic!("the table saw");
    };
    let Ok(Pass::Kerfed(slotted)) = saw.feed(&board, lie, travel.0, travel.1) else {
        panic!("expected a slot partway through");
    };
    let size = slotted.slot(slotted.kerfs().last().unwrap());
    // In through the bevel it rests on, not an edge square to the board's faces.
    let [(bevel, _)] = board.bevels()[..] else {
        panic!("one bevel");
    };
    assert_eq!(size.entry, Surface::Side(bevel));
    // Along the bevel from the leading end to where the teeth reach at the table: the blade's
    // center 8" along, 1-7/8" below the table, its teeth 4.635" ahead of it there.
    let reach = 8.0 + (25.0f64 - 1.875 * 1.875).sqrt();
    let run = size.on_entry.expect("it opens onto the bevel");
    assert!(
        (run.length.as_nm() as f64 / 25_400_000.0 - reach).abs() < 1e-3,
        "{:?}",
        run.length
    );
    assert!(!run.end_to_end);
    // The blade, all the way up, goes through: out its top face, facing up and away from the
    // fence its bottom face rests against, where the blade's reached less far along.
    assert_eq!(size.exit, Surface::Top);
    let out = size.on_exit.expect("it comes out the top face").length;
    assert!(out > Length::from_inches(8) && out < run.length, "{out:?}");
}
