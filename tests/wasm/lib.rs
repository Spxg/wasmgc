use wasmgc::{Gc, wasmgc};

mod array;
mod externref;
mod gc;
mod reference;
mod scalar;

#[wasmgc(constructor)]
pub struct Counter {
    #[getter]
    #[setter]
    pub value: i32,
}

#[wasmgc]
pub fn test_counter() {
    let counter = Gc::new(Counter { value: 1 });

    assert_eq!(counter.value(), 1);

    counter.set_value(2);
    assert_eq!(counter.value(), 2);
}

#[wasmgc]
pub fn test_clone_survives_drop() {
    let original = Gc::new(Counter { value: 1 });
    let alias = original.clone();

    assert!(Gc::ref_eq(&original, &alias));

    alias.set_value(2);
    assert_eq!(original.value(), 2);

    drop(original);
    assert_eq!(alias.value(), 2);
}

#[wasmgc]
pub fn make_counter(value: i32) -> Gc<Counter> {
    Gc::new(Counter { value })
}

#[wasmgc]
pub fn identity_counter(counter: Gc<Counter>) -> Gc<Counter> {
    counter
}

#[wasmgc]
pub fn increment_counter(counter: &Gc<Counter>) -> i32 {
    counter.set_value(counter.value() + 1);
    counter.value()
}

#[wasmgc]
pub fn nullable_counter(counter: Option<Gc<Counter>>) -> Option<Gc<Counter>> {
    counter
}
