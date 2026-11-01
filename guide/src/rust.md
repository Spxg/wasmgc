# Using Rust

These examples run inside a Wasm module after [postprocessing](getting-started.md).

## Structs

`#[wasmgc]` defines a managed struct. `Gc::new` consumes its initial field
values and creates an object on the engine's GC heap.

```rust
use wasmgc::{Gc, GcArray, wasmgc};

#[wasmgc]
struct Counter {
    value: i32,
}

let counter = Gc::new(Counter { value: 1 });
let shared = counter.clone();

shared.set_value(2);
assert_eq!(counter.value(), 2);
assert!(Gc::ref_eq(&counter, &shared));
```

The macro generates `value()` and `set_value()` with the field's Rust
visibility. Cloning creates another root to the same object; both roots observe
the same changes. There is no Rust `&Counter` into the managed object's storage.

## Reference fields

Use `Gc<T>` for a reference field, or `Option<Gc<T>>` when it may be null.
Reference setters borrow their arguments, and getters return owned roots.

```rust
#[wasmgc]
struct Holder {
    counter: Option<Gc<Counter>>,
}

let counter = Gc::new(Counter { value: 3 });
let holder = Gc::new(Holder { counter: None });
holder.set_counter(Some(&counter));

drop(counter);
let retained = holder.counter().unwrap();
assert_eq!(retained.value(), 3);

holder.set_counter(None);
assert_eq!(retained.value(), 3);
```

The holder keeps its field reachable after the original root is dropped. The
getter's returned root then keeps the counter alive after the field is cleared.
The same reference types can form recursive and mutually referencing structs.

## Arrays

`GcArray<T>` is an alias for `Gc<Array<T>>`. Its length is fixed at construction.

```rust
let numbers = GcArray::from_slice([1_i32, 2, 3]);
numbers.set(1, 9);
assert_eq!(numbers.get(1), Some(9));
assert_eq!(numbers.get(3), None);

let zeros = GcArray::<i32>::new_default(4);
assert_eq!(zeros.len(), 4);
assert_eq!(zeros.get(0), Some(0));
```

Arrays can also hold references. Copying references preserves object identity;
it does not copy the objects themselves.

```rust
let counter = Gc::new(Counter { value: 5 });
let counters = GcArray::from_slice([counter.clone()]);
counters.set(0, &counter);

drop(counter);
assert_eq!(counters.get(0).unwrap().value(), 5);
```

`from_slice` copies elements from a Rust slice, array, or vector into GC
storage. It does not transfer the Rust allocation into the GC heap. Array
setters panic on an out-of-bounds index.

## Reachability

A `Gc` owns a root. Dropping it releases that root; the object remains alive
while another root, host reference, or reachable GC object retains it. The
engine can collect unreachable cycles.

Managed objects do not run Rust destructors. Their Rust struct supplies
construction values and must not implement `Drop`. Ordinary Rust allocations,
such as `Vec` and `String`, remain in linear memory and cannot be stored as GC
fields directly.
