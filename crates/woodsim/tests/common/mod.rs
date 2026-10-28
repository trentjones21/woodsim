//! Helpers shared by the integration tests.

use woodsim::{Cut, CutError, Pass};

/// Unwraps a pass that should have cut the board apart.
pub trait CutApart {
    fn cut_apart(self) -> Cut;
}

impl CutApart for Result<Pass, CutError> {
    fn cut_apart(self) -> Cut {
        match self {
            Ok(Pass::Cut(cut)) => cut,
            other => panic!("expected the board to be cut apart, got {other:?}"),
        }
    }
}
