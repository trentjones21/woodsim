//! No cut ever creates or loses wood: the pieces plus the sawdust always add up to the
//! original board, and only the dimension being cut changes. A pass that doesn't cut the board
//! apart leaves it the same size, with one more slot in it.

mod common;

use common::CutApart;

use proptest::prelude::*;
use woodsim::{Blade, Board, CutError, Length, MiterSaw, Orientation, Pass, Piece, TableSaw};

/// Ten feet, in nanometers.
const MAX_NM: i64 = 120 * 25_400_000;
/// One inch, in nanometers.
const INCH_NM: i64 = 25_400_000;

fn blade(kerf: i64) -> Blade {
    Blade {
        diameter: Length::from_inches(10),
        kerf: Length::from_nm(kerf),
    }
}

proptest! {
    #[test]
    fn rip_conserves_wood(width in 1..MAX_NM, fence in 1..MAX_NM, kerf in 1..INCH_NM) {
        let board = Board::new(Length::from_inches(96), Length::from_nm(width), Length::from_inch_fraction(3, 4));
        let saw = TableSaw { blade: blade(kerf), fence: Length::from_nm(fence), height: Length::from_inches(1), miter_gauge: None };

        match saw.rip(&board, Orientation::default()) {
            Ok(Pass::Cut(cut)) => {
                let offcut = cut.offcut.as_ref().map_or(Length::ZERO, |offcut| offcut.board.width());
                prop_assert_eq!(cut.piece.board.width() + offcut + cut.waste, board.width());
                prop_assert_eq!(cut.piece.board.width(), saw.fence);
                prop_assert!(cut.waste <= saw.blade.kerf);
                for piece in std::iter::once(&cut.piece).chain(&cut.offcut).map(|piece| &piece.board) {
                    prop_assert_eq!(piece, &Board::new(board.length(), piece.width(), board.thickness()));
                }
            }
            Ok(pass) => prop_assert!(false, "a blade higher than the board left {:?}", pass),
            Err(error) => {
                prop_assert!(saw.fence >= board.width());
                prop_assert_eq!(error, CutError::MissesBoard { setting: saw.fence, available: board.width() });
            }
        }
    }

    #[test]
    fn miter_gauge_crosscut_conserves_wood(length in INCH_NM..MAX_NM, stop in 1..MAX_NM, kerf in 1..INCH_NM) {
        let board = Board::new(Length::from_nm(length), Length::from_inches(6), Length::from_inch_fraction(3, 4));
        let saw = TableSaw {
            blade: blade(kerf),
            fence: Length::from_inches(6),
            height: Length::from_inches(1),
            miter_gauge: Some(Length::from_nm(stop)),
        };

        match saw.rip(&board, Orientation::default().turned_left()) {
            Ok(Pass::Cut(cut)) => {
                let offcut = cut.offcut.as_ref().map_or(Length::ZERO, |offcut| offcut.board.length());
                prop_assert_eq!(cut.piece.board.length() + offcut + cut.waste, board.length());
                prop_assert_eq!(cut.piece.board.length(), Length::from_nm(stop));
                prop_assert_eq!(cut.piece.l, Length::ZERO);
                prop_assert!(cut.waste <= saw.blade.kerf);
            }
            Ok(pass) => prop_assert!(false, "a blade higher than the board left {:?}", pass),
            Err(error) => {
                prop_assert_eq!(error, CutError::MissesBoard { setting: Length::from_nm(stop), available: board.length() });
            }
        }
    }

    #[test]
    fn crosscut_conserves_wood(length in 1..MAX_NM, at in 1..MAX_NM, kerf in 1..INCH_NM) {
        let board = Board::new(Length::from_nm(length), Length::from_inches(6), Length::from_inch_fraction(3, 4));
        let saw = MiterSaw { blade: blade(kerf), ..MiterSaw::twelve_inch() };
        let at = Length::from_nm(at);

        match saw.crosscut(&board, Orientation::default(), at) {
            Ok(Pass::Cut(cut)) => {
                let offcut = cut.offcut.as_ref().map_or(Length::ZERO, |offcut| offcut.board.length());
                prop_assert_eq!(cut.piece.board.length() + offcut + cut.waste, board.length());
                prop_assert_eq!(cut.piece.board.length(), at);
                prop_assert!(cut.waste <= saw.blade.kerf);
                for piece in std::iter::once(&cut.piece).chain(&cut.offcut).map(|piece| &piece.board) {
                    prop_assert_eq!(piece, &Board::new(piece.length(), board.width(), board.thickness()));
                }
            }
            Ok(pass) => prop_assert!(false, "all the way down, the blade left {:?}", pass),
            Err(error) => {
                prop_assert!(at >= board.length());
                prop_assert_eq!(error, CutError::MissesBoard { setting: at, available: board.length() });
            }
        }
    }

    #[test]
    fn every_pass_keeps_the_board_whole_or_conserves_it(
        width in INCH_NM..12 * INCH_NM,
        fence in 1..12 * INCH_NM,
        height in -INCH_NM..3 * INCH_NM,
        start in -8 * INCH_NM..30 * INCH_NM,
        travel in 0..40 * INCH_NM,
    ) {
        let board = Board::new(Length::from_inches(24), Length::from_nm(width), Length::from_inch_fraction(3, 4));
        let saw = TableSaw { blade: blade(INCH_NM / 8), fence: Length::from_nm(fence), height: Length::from_nm(height), miter_gauge: None };
        let start = Length::from_nm(start);

        match saw.feed(&board, Orientation::default(), start, start + Length::from_nm(travel)) {
            Ok(Pass::Missed) => {}
            Ok(Pass::Kerfed(kerfed)) => {
                prop_assert_eq!(
                    (kerfed.length(), kerfed.width(), kerfed.thickness()),
                    (board.length(), board.width(), board.thickness())
                );
                prop_assert_eq!(kerfed.kerfs().len(), 1);
                let kerf = kerfed.kerfs()[0];
                prop_assert_eq!(kerf.far, board.width() - saw.fence);
                prop_assert_eq!(kerf.far - kerf.near, saw.blade.kerf);
            }
            Ok(Pass::Cut(cut)) => {
                prop_assert!(saw.height >= board.thickness(), "a blade lower than the board cut it apart");
                let offcut = cut.offcut.as_ref().map_or(Length::ZERO, |offcut| offcut.board.width());
                prop_assert_eq!(cut.piece.board.width() + offcut + cut.waste, board.width());
                prop_assert_eq!(cut.piece.board.width(), saw.fence);
            }
            Err(error) => {
                prop_assert_eq!(error, CutError::MissesBoard { setting: saw.fence, available: board.width() });
            }
        }
    }

    #[test]
    fn pushing_further_never_puts_a_board_back_together(
        height in 0..3 * INCH_NM,
        end in -8 * INCH_NM..40 * INCH_NM,
        further in 0..10 * INCH_NM,
    ) {
        let board = Board::new(Length::from_inches(24), Length::from_inch_fraction(29, 4), Length::from_inch_fraction(3, 4));
        let saw = TableSaw { blade: blade(INCH_NM / 8), fence: Length::from_inches(6), height: Length::from_nm(height), miter_gauge: None };
        let start = -Length::from_inches(8);
        let end = Length::from_nm(end).max(start);

        if let Ok(Pass::Cut(_)) = saw.feed(&board, Orientation::default(), start, end) {
            let further = end + Length::from_nm(further);
            prop_assert!(matches!(saw.feed(&board, Orientation::default(), start, further), Ok(Pass::Cut(_))));
        }
    }

    #[test]
    fn crosscutting_a_slotted_board_keeps_every_piece_on_it(
        fence in INCH_NM..7 * INCH_NM,
        height in INCH_NM / 2..2 * INCH_NM,
        end in -2 * INCH_NM..30 * INCH_NM,
        at in INCH_NM..23 * INCH_NM,
    ) {
        let board = Board::new(Length::from_inches(24), Length::from_inch_fraction(29, 4), Length::from_inch_fraction(3, 4));
        let saw = TableSaw { blade: blade(INCH_NM / 8), fence: Length::from_nm(fence), height: Length::from_nm(height), miter_gauge: None };
        let slotted = match saw.feed(&board, Orientation::default(), -Length::from_inches(6), Length::from_nm(end)) {
            Ok(Pass::Kerfed(slotted)) => slotted,
            _ => return Ok(()),
        };
        let at = Length::from_nm(at);
        let cut = MiterSaw { blade: blade(INCH_NM / 8), ..MiterSaw::twelve_inch() }.crosscut(&slotted, Orientation::default(), at).cut_apart();
        let pieces: Vec<&Piece> = std::iter::once(&cut.piece).chain(&cut.offcut).chain(&cut.loose).collect();

        // Every piece lies on the board, and no two share any wood.
        for piece in &pieces {
            prop_assert!(piece.l >= Length::ZERO && piece.w >= Length::ZERO);
            prop_assert!(piece.l + piece.board.length() <= board.length());
            prop_assert!(piece.w + piece.board.width() <= board.width());
        }
        for (i, a) in pieces.iter().enumerate() {
            for b in &pieces[i + 1..] {
                let apart_along = a.l + a.board.length() <= b.l || b.l + b.board.length() <= a.l;
                let apart_across = a.w + a.board.width() <= b.w || b.w + b.board.width() <= a.w;
                prop_assert!(apart_along || apart_across, "{:?} overlaps {:?}", a, b);
            }
        }
        // Each side of the blade keeps the board's whole width, less the slot where it goes
        // through that whole side.
        let kerf = slotted.kerfs()[0];
        let slot = kerf.far.min(board.width()) - kerf.near.max(Length::ZERO);
        let stop_side = board.length() - at;
        for against_stop in [true, false] {
            let widths: Length = pieces.iter().filter(|piece| (piece.l >= stop_side) == against_stop).map(|piece| piece.board.width()).sum();
            prop_assert!(
                widths == board.width() || widths == board.width() - slot || (!against_stop && cut.offcut.is_none()),
                "one side's pieces add up to {} of {}", widths, board.width()
            );
        }
    }
}
