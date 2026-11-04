# wasmgc

[![Crates.io](https://img.shields.io/crates/v/wasmgc.svg)](https://crates.io/crates/wasmgc)

A Rust implementation of WasmGC.

Create and mutate objects on the WebAssembly engine's garbage-collected heap
from Rust:

- `Gc<T>` holds a managed object; `#[wasmgc]` defines structs and their accessors.
- `GcArray<T>` holds a fixed-length array of scalars or references.
- `ExternRef` keeps host values alive and lets GC objects store them directly.

```rust
use wasmgc::{Gc, wasmgc};

#[wasmgc]
struct Counter {
    value: i32,
}

let counter = Gc::new(Counter { value: 1 });
counter.set_value(2);
assert_eq!(counter.value(), 2);
```

Compile your application to Wasm, then run the `wasmgc` post-link tool to supply
GC instructions and reference adapters. Imported and exported functions can
exchange native GC references with the host.

```sh
cargo install wasmgc --features cli
wasmgc input.wasm -o output.wasm
```

[Getting started](https://spxg.github.io/wasmgc/getting-started.html) ·
[API reference](https://docs.rs/wasmgc/)
