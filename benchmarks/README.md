# Browser benchmarks

Compare wasmgc and wasm-bindgen in your browser. **Run** tests both
implementations; **Run all** queues every benchmark. **(?)** shows their Rust
code and data structures.

[Open the benchmarks](https://spxg.github.io/wasmgc/benchmarks/).

Install Binaryen's `wasm-opt`, the Rust Wasm target, and matching wasm-bindgen CLI:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
```

Start the page:

```sh
cd benchmarks
npm ci
npm run dev
```

Open <http://127.0.0.1:8080>. `npm run build` writes the static page to
`target/site/benchmarks/`. Both Wasm crates use release builds with LTO,
followed by `wasm-opt -O3 --all-features`.

| Group | Coverage |
|---|---|
| JS accesses Rust data | Create a Point; read/write struct fields |
| Object references | Read/replace shared children; update a value shared by 16 holders; traverse `Node { value, next }` using `Gc` versus `Rc` |

Results are nanoseconds per operation; lower is better, and the faster result
is bold. `±` is the relative margin of error at 95% confidence. Shared updates
report one write and reads through all 16 holders. Linked-list traversals
report the whole 1,024-node traversal.

Fixture setup and cleanup, including returned child and Node wrappers, are
outside timing. GC runs naturally and is not forced.

Each measurement runs in a fresh Worker to isolate the two implementations.
