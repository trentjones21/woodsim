# Woodshop Simulator

A woodworking sim game: start in a garage with a handsaw, earn money, buy tools, move to bigger shops. Cuts should be accurate enough to plan a real project.

## Pinned versions

Don't upgrade any of these without asking first.

- Rust 1.98.1, edition 2024
- Bevy 0.20.x (`bevy = "0.20.0"` in the workspace `Cargo.toml`)
- proptest 1.12.x

`Cargo.lock` is the source of truth for exact versions. Keep it committed.

## Layout

- `crates/woodsim`: the woodworking simulation. Boards and the exact shape of their faces (`face.rs`), `Length` and `Angle`, kerfs, how a board lies on a saw (`Orientation`), saws, cuts. **No Bevy dependency, ever.** Every rule about wood lives here and is tested here.
- `crates/game`: the Bevy app. It draws boards and turns input into `woodsim` calls. It never changes board dimensions itself.
  - Each station is a pair of modules: its state and where to draw the board (plain Rust, no systems, unit tested), and its Bevy side (spawning, input, drawing).
    - Table saw: `feed.rs` (`TableSawStation`: how the board lies, which guide is in use, where the board is along the blade, and the pass under way) and `table_saw.rs`. G switches between the rip fence and the miter gauge; the fence slides out of the way while the gauge is in use.
    - Miter saw: `chop.rs` (`MiterSawStation`: the stop, the miter angle, the depth stop, how far the blade is down, the parts pile and cut list) and `miter_saw.rs`. `[` and `]` swing the turntable 1° (with Shift, to its next stop), and the arm and blade turn with it.
    - At both saws, Q/E turn a waiting board a quarter turn, F flips it over, and V stands it on an edge or lays it back down.
  - `shop.rs`: the shop around the stations: layout helpers (`Block`, `inches`, `rounded_block`), shared textures and materials (`ShopAssets`), the floor, the camera, and the readout (including the FPS counter). A station reports the boards on it as `Placed`: the `woodsim::Board`, where its corner sits and how it's turned, in the saw's own frame (across toward the fence, up, along as x, y, z). `Placed::lying` lays a board down as its `Orientation` says, `Placed::piece` puts a cut piece where it was on its board, and `place_parts` turns the saw's frame into the station's and draws them. `describe_board` puts a board, its mitered ends, and its slots into words, with a mitered board's length to its long points; it reads kerfs cut side by side the same way as one wider slot (a dado nibbled out a stroke at a time) and counts slots that measure the same. The UI font has no degree sign, so angles go through `degrees` ("45 deg"). A part's `Shape` remembers the board its mesh was built from, so meshes are only rebuilt when a board changes, not every frame the blade moves.
    - `ShopView` is what the camera shows: the `Overview` of the whole shop (the starting view; pan, zoom, rotate) or one station up close. A station's keyboard controls only run while it's up close; mouse controls work from either.
    - Each station's coordinates start at its own `ORIGIN`, and its `BOUNDS` outline it in the overview. Pass its body's entities to `station_part` so clicking them zooms in.
  - `scenery.rs`: everything that's just for looks: walls that cut away on the camera's side, the sun and ambient light, the yard, the lumber rack, pegboard, and props.
  - `textures.rs`: every texture is generated in code at startup (wood, floor tiles, pegboard, grass). There are no asset files.
  - `meshes.rs`: every box is a `rounded_box`, whose soft edges give the toy look; its UVs are in meters. `lumber_mesh` draws a simulated board with crisp sawn edges, whatever its outline (mitered ends, corners cut out), every slot at any angle, and resaw slots that leave the wood at a point in two spans or more; wooden furniture uses the rounded `board_mesh`. Use either with `ShopAssets::wood` (or `maple`) so the grain runs along the board. `Placed::grain` keeps the grain continuous as a board is cut.
  - No wood disappears: anything a carried piece displaces, and any strip a cut sets loose, at either saw, goes on the parts pile (which lives in `MiterSawStation`). The one exception is R at the table saw, which throws the leftovers in the scrap bin.
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
- Positions on a board are in its own coordinates: `l` along its length, `w` across its width, `t` up through its thickness. A board lies on a saw as its `Orientation` says: rolled some quarter turns about its length (flat, on an edge, flipped, on the other edge), then turned some quarter turns; `across`, `up`, and `along` say which of its axes (`Axis::Length`, `Width`, `Thickness`) lies each way. Tipped onto an edge, it stands on its `w = 0` edge with its `t = 0` face toward the fence; to resaw from the other edge with the same face to the fence, flip it and turn it end for end. Unrolled and unturned, the table saw holds the `w = width` edge against the fence and leads with `l = 0`, and the miter saw puts the `l = length` end against the stop and the `w = width` edge against the fence. Turned a quarter turn and pushed by the miter gauge, the table saw crosscuts: the board's near side rests against the gauge's stop, which measures the piece from the blade's other side.
- Each table saw guide warns when it can't hold the board straight, as a real shop would: the rip fence when the board rides it on its short side (wider across than it is long along the fence), and the miter gauge when the board is longer along the cut than it is wide. The game builds a board's mesh in its own axes (`w`, `t`, `l` along x, y, z) and each station turns it into place.
- A board is its face, through its whole thickness, plus the `Kerf`s cut into it: slots a round blade swept without cutting it apart. The face is convex `Region`s bounded by `Edge`s (`face.rs`): one rectangle until a cut leaves some other shape, like a mitered end or a corner cut out. `length` and `width` are the rectangle around it, so a mitered board's length is to its long points.
- Most slots are cut by a blade standing square to the board's face (`Across::Face`): one runs along the board (a stopped rip, a groove, a rabbet), across it (a dado), or slanted (a scar from a miter saw turned to an angle), and the blade came through the bottom face or the top. A blade standing parallel to the faces, through a board on edge, cuts a resaw slot square to the thickness (`Across::Thickness`), in through an edge. A kerf records exactly what the saw did (the `Direction`s across and along it, the slot's faces, the blade's radius and center height, how far the center traveled); shapes are worked out from that. Positions along a slanted direction are scaled by its length (`Direction::scale`) so they stay whole numbers as pieces move; `Board::slot` measures a slot as you would.
- Every board woodsim hands back is one piece. After any cut, `Board::pieces` takes wherever a slot goes all the way through out of the board's face and groups what's left by which regions share a stretch of edge: a slot the board's length splits it, a miter splits it on the slant, and a rip and a crosscut that meet cut out the corner between them. Earlier kerfs move into the pieces they touch, and each `Piece` says where its corner was on the board that was cut, through the thickness too. A cut through an earlier slot can set strips loose (`Cut::loose`).
- `Board::pieces` splits a board into layers where resaw slots sharing some depth together reach across its whole face: one pass with the blade higher than the board is wide, or two from opposite edges that meet. Their outlines on the face are just inside the blade's round ends, so a partial pass never splits off a sliver the blade didn't quite reach. A slot in the face that goes through a thinner layer then divides that layer.
- Slots in the face divide a board only where one goes through on its own; two from opposite faces whose depths only go through together don't yet. Standing a board on end isn't modeled yet, nor crosscutting a board on edge at the table saw or at a miter (`CutError::NotModeled`).
- The miter saw's table turns 50° either way (`MiterSaw::miter`; positive swings the blade's front toward the stop) about where the blade's stop-side face crosses the fence, so the piece against the stop comes out exactly the set length along the fence. The stop touches a board where it reaches furthest toward it: a mitered end's long point. `Board::miters` reads a board's slanted ends back.
- How wide a board the miter saw cuts through is how far its blade reaches at the bottom of its stroke (`MiterSaw::reach`): 7-1/2" out from the fence for the 12" saw, square, and less across the fence at an angle (`MiterSaw::widest`). A wider board, like one turned across the saw or a 6" board at 45°, it cuts as far as it reaches and leaves in one piece, and the station warns before the stroke.
- The miter saw's blade swings down on an arm from a pivot behind the fence (`MiterSaw::pivot`, `arm`, `stroke`), so its center moves on an arc. `MiterSaw::chop` takes how high the blade's lowest point stopped: below the table it cuts through (`crosscut` is a full stroke), and above it, by hand or at the depth stop, it scars the board from above. The saw doesn't slide, so a scar's bottom is curved like the blade and it reaches across only part of a wide board. A stroke leaves one kerf: the blade at the deepest point it reached, which covers what the arc swept on the way down.
- Tool settings and travel are exact, including the miter saw's depth stop and its angle (`Angle` counts hundredths of a degree). Lengths that come from the round blade (where a stopped slot ends) are exact to the nanometer and shown rounded to 1/32", as you'd measure them.
- A slanted direction is a pair of whole numbers within 1/16,000,000 of a radian of its angle, and every edge crosses the face at a whole number, so which side of a cut a point is on, and whether two pieces still hold together, are exact. Lengths along a slant land within a few nanometers of the ideal angle's. Keep the exact predicates in `face.rs` on `i128`; measurements for people can use `f64`.
- Every cut loses up to one kerf to sawdust. `Cut::piece` is the measured side (against the fence, or the length cut to).
- Wood is conserved: piece + offcut + waste always equals the original dimension, and a pass that doesn't cut through leaves the dimensions alone and adds exactly one kerf. `tests/conservation.rs` checks this with proptest, `tests/miters.rs` checks it along the fence for cuts at any angle, and `tests/on_edge.rs` through the thickness for resaws, with `Board::has_wood_in` sampling that no two pieces share wood; any new operation needs the same kind of test. Saws return a `Pass` (missed, kerfed, or cut apart); tests that expect a cut use `CutApart::cut_apart` from `tests/common`.
