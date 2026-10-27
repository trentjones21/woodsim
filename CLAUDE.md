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
  - `shop.rs`: the shop around the stations: layout helpers (`Block`, `inches`, `rounded_block`), shared textures and materials (`ShopAssets`), the floor, the camera, and the readout (including the FPS counter).
    - `ShopView` is what the camera shows: the `Overview` of the whole shop (the starting view; pan, zoom, rotate) or one station up close. A station's keyboard controls only run while it's up close; mouse controls work from either.
    - Each station's coordinates start at its own `ORIGIN`, and its `BOUNDS` outline it in the overview. Pass its body's entities to `station_part` so clicking them zooms in.
  - `scenery.rs`: everything that's just for looks: walls that cut away on the camera's side, the sun and ambient light, the yard, the lumber rack, pegboard, and props.
  - `textures.rs`: every texture is generated in code at startup (wood, floor tiles, pegboard, grass). There are no asset files.
  - `meshes.rs`: every box is a `rounded_box`, whose soft edges give the toy look; its UVs are in meters. Give anything wooden a `board_mesh` and `ShopAssets::wood` (or `maple`) so the grain runs along the board. `BoardBlocks::grain_origin` keeps the grain continuous as a board is cut.
  - No wood disappears: anything a carried piece displaces, at either saw, goes on the parts pile (which lives in `MiterSawStation`). The one exception is R at the table saw, which throws the leftovers in the scrap bin.
  - Put game logic that doesn't need a window into plain structs like the station states so `cargo test -p game` can cover it.

## Look and performance

The look is bright and toylike, like Two Point Museum, not realistic, and the game must run well on modest hardware. One shadowed sun, a cool ambient fill, plain painted materials, MSAA, and tonemapping; no post-processing beyond that. Nothing is metallic: there's no environment map, so metal would render black.

Run `WOODSHOP_BENCH` before and after anything that adds rendering cost (new lights with shadows, post-processing, transparency, big meshes). For reference, on an M5 Pro with nothing else running the overview takes about 3.5 ms a frame at 4K (about 5.5 ms with the desktop sharing the GPU); the earlier realistic look (volumetric light, depth of field, SSAO, TAA) took 34 ms. Compare numbers measured under the same conditions.

## Commands

- `cargo test -p woodsim`: fast, run constantly. No Bevy compile needed.
- `cargo run -p game`: run the game.
- `cargo run -p game --features dev`: same, with Bevy dynamic linking for faster rebuilds.
- `WOODSHOP_SCREENSHOT=screenshots/shop.png cargo run -p game`: render a few frames offscreen, save a screenshot, quit. Use this to check visual changes yourself, then look at the image. It opens no window, so it works while the screen is locked.
  - To capture a station up close, temporarily `insert_resource(ShopView::TableSaw)` (or `MiterSaw`) before the plugins are added.
  - To check an interaction, temporarily add a `PreUpdate` system that calls `ButtonInput::<KeyCode>::press`, ordered `.after(bevy::input::InputSystems)`. Without that ordering, Bevy clears `just_pressed` before the game sees it. To capture a later moment, temporarily change the frame numbers in `screenshot_and_exit`. Remove both afterward.
- `WOODSHOP_BENCH=1 cargo run -p game --release`: render the overview offscreen at 4K as fast as possible, print frame times, quit. Offscreen, frames aren't held to the display's refresh rate, so this measures what a frame costs.
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
- A spawned tuple can hold at most 15 components; add the rest with `.insert((...))`.
- Shadows respect `RenderLayers` but lighting doesn't: a light shades every mesh, whatever its layer.
- `Screenshot::image` captures of an image render target came out black here; `main.rs` reads the image back with `ReadbackOnce::texture` instead. A camera that renders to an image also needs `IsDefaultUiCamera`, or the UI isn't drawn.
- Runtime-made images have no mipmaps unless you build them (`textures::with_mipmaps`); without them, textures shimmer at a distance.

## Simulation rules

- `Length` is an exact integer count of nanometers (1" = 25,400,000 nm), chosen so 1/64", 0.001", and 0.01 mm are all exact. Never store a length the simulation depends on as a float. Convert with `as_meters_f32()` only when handing values to Bevy (1 Bevy unit = 1 meter).
- A board's length runs along the grain. Ripping (table saw) cuts the width; crosscutting (miter saw) cuts the length.
- Every cut loses up to one kerf to sawdust. `Cut::piece` is the measured side (against the fence, or the length cut to).
- Wood is conserved: piece + offcut + waste always equals the original dimension. `tests/conservation.rs` checks this with proptest; any new operation needs the same kind of test.
