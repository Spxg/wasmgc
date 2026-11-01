# Examples

## Shared palette

[Open the browser example](examples/palette/).

Three GC color objects are shared by 6,144 cells. Editing a color updates every
cell that references the same object. The browser and terminal versions use the
same [Rust implementation](https://github.com/Spxg/wasmgc/tree/master/examples/palette).

Run the terminal version from a repository checkout:

```sh
node examples/palette/run.mjs
```

## Breakout

A terminal game with a ball, paddle, and bricks stored in GC objects and arrays.
Move the paddle with the arrow keys or A/D.

```sh
node examples/breakout/run.mjs
```

See the [source](https://github.com/Spxg/wasmgc/tree/master/examples/breakout).
Both terminal commands build and postprocess the Wasm module automatically.
