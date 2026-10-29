//! Every board remembers where it came from: the stock it started as and every cut since, with
//! the saw's settings, numbered in the order they happened, so a project can be made again in
//! a real shop the way it was made here. Making the cuts again from the stock makes each board
//! again, exactly.

mod common;

use std::sync::Arc;

use common::CutApart;
use proptest::{prelude::*, sample::Index};
use woodsim::{
    Angle, Assembly, Blade, Board, Fastener, Length, Lie, Mating, MiterSaw, Operation, Orientation,
    Part, Pass, Rotation, Source, Surface, TableSaw, Tool, surfaces,
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

fn table_saw(fence: Length, height: Length) -> TableSaw {
    TableSaw {
        blade: Blade {
            diameter: inches(10),
            kerf: Blade::FULL_KERF,
        },
        fence,
        height,
        miter_gauge: None,
    }
}

fn mitered(hundredths: i32) -> MiterSaw {
    MiterSaw {
        miter: Angle::from_hundredths(hundredths),
        ..MiterSaw::twelve_inch()
    }
}

/// The last saw pass a board went through.
fn last(board: &Board) -> Arc<Operation> {
    board
        .origin()
        .operations()
        .pop()
        .expect("a board that's been cut")
}

#[test]
fn new_lumber_is_its_own_stock_and_boards_compare_by_wood_not_history() {
    let (a, b) = (one_by_two(), one_by_two());
    assert_eq!(a, b);
    assert!(!a.origin().same(b.origin()));
    assert!(a.origin().same(a.clone().origin()));
    assert!(matches!(
        a.origin().source(),
        Source::Stock { length, .. } if *length == inches(48)
    ));
    assert!(a.origin().operations().is_empty());
    assert_eq!(a.origin().at_on_stock(), [Length::ZERO; 3]);
    assert_eq!(a.origin().replay(), a);
}

#[test]
fn pieces_cut_one_after_another_remember_where_they_were_on_the_stock() {
    let saw = mitered(0);
    let first = saw
        .crosscut(&one_by_two(), Orientation::default(), inches(12))
        .cut_apart();
    // Against the stop, the 12" piece is the stock's far end.
    assert_eq!(
        first.piece.board.origin().at_on_stock(),
        [inches(36), Length::ZERO, Length::ZERO]
    );
    let rest = first.offcut.unwrap().board;
    assert_eq!(rest.origin().at_on_stock(), [Length::ZERO; 3]);
    let second = saw
        .crosscut(&rest, Orientation::default(), inches(12))
        .cut_apart();
    let piece = second.piece.board;
    // The next 12" is a kerf further along, both cuts in order.
    assert_eq!(
        piece.origin().at_on_stock()[0],
        inches(36) - fraction(1, 8) - inches(12)
    );
    let operations = piece.origin().operations();
    assert_eq!(operations.len(), 2);
    assert!(operations[0].number < operations[1].number);
    assert!(matches!(
        operations[1].tool,
        Tool::MiterSaw { length, lie: Lie::Board(_), .. } if length == inches(12)
    ));
    assert_eq!(piece.origin().replay(), piece);
    assert!(piece.origin().stock().same(rest.origin().stock()));
}

#[test]
fn the_two_sides_of_a_cut_share_it() {
    let cut = table_saw(fraction(3, 4), inches(1))
        .rip(&one_by_two(), Orientation::default())
        .cut_apart();
    let (piece, offcut) = (&cut.piece.board, &cut.offcut.as_ref().unwrap().board);
    assert!(Arc::ptr_eq(&last(piece), &last(offcut)));
    let (
        Source::Sawn { from, part, .. },
        Source::Sawn {
            from: other,
            part: other_part,
            ..
        },
    ) = (piece.origin().source(), offcut.origin().source())
    else {
        panic!("both sawn");
    };
    assert!(from.same(other));
    assert_ne!(part, other_part);
    assert!(matches!(
        last(piece).tool,
        Tool::TableSaw { saw, .. } if saw.fence == fraction(3, 4)
    ));
    for board in [piece, offcut] {
        assert_eq!(&board.origin().replay(), board);
    }
}

#[test]
fn a_slot_that_doesnt_go_through_is_recorded_on_the_whole_board() {
    let Ok(Pass::Kerfed(grooved)) =
        table_saw(fraction(1, 2), fraction(1, 4)).rip(&one_by_two(), Orientation::default())
    else {
        panic!("expected a groove");
    };
    assert!(matches!(
        grooved.origin().source(),
        Source::Sawn {
            part: Part::Whole,
            ..
        }
    ));
    assert_eq!(grooved.origin().replay(), grooved);
    // Cut again, the slot's pass and the next both stay in the record.
    let cut = mitered(0)
        .crosscut(&grooved, Orientation::default(), inches(10))
        .cut_apart();
    assert_eq!(cut.piece.board.origin().operations().len(), 2);
    assert_eq!(cut.piece.board.origin().replay(), cut.piece.board);
}

#[test]
fn a_resaw_through_a_board_on_edge_replays() {
    let board = Board::new(inches(24), inches(3), fraction(3, 2));
    let cut = table_saw(fraction(5, 8), inches(3) + fraction(1, 4))
        .rip(&board, Orientation::default().tipped())
        .cut_apart();
    for piece in std::iter::once(&cut.piece).chain(&cut.offcut) {
        assert_eq!(piece.board.origin().replay(), piece.board);
        assert!(matches!(
            last(&piece.board).tool,
            Tool::TableSaw { lie: Lie::Board(lie), .. } if lie.on_edge()
        ));
    }
}

/// Two 1x2s, 24" long, glued edge to edge.
fn panel() -> Assembly {
    let board = Board::new(inches(24), fraction(3, 2), fraction(3, 4));
    let edge = |far: bool| {
        surfaces(&board)
            .into_iter()
            .find(|surface| {
                matches!(surface, Surface::Side(edge) if edge.out.l == 0 && (edge.out.w > 0) == far)
            })
            .unwrap()
    };
    let mut panel = Assembly::of(board.clone());
    let other = Assembly::of(board.clone());
    let mating = (0..4)
        .map(|turn| Mating {
            turn,
            slide: [Length::ZERO; 2],
        })
        .find(|&mating| {
            let motion = panel.mating((0, edge(true)), &other, (0, edge(false)), mating);
            motion.unwrap().rotation.apply([0.0, 0.0, 1.0])[2] > 0.999
        })
        .unwrap();
    assert!(panel.attach(
        (0, edge(true)),
        &other,
        (0, edge(false)),
        mating,
        Fastener::Glue
    ));
    panel
}

#[test]
fn a_cut_through_a_glue_up_is_one_pass_through_every_board() {
    let panel = panel();
    let glued = panel.joints()[0].number;
    let woodsim::GlueUpPass::Cut(cut) = mitered(0)
        .crosscut_glue_up(&panel, Rotation::NONE, inches(10))
        .unwrap()
    else {
        panic!("expected the panel cut apart");
    };
    let members = cut.piece.members();
    assert_eq!(members.len(), 2);
    let pass = last(&members[0].board);
    assert!(Arc::ptr_eq(&pass, &last(&members[1].board)));
    assert!(matches!(
        pass.tool,
        Tool::MiterSaw {
            lie: Lie::GlueUp(_),
            ..
        }
    ));
    // Glued before it was cut, and the joint keeps saying so.
    assert!(glued < pass.number);
    assert_eq!(cut.piece.joints()[0].number, glued);
    for member in members {
        assert_eq!(member.board.origin().replay(), member.board);
    }
}

#[test]
fn joints_are_numbered_in_the_order_they_go_together() {
    let piece = Board::new(inches(10), fraction(3, 2), fraction(3, 4));
    let end = |far: bool| {
        surfaces(&piece)
            .into_iter()
            .find(|surface| {
                matches!(surface, Surface::Side(edge) if edge.out.w == 0 && (edge.out.l > 0) == far)
            })
            .unwrap()
    };
    let mut row = Assembly::of(piece.clone());
    for last in 0..2 {
        let other = Assembly::of(piece.clone());
        assert!(row.attach(
            (last, end(true)),
            &other,
            (0, end(false)),
            Mating::default(),
            Fastener::Glue
        ));
    }
    let numbers: Vec<u64> = row.joints().iter().map(|joint| joint.number).collect();
    assert_eq!(numbers.len(), 2);
    assert!(numbers[0] < numbers[1]);
    // A glue-up attached whole keeps its joints' numbers.
    let mut bigger = Assembly::of(piece.clone());
    assert!(bigger.attach(
        (0, end(true)),
        &row,
        (0, end(false)),
        Mating::default(),
        Fastener::Glue
    ));
    assert_eq!(bigger.joints()[0].number, numbers[0]);
    assert!(bigger.joints().last().unwrap().number > numbers[1]);
}

/// A saw pass, in 64ths of an inch: a rip against the fence with the blade at some height, or
/// a chop on the miter saw at some angle, through or stopping above the table.
#[derive(Clone, Debug)]
enum Pass64 {
    Rip { fence: i64, height: i64 },
    Chop { length: i64, miter: i32, scar: bool },
}

fn pass() -> impl Strategy<Value = Pass64> {
    prop_oneof![
        (1..200i64, 8..100i64).prop_map(|(fence, height)| Pass64::Rip { fence, height }),
        (16..1500i64, -4500..=4500i32, any::<bool>()).prop_map(|(length, miter, scar)| {
            Pass64::Chop {
                length,
                miter,
                scar,
            }
        }),
    ]
}

/// Every board `pass` leaves `board` in.
fn apply(pass: &Pass64, board: &Board) -> Vec<Board> {
    let sixty_fourths = |n: i64| Length::from_inch_fraction(n, 64);
    let result = match *pass {
        Pass64::Rip { fence, height } => table_saw(sixty_fourths(fence), sixty_fourths(height))
            .rip(board, Orientation::default()),
        Pass64::Chop {
            length,
            miter,
            scar,
        } => {
            let saw = mitered(miter);
            let bottom = if scar { fraction(1, 4) } else { saw.stroke.1 };
            saw.chop(board, Orientation::default(), sixty_fourths(length), bottom)
        }
    };
    match result {
        Ok(Pass::Cut(cut)) => std::iter::once(cut.piece)
            .chain(cut.offcut)
            .chain(cut.loose)
            .map(|piece| piece.board)
            .collect(),
        Ok(Pass::Kerfed(board)) => vec![board],
        _ => Vec::new(),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn any_cuts_leave_boards_that_replay_exactly(
        width in 32..800i64,
        passes in prop::collection::vec((pass(), any::<Index>()), 1..6),
    ) {
        let stock = Board::new(inches(48), Length::from_inch_fraction(width, 64), fraction(3, 4));
        let mut boards = vec![stock.clone()];
        for (pass, which) in &passes {
            let board = boards[which.index(boards.len())].clone();
            boards.extend(apply(pass, &board));
        }
        for board in &boards {
            prop_assert_eq!(&board.origin().replay(), board);
            prop_assert!(board.origin().stock().same(stock.origin()));
            let numbers: Vec<u64> = board.origin().operations().iter().map(|op| op.number).collect();
            prop_assert!(numbers.windows(2).all(|pair| pair[0] < pair[1]), "{:?}", numbers);
            // It sits within the stock it came from.
            let [l, w, t] = board.origin().at_on_stock();
            prop_assert!(l + board.length() <= stock.length());
            prop_assert!(w + board.width() <= stock.width());
            prop_assert!(t + board.thickness() <= stock.thickness());
        }
    }
}
