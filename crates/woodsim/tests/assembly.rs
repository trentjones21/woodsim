//! Putting parts together: a picture frame of four mitered pieces that closes, or doesn't, and a
//! butt joint.

mod common;

use common::CutApart;

use woodsim::{
    Angle, Assembly, Board, Corner, Fastener, Length, MiterSaw, Orientation, Surface, surfaces,
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

/// Puts the pieces together into a frame, each one's first miter against the last one's
/// second.
fn frame(pieces: [Board; 4]) -> Assembly {
    let [first, rest @ ..] = pieces;
    let mut assembly = Assembly::of(first);
    for piece in rest {
        let last = assembly.members().len() - 1;
        let onto = (last, miter(&assembly.members()[last].board, true));
        let member = assembly
            .place(&piece, miter(&piece, false), onto, 0, Corner(0))
            .unwrap();
        assembly.join(member, miter(&piece, false), onto, Fastener::Glue);
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
        assert_eq!(member.turn.columns()[2], [0, 0, 1]);
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
fn a_miter_off_by_a_degree_opens_its_joint() {
    let piece = frame_piece(inches(10), 4500, 4500);
    let off = frame_piece(inches(10), 4500, 4600);
    let mut assembly = Assembly::of(piece.clone());
    let onto = (0, miter(&piece, true));
    let member = assembly
        .place(&off, miter(&off, false), onto, 0, Corner(0))
        .unwrap();
    assembly.join(member, miter(&off, false), onto, Fastener::Brads);

    let contact = assembly
        .fit()
        .into_iter()
        .find(|contact| contact.between == [onto, (1, miter(&off, false))])
        .unwrap();
    // It touches at one end of the joint and opens toward the other, about the joint's
    // 2-1/8" length times the tangent of 1°.
    assert!(contact.least.as_nm().abs() < 1_000, "{contact:?}");
    let open = contact.most.as_nm() as f64 / fraction(1, 1).as_nm() as f64;
    assert!((0.03..0.045).contains(&open), "{open}\"");
}

#[test]
fn an_end_butts_against_a_face() {
    let base = Board::new(inches(12), inches(4), fraction(3, 4));
    let upright = Board::new(inches(6), inches(4), fraction(3, 4));
    let end = surfaces(&upright)
        .into_iter()
        .find(|surface| matches!(surface, Surface::Side(edge) if edge.out.l > 0))
        .unwrap();
    let mut assembly = Assembly::of(base.clone());
    let member = assembly
        .place(&upright, end, (0, Surface::Top), 0, Corner(0))
        .unwrap();
    // Standing on its end on the base's top, flush with the base's corner.
    assert_eq!(
        member.point(inches(6), Length::ZERO, Length::ZERO)[2],
        fraction(3, 4)
    );
    assembly.join(member, end, (0, Surface::Top), Fastener::Screws);
    let contact = assembly.fit()[0];
    assert!(contact.is_tight());
    assert_eq!(contact.between, [(0, Surface::Top), (1, end)]);
}

#[test]
fn the_last_member_can_come_back_out() {
    let piece = frame_piece(inches(10), 4500, 4500);
    let mut assembly = frame([piece.clone(), piece.clone(), piece.clone(), piece]);
    assert!(assembly.remove_last().is_some());
    assert_eq!(assembly.members().len(), 3);
    assert_eq!(assembly.joints().len(), 2);
}

#[test]
fn the_best_way_on_for_a_frame_piece_is_its_matching_miter_face_up() {
    let piece = frame_piece(inches(10), 4500, 4500);
    let assembly = Assembly::of(piece.clone());
    let ways = assembly.ways(&piece, (0, miter(&piece, true)));
    assert_eq!(ways[0], (miter(&piece, false), 0));
}

#[test]
fn the_best_way_on_for_a_board_on_a_board_is_flat() {
    let board = Board::new(inches(12), inches(4), fraction(3, 4));
    let assembly = Assembly::of(board.clone());
    let ways = assembly.ways(&board, (0, Surface::Top));
    assert_eq!(ways[0].0, Surface::Bottom);
    // Its ends and edges can go against it too, further down the list.
    assert!(
        ways.iter()
            .any(|(surface, _)| matches!(surface, Surface::Side(_)))
    );
}
