# Host interoperability

After postprocessing, host functions exchange actual Wasm references. JavaScript
receives GC objects and host values directly, without numeric handles.

## Loading a module

In a Rust library project, put the following examples in `src/lib.rs` and configure
`Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib"]
```

Build the library for `wasm32-unknown-unknown` and postprocess it:

```sh
rustup target add wasm32-unknown-unknown
cargo build --target wasm32-unknown-unknown --release
wasmgc target/wasm32-unknown-unknown/release/your_crate.wasm -o app.wasm
```

Replace `your_crate.wasm` with the library's output filename. Serve `app.wasm`
alongside your page and load it with:

```js
const bytes = await (await fetch("./app.wasm")).arrayBuffer();
const { instance } = await WebAssembly.instantiate(bytes);
const wasm = instance.exports;
```

## Struct exports

Choose which operations to expose to the host:

```rust
use wasmgc::{Gc, wasmgc};

#[wasmgc(constructor)]
pub struct Counter {
    #[getter]
    #[setter]
    value: i32,
}
```

JavaScript can construct and update a counter:

```js
const counter = wasm["Counter.new"](1);
wasm["Counter.set_value"](counter, 2);
console.log(wasm["Counter.get_value"](counter)); // 2
```

`constructor` exports `Counter.new`, with parameters in field order. Field
`#[getter]` and `#[setter]` attributes independently export the corresponding
operations. These options only control host exports: Rust always gets its
field methods, even when the attributes are omitted.

## Function exports

Apply `#[wasmgc]` to a free function to export it under its Rust name:

```rust
#[wasmgc]
pub fn counter(value: i32) -> Gc<Counter> {
    Gc::new(Counter { value })
}

#[wasmgc]
pub fn read(counter: &Gc<Counter>) -> i32 {
    counter.value()
}
```

```js
const value = wasm.counter(3);
console.log(wasm.read(value)); // 3
```

The functions remain callable from Rust. Parameters may be owned values or
shared borrows; return values must be owned or `()`. Supported values include
scalars, `Gc<T>`, `GcArray<T>`, `Option<Gc<T>>`, and `ExternRef`. Functions must
be synchronous and non-generic. Export names do not include Rust module paths.

## Host imports and `ExternRef`

`ExternRef` keeps a host value alive. Declare host operations in a
`#[wasmgc] extern "wasmgc"` block; imports use the `env` module and their Rust
function names.

```rust
use wasmgc::ExternRef;

#[wasmgc]
extern "wasmgc" {
    /// # Safety
    ///
    /// The host value must have an add(i32) method returning an i32 normally.
    fn counter_add(counter: &ExternRef, delta: i32) -> i32;
}

/// # Safety
///
/// `counter` must satisfy the `counter_add` host contract.
#[wasmgc]
pub unsafe fn increment(counter: ExternRef) -> i32 {
    // SAFETY: The host supplies a counter satisfying counter_add's contract.
    unsafe { counter_add(&counter, 1) }
}
```

Supply the import when instantiating the processed module:

```js
const { instance } = await WebAssembly.instantiate(bytes, {
    env: { counter_add: (counter, delta) => counter.add(delta) },
});
const counter = { value: 0, add(delta) { return this.value += delta; } };
console.log(instance.exports.increment(counter)); // 1
```

Imports are unsafe to call by default. Host implementations must satisfy their
declared contract and return normally; unwinding through the generated bridges
is unsupported. An import may use `safe fn` when its host implementation is safe
for every argument allowed by its Rust types.

`ExternRef` can also be stored in GC fields and arrays. Cloning or storing a
reference retains the same host value; it does not copy the host object.
