# Spike 3 — Network map

*2026-10-08 · Leptos 0.8, WebKitGTK 2.52, in the codespace (Xvfb, no GPU, 2 cores, debug wasm)*

## Answer

**Can the network map (DESIGN §7) be drawn in Rust, with Leptos and SVG, rather than with a JavaScript graph library?** Yes.
- **Dragging a node** costs almost nothing in Leptos. Each node's position is a pair of signals, so a drag updates that node's `transform` and the ends of its lines, and nothing else.
- **Panning and zooming** cost what WebKit takes to repaint the map. A JavaScript library drawing SVG would pay the same, so it wouldn't be faster.
- **Moving the map as a layer** helps. Putting the pan and zoom in a CSS `transform` on the `<svg>` (with `will-change: transform`), instead of an SVG `transform` on a `<g>` inside it, made panning and zooming faster, most of all at 500 nodes. The map will do this.

## Timings

Each test moves something on every frame for 180 frames and measures the gaps between frames. The map is drawn like Look 4: a tile, an icon and two lines of text per node, with every fourth line labelled.

| Nodes, lines | Pan and zoom by | Drag (median / p95) | Pan | Zoom |
|---|---|---|---|---|
| 200, 301 | SVG transform | 30 / 52 ms | 36 / 55 ms | 39 / 62 ms |
| 200, 301 | CSS layer | 28 / 48 ms | 23 / 36 ms | 30 / 43 ms |
| 500, 752 | SVG transform | 35 / 50 ms | 51 / 79 ms | 53 / 80 ms |
| 500, 752 | CSS layer | 35 / 57 ms | 25 / 37 ms | 32 / 50 ms |

These are a worst case: software rendering on two cores, with a debug build. An earlier run of the 200-node drag gave 16 ms (60 fps), so the numbers vary by about a factor of two from run to run. Even so, 500 nodes with the layer stays around 30 fps.

## Still to check

- **On the user's machine,** with a GPU. On the `feat/network` branch, open the app with `cargo tauri dev --config '{"build":{"devUrl":"http://localhost:1420/#network-spike"}}'`. The panel's buttons time a drag, a pan and a zoom, and switch between 200 and 500 nodes and the two ways of moving the map.
- **Text when zoomed as a layer:** during a zoom, WebKit may scale the layer's pixels and redraw it sharp only afterwards. If labels look blurry while zooming, the map can switch to an SVG transform once the zoom stops.

## Code

`crates/desktop/src/network_spike.rs` is mounted instead of the app when the URL's hash is `#network-spike`, `#network-spike-500` or either one with `-layer`. It's there only to be tried, and comes out once the real map is built.
