# Shared palette

Three Rust `Color` objects are shared by 6,144 browser cells (96 × 64).
Editing one color changes all 2,048 cells that reference it. The terminal uses
a smaller 12 × 6 grid. The browser and terminal use the same
[Rust implementation](src/lib.rs).

Install the Rust target once:

```sh
rustup target add wasm32-unknown-unknown
```

Run the browser example from the repository root:

```sh
node examples/palette/web/serve.mjs
```

Open <http://localhost:4173> in a browser with WasmGC support. Select a tile or
palette entry, then use the RGB sliders or color picker. Outlined tiles hold
the same GC reference. Scroll to explore the grid and adjust the cell size to
zoom in or out. Arrow keys move between tiles; Enter selects their color.

The browser sources and builder live in [`web/`](web). Build just the example with:

```sh
node examples/palette/web/build.mjs
```

This writes `index.html`, `main.mjs`, `style.css`, and `palette.wasm` to
`target/site/examples/palette/`. The local server serves the same output.

The terminal example remains available:

```sh
node examples/palette/run.mjs
```

The build and run commands compile the Rust crate and postprocess its Wasm automatically.
