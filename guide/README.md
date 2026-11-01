# Guide

The guide uses [mdBook](https://rust-lang.github.io/mdBook/).

Preview the guide from the repository root:

```sh
mdbook serve guide --open
```

`mdbook build guide` writes the book to `target/guide/`.

Build the full site, including browser examples:

```sh
mdbook build guide --dest-dir target/site
node examples/palette/web/build.mjs
```

The result is in `target/site/`: the book at the root and browser examples under
`examples/<name>/`. Build the book first because mdBook clears its output directory.
