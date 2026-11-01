# Getting started

wasmgc provides GC structs, arrays, and host references inside WebAssembly.
Rust owns handles to the objects; the engine manages their memory.

Compile your application to Wasm, then process it with the `wasmgc` CLI. The
tool replaces generated bindings with GC instructions and reference conversions.
The resulting module requires a WasmGC-capable engine.

## Setup

Add `wasmgc` to your application and install the target and CLI:

```sh
cargo add wasmgc
rustup target add wasm32-unknown-unknown
cargo install wasmgc --features cli
```

For a local wasmgc checkout, use a path dependency and install its CLI with
`cargo install --path /path/to/wasmgc --features cli`.

Configure your application's `Cargo.toml` to build a Wasm library:

```toml
[lib]
crate-type = ["cdylib"]
```

## A Rust example

Put this in `src/lib.rs`:

```rust
use wasmgc::{Gc, wasmgc};

#[wasmgc]
struct Counter {
    value: i32,
}

#[wasmgc]
pub fn example() -> i32 {
    let counter = Gc::new(Counter { value: 1 });
    let shared = counter.clone();
    shared.set_value(2);
    counter.value()
}
```

The struct attribute generates Rust field accessors. The function attribute
exports an entry point so a host can invoke this example; ordinary Rust
functions using `Gc` do not need to be exported.

## Build and run

```sh
cargo build --target wasm32-unknown-unknown --release
wasmgc target/wasm32-unknown-unknown/release/your_crate.wasm -o app.wasm
```

Replace `your_crate.wasm` with your application's output filename. In a browser,
serve `app.wasm` alongside the page and load it with:

```js
const bytes = await (await fetch("./app.wasm")).arrayBuffer();
const { instance } = await WebAssembly.instantiate(bytes);
console.log(instance.exports.example()); // 2
```

Continue with [Using Rust](rust.md) for structs and arrays, or
[Host interoperability](interop.md) for reference parameters and host values.
