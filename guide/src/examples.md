# Examples

## Persistent tree

[Open the browser example](examples/tree/).

Edit a tree and compare the previous and current versions. Rust copies only the
path to the edited node; the rest of the tree is shared between versions.
Matching object labels and highlighted nodes show that sharing.

Only these two versions are retained. Each edit releases the oldest version;
the page uses weak references to show when nodes no longer referenced by either
version are actually collected by the browser.

See the [source and local run commands](https://github.com/Spxg/wasmgc/tree/master/examples/tree).

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
