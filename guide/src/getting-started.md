# Getting started

wasmgc provides garbage-collected structs and arrays for Rust on WebAssembly.
The engine manages the objects' memory.

## Setup

Create a Rust library and add `wasmgc`:

```sh
cargo new --lib counter
cd counter
cargo add wasmgc
```

Install the Wasm target and the postprocessing tool:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasmgc --features cli
```

Configure `Cargo.toml` to produce a Wasm module:

```toml
[lib]
crate-type = ["cdylib"]
```

## A Rust example

Replace `src/lib.rs` with:

```rust
use wasmgc::{Gc, wasmgc};

#[wasmgc]
struct Counter {
    value: i32,
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() {
    let counter = Gc::new(Counter { value: 1 });
    counter.set_value(2);
    assert_eq!(counter.value(), 2);
}
```

`#[wasmgc]` generates the struct's field accessors. `Gc::new` creates an object
on the engine's GC heap; `value()` reads its field and `set_value()` updates it.
`_start` is the entry point invoked by `wasmtime run`.

## Build and run

Install [Wasmtime](https://docs.wasmtime.dev/cli-install.html) to run the example.
Compile to Wasm, then postprocess the output to replace generated bindings with
WasmGC instructions:

```sh
cargo build --target wasm32-unknown-unknown --release
wasmgc target/wasm32-unknown-unknown/release/counter.wasm -o counter.gc.wasm
wasmtime run counter.gc.wasm
```

The program exits successfully when the assertion passes.

Continue with [Using Rust](rust.md) for structs and arrays, or
[Host interoperability](interop.md) for loading a module and exposing APIs to a host.
