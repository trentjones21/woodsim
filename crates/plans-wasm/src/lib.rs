//! Plans drawn up in WebAssembly, for the marketplace's Worker (`marketplace/`): a recipe in,
//! its plan page out. The module talks through a few plain functions on its own memory rather
//! than generated bindings, so the Worker needs nothing but the `.wasm` file: it writes a
//! recipe's text into memory from `alloc`, calls `draw_up_at`, and reads what came out with
//! `output` and `output_len`. The Worker makes a fresh instance for each recipe, so nothing
//! carries over from one to the next, and a recipe that panics only traps its own instance.

use std::sync::Mutex;

use plans::Plan;
use woodsim::{Assembly, Recipe, Wood};

/// Plans drawn up from a recipe.
#[derive(Debug)]
pub struct Drawn {
    /// The plan page, as the game would save it.
    pub page: String,
    /// The recipe written again as this woodsim writes it: its `version` line first, and no
    /// `saw` line, since how a saw's set up now isn't part of the plans.
    pub recipe: String,
}

/// Draws up plans for the wood `recipe` writes out, a board or a glue-up, or says why it
/// doesn't read.
pub fn draw_up(recipe: &str) -> Result<Drawn, String> {
    let wood = Recipe::read(recipe)?.wood;
    let written = Recipe {
        wood: wood.clone(),
        saw: None,
    }
    .write();
    let assembly = match wood {
        Wood::Board(board) => Assembly::of(board),
        Wood::GlueUp(glue_up) => glue_up,
    };
    Ok(Drawn {
        page: Plan::of(assembly).page(),
        recipe: written,
    })
}

/// What the last `draw_up_at` left to read: the page and the recipe, or why it didn't read.
static OUTPUT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Room for `len` bytes, for the caller to write a recipe's text into. It's never given back:
/// the instance is thrown away after one recipe.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    Box::leak(vec![0u8; len].into_boxed_slice()).as_mut_ptr()
}

/// Draws up plans for the recipe in the `len` bytes at `at`. It returns 0 if they're drawn up,
/// with `output(0)` the page and `output(1)` the recipe written again, or 1 if the recipe
/// doesn't read, with `output(0)` saying why.
///
/// # Safety
///
/// `at` must point to `len` bytes the module owns, as `alloc` gives.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn draw_up_at(at: *const u8, len: usize) -> u32 {
    // SAFETY: the caller wrote `len` bytes from `alloc`, which nothing else holds.
    let bytes = unsafe { std::slice::from_raw_parts(at, len) };
    let drawn = std::str::from_utf8(bytes)
        .map_err(|_| "it isn't text".to_string())
        .and_then(draw_up);
    let (status, output) = match drawn {
        Ok(drawn) => (0, vec![drawn.page, drawn.recipe]),
        Err(why) => (1, vec![why]),
    };
    *OUTPUT.lock().expect("one caller at a time") = output;
    status
}

/// Where the `which`th thing the last `draw_up_at` left starts, in the module's memory. It
/// stays there until the next `draw_up_at`.
#[unsafe(no_mangle)]
pub extern "C" fn output(which: usize) -> *const u8 {
    let output = OUTPUT.lock().expect("one caller at a time");
    output
        .get(which)
        .map_or(std::ptr::null(), |text| text.as_ptr())
}

/// How many bytes long the `which`th thing the last `draw_up_at` left is.
#[unsafe(no_mangle)]
pub extern "C" fn output_len(which: usize) -> usize {
    let output = OUTPUT.lock().expect("one caller at a time");
    output.get(which).map_or(0, String::len)
}

/// Which way of writing wood `output(1)` is in (`Recipe::VERSION`).
#[unsafe(no_mangle)]
pub extern "C" fn recipe_version() -> u32 {
    Recipe::VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three boards glued up, pasted from the game.
    const GLUE_UP: &str = include_str!("../../woodsim/tests/pasted/microns-bridge.txt");

    #[test]
    fn a_glue_up_draws_up_as_the_game_would() {
        let Wood::GlueUp(glue_up) = Recipe::read(GLUE_UP).unwrap().wood else {
            panic!("a glue-up");
        };
        let drawn = draw_up(GLUE_UP).unwrap();
        assert_eq!(drawn.page, Plan::of(glue_up.clone()).page());
        assert!(
            drawn
                .page
                .contains("<title>Plans for a glue-up of 3 parts</title>")
        );

        // The recipe comes back as this woodsim writes it, and reads back as the same wood.
        assert!(drawn.recipe.starts_with("version 1\n"), "{}", drawn.recipe);
        assert!(!drawn.recipe.contains("\nsaw "));
        assert_eq!(
            Recipe::read(&drawn.recipe).unwrap().wood,
            Wood::GlueUp(glue_up)
        );
    }

    #[test]
    fn a_board_draws_up_as_one_part() {
        let board = Recipe {
            wood: Wood::Board(woodsim::Board::new(
                woodsim::Length::from_inches(24),
                woodsim::Length::from_inch_fraction(7, 2),
                woodsim::Length::from_inch_fraction(3, 4),
            )),
            saw: None,
        };
        let drawn = draw_up(&board.write()).unwrap();
        assert!(drawn.page.contains("<title>Plans for one part</title>"));
        assert_eq!(drawn.recipe, board.write());
    }

    #[test]
    fn a_recipe_that_doesnt_read_says_why() {
        let why = draw_up("stock s0 1 2 3\nboard s9\n").unwrap_err();
        assert!(why.starts_with("line 2: no board `s9`"), "{why}");
    }

    /// Calls the module's functions as the Worker does, on text in its memory.
    fn through_memory(text: &[u8]) -> (u32, Vec<String>) {
        let at = alloc(text.len());
        // SAFETY: `alloc` gave `text.len()` bytes at `at`.
        let status = unsafe {
            std::ptr::copy_nonoverlapping(text.as_ptr(), at, text.len());
            draw_up_at(at, text.len())
        };
        let outputs = (0..2)
            .filter(|&which| !output(which).is_null())
            .map(|which| {
                // SAFETY: `output` and `output_len` give a string the module holds.
                let bytes = unsafe { std::slice::from_raw_parts(output(which), output_len(which)) };
                String::from_utf8(bytes.to_vec()).unwrap()
            })
            .collect();
        (status, outputs)
    }

    #[test]
    fn the_worker_reads_the_page_and_recipe_out_of_memory() {
        let (status, outputs) = through_memory(GLUE_UP.as_bytes());
        assert_eq!(status, 0);
        let drawn = draw_up(GLUE_UP).unwrap();
        assert_eq!(outputs, [drawn.page, drawn.recipe]);

        let (status, outputs) = through_memory(b"\xff\xfe");
        assert_eq!(
            (status, outputs.as_slice()),
            (1, ["it isn't text".to_string()].as_slice())
        );
        assert_eq!(recipe_version(), Recipe::VERSION);
    }
}
