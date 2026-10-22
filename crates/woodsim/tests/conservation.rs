//! No cut ever creates or loses wood: the pieces plus the sawdust always add up to the
//! original board, and only the dimension being cut changes.

use proptest::prelude::*;
use woodsim::{Blade, Board, CutError, Length, MiterSaw, TableSaw};

/// Ten feet, in nanometers.
const MAX_NM: i64 = 120 * 25_400_000;
/// One inch, in nanometers.
const INCH_NM: i64 = 25_400_000;

proptest! {
    #[test]
    fn rip_conserves_wood(width in 1..MAX_NM, fence in 1..MAX_NM, kerf in 1..INCH_NM) {
        let board = Board::new(Length::from_inches(96), Length::from_nm(width), Length::from_inch_fraction(3, 4));
        let saw = TableSaw { blade: Blade { kerf: Length::from_nm(kerf) }, fence: Length::from_nm(fence) };

        match saw.rip(&board) {
            Ok(cut) => {
                let offcut = cut.offcut.as_ref().map_or(Length::ZERO, Board::width);
                prop_assert_eq!(cut.piece.width() + offcut + cut.waste, board.width());
                prop_assert_eq!(cut.piece.width(), saw.fence);
                prop_assert!(cut.waste <= saw.blade.kerf);
                for piece in std::iter::once(&cut.piece).chain(&cut.offcut) {
                    prop_assert_eq!(piece, &Board::new(board.length(), piece.width(), board.thickness()));
                }
            }
            Err(error) => {
                prop_assert!(saw.fence >= board.width());
                prop_assert_eq!(error, CutError::MissesBoard { setting: saw.fence, available: board.width() });
            }
        }
    }

    #[test]
    fn crosscut_conserves_wood(length in 1..MAX_NM, at in 1..MAX_NM, kerf in 1..INCH_NM) {
        let board = Board::new(Length::from_nm(length), Length::from_inches(6), Length::from_inch_fraction(3, 4));
        let saw = MiterSaw { blade: Blade { kerf: Length::from_nm(kerf) } };
        let at = Length::from_nm(at);

        match saw.crosscut(&board, at) {
            Ok(cut) => {
                let offcut = cut.offcut.as_ref().map_or(Length::ZERO, Board::length);
                prop_assert_eq!(cut.piece.length() + offcut + cut.waste, board.length());
                prop_assert_eq!(cut.piece.length(), at);
                prop_assert!(cut.waste <= saw.blade.kerf);
                for piece in std::iter::once(&cut.piece).chain(&cut.offcut) {
                    prop_assert_eq!(piece, &Board::new(piece.length(), board.width(), board.thickness()));
                }
            }
            Err(error) => {
                prop_assert!(at >= board.length());
                prop_assert_eq!(error, CutError::MissesBoard { setting: at, available: board.length() });
            }
        }
    }
}
