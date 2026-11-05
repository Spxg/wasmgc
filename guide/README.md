# Guide

The guide uses [mdBook](https://rust-lang.github.io/mdBook/).

Preview the guide from the repository root:

```sh
mdbook serve guide --open
```

`mdbook build guide` writes the book to `target/guide/`.

Build the full site, including browser examples and benchmarks. This also requires
Node.js, the `wasm32-unknown-unknown` Rust target, Binaryen's `wasm-opt`, and
`wasm-bindgen-cli` 0.2.130:

```sh
mdbook build guide --dest-dir target/site
node examples/palette/web/build.mjs
node examples/tree/web/build.mjs
npm ci --prefix benchmarks
npm run build --prefix benchmarks
```

The result is in `target/site/`: the book at the root, browser examples under
`examples/<name>/`, and benchmarks under `benchmarks/`. Build the book first because
mdBook clears its output directory.
