# wasmgc

[![Crates.io](https://img.shields.io/crates/v/wasmgc.svg)](https://crates.io/crates/wasmgc)

Garbage-collected structs and arrays for Rust on WebAssembly.

```rust
use wasmgc::{Gc, wasmgc};

#[wasmgc]
struct Counter {
    value: i32,
}

fn example() {
    let counter = Gc::new(Counter { value: 1 });
    let shared = counter.clone();

    shared.set_value(2);
    assert_eq!(counter.value(), 2);
}
```

Cloning a `Gc` shares the same object. The engine collects unreachable objects,
including cycles.

See the [guide](https://spxg.github.io/wasmgc/) for building, Rust APIs, and host
interoperability. Requires a WasmGC-capable engine and postprocessing with `wasmgc`.

[Examples](examples) · [Benchmarks](benchmarks)
