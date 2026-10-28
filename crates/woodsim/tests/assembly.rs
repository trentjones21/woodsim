//! Putting parts together: a picture frame of four mitered pieces that closes, or doesn't, two
//! halves of a frame put together, a butt joint, and boards edge to edge.

mod common;

use common::CutApart;

use woodsim::{
    Angle, Assembly, Board, Fastener, Length, Mating, MiterSaw, Orientation, Surface, surfaces,
};

fn inches(n: i64) -> Length {
    Length::from_inches(n)
}

fn fraction(numerator: i64, denominator: i64) -> Length {
    Length::from_inch_fraction(numerator, denominator)
}

/// 1x2 stock: 3/4" x 1-1/2" x 48".
fn one_by_two() -> Board {
    Board::new(inches(48), fraction(3, 2), fraction(3, 4))
}

fn mitered(hundredths: i32) -> MiterSaw {
    MiterSaw {
        miter: Angle::from_hundredths(hundredths),
        ..MiterSaw::twelve_inch()
    }
}

/// A frame piece `length` long point to long point, mitered `far` hundredths of a degree at its
/// `l = length` end and `near` at its `l = 0` end, long points along its `w = width` edge: trim
/// the stock's end swung away from the stop, slide the offcut back against the stop, and cut it
/// to length swung toward it.
fn frame_piece(length: Length, far: i32, near: i32) -> Board {
    let trimmed = mitered(-far)
        .crosscut(&one_by_two(), Orientation::default(), inches(2))
        .cut_apart()
        .offcut
        .unwrap()
        .board;
    mitered(near)
        .crosscut(&trimmed, Orientation::default(), length)
        .cut_apart()
        .piece
        .board
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

/// The end of a square-cut board at its `l = length` end.
fn far_end(board: &Board) -> Surface {
    surfaces(board)
        .into_iter()
        .find(|surface| matches!(surface, Surface::Side(edge) if edge.out.l > 0))
        .unwrap()
}

/// The way `other`'s `surface` goes against `onto` that keeps it face up, flush at the least
/// sides.
fn face_up(
    assembly: &Assembly,
    onto: (usize, Surface),
    other: &Assembly,
    surface: (usize, Surface),
) -> Mating {
    lying(assembly, onto, other, surface, 1.0)
}

/// The way `other`'s `surface` goes against `onto` that leaves its top facing `up` (1 for up,
/// -1 for down), flush at the least sides.
fn lying(
    assembly: &Assembly,
    onto: (usize, Surface),
    other: &Assembly,
    surface: (usize, Surface),
    up: f64,
) -> Mating {
    (0..4)
        .map(|turn| Mating {
            turn,
            slide: [Length::ZERO; 2],
        })
        .find(|&mating| {
            let motion = assembly.mating(onto, other, surface, mating).unwrap();
            motion.rotation.apply([0.0, 0.0, 1.0])[2] * up > 0.999
        })
        .expect("a way that lies that way up")
}

/// Puts `piece`'s `surface` against `onto`, face up and flush.
fn add(assembly: &mut Assembly, onto: (usize, Surface), piece: &Board, surface: Surface) {
    let other = Assembly::of(piece.clone());
    let mating = face_up(assembly, onto, &other, (0, surface));
    assert!(assembly.attach(onto, &other, (0, surface), mating, Fastener::Glue));
}

/// Puts the pieces together into a frame, each one's first miter against the last one's
/// second.
fn frame(pieces: [Board; 4]) -> Assembly {
    let [first, rest @ ..] = pieces;
    let mut assembly = Assembly::of(first);
    for piece in rest {
        let last = assembly.members().len() - 1;
        let onto = (last, miter(&assembly.members()[last].board, true));
        add(&mut assembly, onto, &piece, miter(&piece, false));
    }
    assembly
}

/// How the last piece's far end meets the first piece's near end, as least and most apart.
fn closing(assembly: &Assembly) -> (Length, Length) {
    let first = miter(&assembly.members()[0].board, false);
    let last = miter(&assembly.members()[3].board, true);
    let contact = assembly
        .fit()
        .into_iter()
        .find(|contact| contact.between == [(0, first), (3, last)])
        .expect("the last corner meets the first piece");
    (contact.least, contact.most)
}

/// The angle between two members' lengths, in degrees.
fn between(assembly: &Assembly, a: usize, b: usize) -> f64 {
    let [first, second] = [a, b].map(|k| assembly.members()[k].axes()[0]);
    (first[0] * second[0] + first[1] * second[1] + first[2] * second[2])
        .acos()
        .to_degrees()
}

#[test]
fn four_matching_pieces_close_into_a_square() {
    let piece = frame_piece(inches(10), 4500, 4500);
    assert_eq!(piece.miters(), [Angle::from_degrees(45); 2]);
    let assembly = frame([piece.clone(), piece.clone(), piece.clone(), piece]);

    assert_eq!(closing(&assembly), (Length::ZERO, Length::ZERO));
    assert!(assembly.fit().iter().all(|contact| contact.is_tight()));
    // Four joints, all in the frame's plane: each piece is face up like the first.
    assert_eq!(assembly.joints().len(), 3);
    for member in assembly.members() {
        assert_eq!(member.axes()[2], [0.0, 0.0, 1.0]);
    }
    // Around a 10" square, the far corners are 10" apart.
    let outside = |member: usize, l: Length| {
        let board = &assembly.members()[member].board;
        assembly.members()[member].point(l, board.width(), Length::ZERO)
    };
    let (first, third) = (outside(0, Length::ZERO), outside(2, inches(10)));
    assert_eq!(
        [first[0] - third[0], first[1] - third[1]].map(|d| d.as_nm().abs()),
        [0, inches(10).as_nm()]
    );
}

#[test]
fn a_long_side_leaves_the_last_corner_open() {
    let piece = frame_piece(inches(10), 4500, 4500);
    let long = frame_piece(inches(10) + fraction(1, 16), 4500, 4500);
    let assembly = frame([piece.clone(), piece.clone(), long, piece]);

    // The bottom is 1/16" longer than the top, so the left side ends up 1/16" further out
    // than the top's end, and across the 45° miter that's 1/16" x the cosine of 45°.
    let (least, most) = closing(&assembly);
    let expected = fraction(1, 16).as_nm() as f64 / 2f64.sqrt();
    assert!((least.as_nm() as f64 - expected).abs() < 2.0, "{least:?}");
    assert_eq!(least, most);
}

#[test]
fn a_miter_off_by_a_degree_closes_but_leaves_the_corner_out_of_square() {
    let piece = frame_piece(inches(10), 4500, 4500);
    let off = frame_piece(inches(10), 4500, 4600);
    let mut assembly = Assembly::of(piece.clone());
    add(
        &mut assembly,
        (0, miter(&piece, true)),
        &off,
        miter(&off, false),
    );

    // Clamped together, the joint closes, the way a real one would, and the corner is 91°
    // (or 89°) instead of square: the error shows up across the frame.
    assert!(assembly.fit().iter().all(|contact| contact.is_tight()));
    let corner = between(&assembly, 0, 1);
    assert!(
        (corner - 91.0).abs() < 1e-4 || (corner - 89.0).abs() < 1e-4,
        "{corner}"
    );
}

#[test]
fn two_halves_of_a_frame_close_into_a_square() {
    let piece = frame_piece(inches(10), 4500, 4500);
    let half = |piece: &Board| {
        let mut half = Assembly::of(piece.clone());
        add(
            &mut half,
            (0, miter(piece, true)),
            piece,
            miter(piece, false),
        );
        half
    };
    let mut assembly = half(&piece);
    let other = half(&piece);
    // The second half's first piece goes against the first half's last.
    let onto = (1, miter(&piece, true));
    let surface = (0, miter(&piece, false));
    let mating = face_up(&assembly, onto, &other, surface);
    assert!(assembly.attach(onto, &other, surface, mating, Fastener::Glue));

    assert_eq!(assembly.members().len(), 4);
    // Its own joint comes along, renumbered, and the new one joins the halves.
    assert_eq!(assembly.joints().len(), 3);
    assert_eq!(assembly.joints()[1].between[0].0, 2);
    assert_eq!(assembly.joints()[2].between, [onto, (2, surface.1)]);
    assert_eq!(closing(&assembly), (Length::ZERO, Length::ZERO));
    assert!(assembly.fit().iter().all(|contact| contact.is_tight()));
}

#[test]
fn an_end_butts_against_a_face_where_it_is_slid() {
    let base = Board::new(inches(12), inches(4), fraction(3, 4));
    let upright = Board::new(inches(6), inches(4), fraction(3, 4));
    let end = far_end(&upright);
    let assembly = Assembly::of(base.clone());
    let other = Assembly::of(upright.clone());
    let onto = (0, Surface::Top);

    // Across the base's top, along its length and its width.
    let extent = assembly.extent(onto).unwrap();
    assert_eq!(
        extent.reach,
        [(Length::ZERO, inches(12)), (Length::ZERO, inches(4))]
    );
    // One way around, the upright's width runs along the base's length; a quarter turn on, its
    // thickness does. Either way, it can sit flush at one side, centered, or flush at the other.
    let stops = |turn| assembly.stops(onto, &other, (0, end), turn).unwrap();
    let along_length = (0..4)
        .find(|&turn| stops(turn)[0][2] == inches(8))
        .expect("a way with the width along the length");
    assert_eq!(
        stops(along_length),
        [
            [Length::ZERO, inches(4), inches(8)],
            [Length::ZERO, fraction(13, 8), fraction(13, 4)]
        ]
    );
    let across = (along_length + 1) % 4;
    assert_eq!(stops(across)[0][2], inches(12) - fraction(3, 4));

    // Centered along the length and flush with the far edge.
    let mating = Mating {
        turn: along_length,
        slide: [inches(4), fraction(13, 4)],
    };
    let mut glued = assembly.clone();
    assert!(glued.attach(onto, &other, (0, end), mating, Fastener::Screws));
    let member = &glued.members()[1];
    // Standing on its end on the base's top, between 4" and 8" along it.
    let foot = [Length::ZERO, upright.width()].map(|w| member.point(inches(6), w, Length::ZERO));
    for corner in foot {
        assert_eq!(corner[2], fraction(3, 4));
    }
    let mut along: Vec<Length> = foot.iter().map(|corner| corner[0]).collect();
    along.sort();
    assert_eq!(along, [inches(4), inches(8)]);
    let contact = glued.fit()[0];
    assert!(contact.is_tight());
    assert_eq!(contact.between, [(0, Surface::Top), (1, end)]);
}

#[test]
fn boards_edge_to_edge_make_a_panel() {
    let board = Board::new(inches(24), inches(4), fraction(3, 4));
    let edge = |far: bool| {
        surfaces(&board)
            .into_iter()
            .find(|surface| {
                matches!(surface, Surface::Side(edge) if edge.out.l == 0 && (edge.out.w > 0) == far)
            })
            .unwrap()
    };
    let mut panel = Assembly::of(board.clone());
    add(&mut panel, (0, edge(true)), &board, edge(false));
    add(&mut panel, (1, edge(true)), &board, edge(false));

    assert!(panel.fit().iter().all(|contact| contact.is_tight()));
    assert_eq!(panel.fit().len(), 2);
    // Three 4" boards side by side, ends flush: 12" across.
    let far = panel.members()[2].point(inches(24), inches(4), Length::ZERO);
    assert_eq!(far, [inches(24), inches(12), Length::ZERO]);
}

#[test]
fn two_fifty_degree_miters_meet_flush_at_eighty_degrees() {
    let saw = mitered(5000);
    let cut = saw
        .crosscut(&one_by_two(), Orientation::default(), inches(12))
        .cut_apart();
    let (piece, offcut) = (cut.piece.board, cut.offcut.unwrap().board);
    let mut assembly = Assembly::of(piece.clone());
    let offcut_miter = surfaces(&offcut)
        .into_iter()
        .find(
            |surface| matches!(surface, Surface::Side(edge) if edge.out.l != 0 && edge.out.w != 0),
        )
        .unwrap();
    let onto = (0, miter(&piece, false));
    let other = Assembly::of(offcut);
    // Face up, the offcut's miter against the piece's puts back the board they were cut from.
    let mut uncut = assembly.clone();
    let mating = face_up(&uncut, onto, &other, (0, offcut_miter));
    uncut.attach(onto, &other, (0, offcut_miter), mating, Fastener::Glue);
    assert!(between(&uncut, 0, 1) < 1e-4);
    // Flipped over, it makes a corner.
    let mating = lying(&assembly, onto, &other, (0, offcut_miter), -1.0);
    assembly.attach(onto, &other, (0, offcut_miter), mating, Fastener::Glue);

    assert!(
        assembly.fit().iter().all(|contact| contact.is_tight()),
        "{:?}",
        assembly.fit()
    );
    // The two lengths are 80° apart (or 100°, the other way around the corner).
    let corner = between(&assembly, 0, 1);
    assert!(
        (corner - 80.0).abs() < 1e-4 || (corner - 100.0).abs() < 1e-4,
        "{corner}"
    );
}

#[test]
fn eight_pieces_mitered_at_22_and_a_half_degrees_close_into_an_octagon() {
    let piece = frame_piece(inches(6), 2250, 2250);
    let mut assembly = Assembly::of(piece.clone());
    for last in 0..7 {
        let onto = (last, miter(&assembly.members()[last].board, true));
        add(&mut assembly, onto, &piece, miter(&piece, false));
    }
    // The last piece meets the first within a micron.
    assert!(
        assembly.fit().iter().all(|contact| contact.is_tight()),
        "{:?}",
        assembly.fit()
    );
    assert_eq!(assembly.fit().len(), 8);
}
