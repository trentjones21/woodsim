//! What slots leave to put things against: a rabbet's bottom and side, a groove's bottom and its
//! two sides, and the faces and edges around them, wood only where no slot took it. Parts go
//! into them as they go against any other surface, and fasten wherever they meet flush.

use woodsim::{
    Angle, Assembly, Blade, Board, Fastener, Length, Mating, Orientation, Pass, Recipe, Surface,
    TableSaw, Wood, exposed, outline, slot_bottoms, slot_surfaces, surfaces,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

/// A rabbet 7/8" wide and 3/8" deep across the end of a 1x8, nibbled out with the miter gauge
/// in seven passes, as copied at the bench.
const RABBET: &str = include_str!("pasted/rabbet-across-the-end.txt");

fn rabbeted() -> Board {
    let Wood::GlueUp(glue_up) = Recipe::read(RABBET).unwrap().wood else {
        panic!("a glue-up of one");
    };
    glue_up.members()[0].board.clone()
}

/// How much of a surface there is, in square inches, from its polygons.
fn area(polygons: &[Vec<[Length; 3]>]) -> f64 {
    let square_inch = 25_400_000f64.powi(2);
    polygons
        .iter()
        .map(|polygon| {
            // Half the length of the sum of the corners' cross products (Newell's method).
            let points: Vec<[f64; 3]> = polygon
                .iter()
                .map(|p| p.map(|at| at.as_nm() as f64))
                .collect();
            let mut sum = [0.0; 3];
            for (i, p) in points.iter().enumerate() {
                let q = points[(i + 1) % points.len()];
                sum[0] += p[1] * q[2] - p[2] * q[1];
                sum[1] += p[2] * q[0] - p[0] * q[2];
                sum[2] += p[0] * q[1] - p[1] * q[0];
            }
            (sum[0] * sum[0] + sum[1] * sum[1] + sum[2] * sum[2]).sqrt() / 2.0
        })
        .sum::<f64>()
        / square_inch
}

/// The least and most of a surface's corners on each axis.
fn bounds(polygons: &[Vec<[Length; 3]>]) -> [(Length, Length); 3] {
    let mut bounds = [(Length::from_nm(i64::MAX), Length::from_nm(i64::MIN)); 3];
    for corner in polygons.iter().flatten() {
        for (bound, &at) in bounds.iter_mut().zip(corner) {
            *bound = (bound.0.min(at), bound.1.max(at));
        }
    }
    bounds
}

/// The side of `board`'s outline facing along `out`: `(l, w)`, each -1, 0, or 1.
fn side(board: &Board, out: (i64, i64)) -> Surface {
    surfaces(board)
        .into_iter()
        .find(|surface| {
            matches!(surface, Surface::Side(edge) if (edge.out.l.signum(), edge.out.w.signum()) == out)
        })
        .unwrap()
}

/// The way `other`'s `surface` goes against `onto` with its thickness running along `axis` of
/// this assembly, slid to the `stop`th stop each way (flush with the least side, centered, or
/// flush with the most).
fn mating(
    assembly: &Assembly,
    onto: (usize, Surface),
    other: &Assembly,
    surface: (usize, Surface),
    axis: usize,
    stop: [usize; 2],
) -> Mating {
    let turn = (0..4)
        .find(|&turn| {
            let mating = Mating {
                turn,
                slide: [Length::ZERO; 2],
            };
            let motion = assembly.mating(onto, other, surface, mating).unwrap();
            motion.rotation.apply([0.0, 0.0, 1.0])[axis].abs() > 0.999
        })
        .expect("a way with its thickness that way");
    let stops = assembly.stops(onto, other, surface, turn).unwrap();
    Mating {
        turn,
        slide: [stops[0][stop[0]], stops[1][stop[1]]],
    }
}

#[test]
fn a_rabbet_across_an_end_leaves_its_bottom_and_one_side() {
    let board = rabbeted();
    let (length, width, thickness) = (board.length(), board.width(), board.thickness());
    assert_eq!(board.kerfs().len(), 7);
    let (wide, deep) = (fraction(7, 8), fraction(3, 8));

    // The seven strokes share one bottom, 3/8" up from the bottom face and facing down, and the
    // only side left is the last stroke's far one, the shoulder, 7/8" in from the end.
    let slots = slot_surfaces(&board);
    assert_eq!(slots.len(), 2, "{slots:?}");
    let [Surface::Slot(shoulder), Surface::Slot(bottom)] = slots[..] else {
        panic!("expected two slot surfaces, got {slots:?}");
    };
    assert_eq!(
        slot_bottoms(&board)
            .iter()
            .filter(|&&b| b == bottom)
            .count(),
        7
    );
    assert!(!slot_bottoms(&board).contains(&shoulder));
    assert_eq!((bottom.out.l, bottom.out.w, bottom.lean), (0, 0, -1));
    assert_eq!(bottom.at, -(deep.as_nm() as i128));
    assert_eq!((shoulder.out.l, shoulder.out.w, shoulder.lean), (-1, 0, 0));
    assert_eq!(shoulder.at, -(wide.as_nm() as i128));

    // Each one piece, across the whole width.
    let floor = exposed(&board, Surface::Slot(bottom));
    assert_eq!(floor.len(), 1, "{floor:?}");
    assert_eq!(
        bounds(&floor),
        [(Length::ZERO, wide), (Length::ZERO, width), (deep, deep)]
    );
    assert_eq!(outline(&board, Surface::Slot(bottom)), floor);
    let wall = exposed(&board, Surface::Slot(shoulder));
    assert_eq!(wall.len(), 1, "{wall:?}");
    assert_eq!(
        bounds(&wall),
        [(wide, wide), (Length::ZERO, width), (Length::ZERO, deep)]
    );

    // The bottom face stops at the shoulder, and the end is wood only above the rabbet; the
    // top is untouched.
    let face = exposed(&board, Surface::Bottom);
    assert_eq!(face.len(), 1, "{face:?}");
    assert_eq!(
        bounds(&face),
        [
            (wide, length),
            (Length::ZERO, width),
            (Length::ZERO, Length::ZERO)
        ]
    );
    assert_eq!(exposed(&board, Surface::Top), outline(&board, Surface::Top));
    let end = exposed(&board, side(&board, (-1, 0)));
    assert_eq!(
        bounds(&end),
        [
            (Length::ZERO, Length::ZERO),
            (Length::ZERO, width),
            (deep, thickness)
        ]
    );
    // Each edge has the rabbet's notch out of its corner.
    let inches = |length: Length| length.as_nm() as f64 / 25_400_000.0;
    for out in [(0, -1), (0, 1)] {
        let edge = exposed(&board, side(&board, out));
        let expected = inches(length) * inches(thickness) - inches(wide) * inches(deep);
        assert!((area(&edge) - expected).abs() < 1e-9, "{edge:?}");
    }
}

#[test]
fn a_board_set_into_a_rabbet_is_glued_to_its_bottom_and_its_side() {
    let board = rabbeted();
    let [_, Surface::Slot(bottom)] = slot_surfaces(&board)[..] else {
        panic!("expected the rabbet's side and bottom");
    };
    let assembly = Assembly::of(board.clone());
    let onto = (0, Surface::Slot(bottom));
    // Across the rabbet's bottom: 7/8" along the length, the board's whole width across it.
    let extent = assembly.extent(onto).unwrap();
    assert_eq!(extent.reach[0], (Length::ZERO, fraction(7, 8)));

    // A 1x8 standing on its end in the rabbet, its thickness across the rabbet's width, pushed
    // up against the shoulder.
    let upright = Board::new(inches(6), board.width(), fraction(3, 4));
    let other = Assembly::of(upright.clone());
    let end = (0, side(&upright, (1, 0)));
    let mating = mating(&assembly, onto, &other, end, 0, [2, 0]);
    let mut glued = assembly.clone();
    assert!(glued.attach(onto, &other, end, mating, Fastener::Glue));

    // Its end sits on the rabbet's bottom, and its face against the shoulder: both tight, and
    // both glued.
    let fit = glued.fit();
    assert!(fit.iter().all(|contact| contact.is_tight()), "{fit:?}");
    let joints: Vec<Surface> = glued
        .joints()
        .iter()
        .map(|joint| joint.between[0].1)
        .collect();
    assert_eq!(joints.len(), 2, "{joints:?}");
    assert!(joints.contains(&Surface::Slot(bottom)));
    assert!(
        joints
            .iter()
            .any(|surface| matches!(surface, Surface::Slot(edge) if edge.out.l == -1))
    );
    // Standing from 1/8" to 7/8" along the rabbeted board, from the rabbet's bottom down.
    let member = &glued.members()[1];
    let corners: Vec<[Length; 3]> = [Length::ZERO, upright.thickness()]
        .into_iter()
        .map(|t| member.point(upright.length(), Length::ZERO, t))
        .collect();
    let mut along: Vec<Length> = corners.iter().map(|corner| corner[0]).collect();
    along.sort();
    assert_eq!(along, [fraction(1, 8), fraction(7, 8)]);
    assert!(corners.iter().all(|corner| corner[2] == fraction(3, 8)));

    // Written out and read back, it's glued the same way.
    let recipe = Recipe {
        wood: Wood::GlueUp(glued.clone()),
        saw: None,
    };
    let text = recipe.write();
    assert!(text.contains(" slot 0 0 -1 "), "{text}");
    let Wood::GlueUp(read) = Recipe::read(&text).unwrap().wood else {
        panic!("a glue-up");
    };
    assert_eq!(read.joints(), glued.joints());
}

#[test]
fn a_panel_in_a_groove_is_glued_to_its_bottom_and_both_sides() {
    // A 1/8" groove 1/4" deep along a 1x4's bottom face, 2" from the edge against the fence.
    let saw = TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence: inches(2),
        height: fraction(1, 4),
        miter_gauge: None,
        gauge_angle: Angle::ZERO,
        tilt: Angle::ZERO,
    };
    let board = Board::new(inches(24), inches(4), fraction(3, 4));
    let Ok(Pass::Kerfed(grooved)) = saw.rip(&board, Orientation::default()) else {
        panic!("expected a groove");
    };
    let slots = slot_surfaces(&grooved);
    assert_eq!(slots.len(), 3, "{slots:?}");
    let bottom = *slots
        .iter()
        .find(|surface| matches!(surface, Surface::Slot(edge) if slot_bottoms(&grooved).contains(edge)))
        .unwrap();
    assert_eq!(
        bounds(&exposed(&grooved, bottom))[2],
        (fraction(1, 4), fraction(1, 4))
    );
    // The bottom face is in two, either side of the groove, the groove's width less.
    let face = exposed(&grooved, Surface::Bottom);
    assert_eq!(face.len(), 2, "{face:?}");
    assert!((area(&face) - 24.0 * (4.0 - 0.125)).abs() < 1e-9);

    // A 1/8" panel's edge in the groove fits it exactly.
    let panel = Board::new(inches(24), inches(6), fraction(1, 8));
    let assembly = Assembly::of(grooved.clone());
    let other = Assembly::of(panel.clone());
    let onto = (0, bottom);
    let edge = (0, side(&panel, (0, 1)));
    let mating = mating(&assembly, onto, &other, edge, 1, [0, 0]);
    let mut glued = assembly.clone();
    assert!(glued.attach(onto, &other, edge, mating, Fastener::Glue));
    let fit = glued.fit();
    assert!(fit.iter().all(|contact| contact.is_tight()), "{fit:?}");
    assert_eq!(glued.joints().len(), 3, "{:?}", glued.joints());
    assert!(
        glued
            .joints()
            .iter()
            .all(|joint| matches!(joint.between[0].1, Surface::Slot(_)))
    );
}

#[test]
fn a_board_with_no_slots_is_wood_all_over() {
    let board = Board::new(inches(24), inches(4), fraction(3, 4));
    assert!(slot_surfaces(&board).is_empty());
    for surface in surfaces(&board) {
        assert_eq!(exposed(&board, surface), outline(&board, surface));
    }
}
