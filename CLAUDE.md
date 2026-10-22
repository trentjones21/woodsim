# Woodshop Simulator

A woodworking sim game: start in a garage with a handsaw, earn money, buy tools, move to bigger shops. Cuts should be accurate enough to plan a real project.

## Pinned versions

Don't upgrade any of these without asking first.

- Rust 1.98.1, edition 2024
- Bevy 0.20.x (`bevy = "0.20.0"` in the workspace `Cargo.toml`)
- proptest 1.12.x

`Cargo.lock` is the source of truth for exact versions. Keep it committed.

## Layout

- `crates/woodsim`: the woodworking simulation. Boards, `Length`, saws, cuts. **No Bevy dependency, ever.** Every rule about wood lives here and is tested here.
- `crates/game`: the Bevy app. It draws boards and turns input into `woodsim` calls. It never changes board dimensions itself.
  - Each station is a pair of modules: its state and where to draw the board (plain Rust, no systems, unit tested), and its Bevy side (spawning, input, drawing).
    - Table saw: `feed.rs` (`TableSawStation`) and `table_saw.rs`.
    - Miter saw: `chop.rs` (`MiterSawStation`, including the parts pile and cut list) and `miter_saw.rs`.
  - `shop.rs`: what the stations share: layout helpers (`Block`, `inches`, `block_transform`), the shared cube mesh and wood materials (`ShopAssets`), the camera, the readout, and `ActiveStation`. Each station's coordinates start at its own `ORIGIN`; keyboard controls only run at the active station.
  - No wood disappears: anything a carried piece displaces, at either saw, goes on the parts pile (which lives in `MiterSawStation`). The one exception is R at the table saw, which throws the leftovers in the scrap bin.
  - Put game logic that doesn't need a window into plain structs like the station states so `cargo test -p game` can cover it.

## Commands

- `cargo test -p woodsim`: fast, run constantly. No Bevy compile needed.
- `cargo run -p game`: run the game.
- `cargo run -p game --features dev`: same, with Bevy dynamic linking for faster rebuilds.
- `WOODSHOP_SCREENSHOT=screenshots/shop.png cargo run -p game`: render a few frames, save a screenshot, quit. Use this to check visual changes yourself, then look at the image.
  - Now and then a screenshot comes back all black. Retake it before assuming a rendering bug; so far every retake has been fine.
  - To check an interaction, temporarily add a `PreUpdate` system that calls `ButtonInput::<KeyCode>::press`, ordered `.after(bevy::input::InputSystems)`. Without that ordering, Bevy clears `just_pressed` before the game sees it. Remove the system afterward.
- `cargo clippy --workspace --all-targets` and `cargo fmt --all` before finishing.

## Writing Bevy code

Bevy breaks its API every release, and much of what you remember is from older versions. Before using any Bevy API you aren't certain about, check the 0.20 source:

- Examples: `~/.cargo/registry/src/*/bevy-0.20.0/examples/` (e.g. `3d/3d_scene.rs`, `window/screenshot.rs`)
- Sub-crate source: `~/.cargo/registry/src/*/bevy_<name>-0.20.0/src/`

Examples of things that differ from older Bevy:

- Events are now "messages" (`MessageWriter`, `commands.write_message`).
- Picking events are their own types (`PointerDrag`, `PointerDragStart`, `PointerOver`), not `Pointer<Drag>`. Observers take `On<E>`, and `MeshPickingPlugin` isn't part of `DefaultPlugins`.
- Text size is `TextFont { font_size: FontSize::Px(18.0), .. }`.
- 0.20 has a new scene macro (`bsn!` / `bsn_list!`). This project uses plain `commands.spawn((...))`; don't mix in BSN without asking.

## Simulation rules

- `Length` is an exact integer count of nanometers (1" = 25,400,000 nm), chosen so 1/64", 0.001", and 0.01 mm are all exact. Never store a length the simulation depends on as a float. Convert with `as_meters_f32()` only when handing values to Bevy (1 Bevy unit = 1 meter).
- A board's length runs along the grain. Ripping (table saw) cuts the width; crosscutting (miter saw) cuts the length.
- Every cut loses up to one kerf to sawdust. `Cut::piece` is the measured side (against the fence, or the length cut to).
- Wood is conserved: piece + offcut + waste always equals the original dimension. `tests/conservation.rs` checks this with proptest; any new operation needs the same kind of test.
