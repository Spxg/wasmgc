//! WasmGC operations paired with the wasm-bindgen benchmark crate.
//!
//! The Node.js runner supplies native GC references and host values, checks the
//! results of both implementations, and measures them through the same callbacks.
//! Host-counter cases use only the non-throwing counters created by `host.js`.

use wasmgc::{ExternRef, Gc, GcArray, wasmgc};

/// Scalar storage used by Rust-access and direct JavaScript-access cases.
#[wasmgc(constructor)]
pub struct Value {
    #[getter]
    #[setter]
    value: i32,
}

/// Adds one with i32 wrapping to measure a scalar call across the boundary.
#[wasmgc]
pub fn bench_scalar(value: i32) -> i32 {
    value.wrapping_add(1)
}

/// Creates a single-field object and returns it to JavaScript.
#[wasmgc]
pub fn bench_struct_new(value: i32) -> Gc<Value> {
    Gc::new(Value { value })
}

/// Reads one scalar field during a single Rust call.
#[wasmgc]
pub fn bench_struct_get(value: &Gc<Value>) -> i32 {
    value.value()
}

/// Writes one scalar field during a single Rust call.
#[wasmgc]
pub fn bench_struct_set(value: &Gc<Value>, new_value: i32) {
    value.set_value(new_value);
}

/// Creates a one-element scalar array and returns it to JavaScript.
#[wasmgc]
pub fn bench_array_new(value: i32) -> GcArray<i32> {
    GcArray::array_new(1, value)
}

/// Reads the first scalar array element during a single Rust call.
///
/// # Panics
///
/// Panics if the array is empty.
#[wasmgc]
pub fn bench_array_get(values: &GcArray<i32>) -> i32 {
    values.get(0).unwrap()
}

/// Writes the first scalar array element during a single Rust call.
///
/// # Panics
///
/// Panics if the array is empty.
#[wasmgc]
pub fn bench_array_set(values: &GcArray<i32>, value: i32) {
    values.set(0, value);
}

/// Sums repeated field reads with wrapping arithmetic inside Rust.
#[wasmgc]
pub fn bench_struct_get_loop(value: &Gc<Value>, iterations: u32) -> i32 {
    let mut sum: i32 = 0;
    for _ in 0..iterations {
        sum = sum.wrapping_add(value.value());
    }
    sum
}

/// Writes successive i32 values to the same field inside Rust.
#[wasmgc]
pub fn bench_struct_set_loop(value: &Gc<Value>, new_value: i32, iterations: u32) {
    for index in 0..iterations {
        value.set_value(new_value.wrapping_add(index as i32));
    }
}

/// Sums repeated reads of the first array element inside Rust.
///
/// # Panics
///
/// Panics if the array is empty and `iterations` is nonzero.
#[wasmgc]
pub fn bench_array_get_loop(values: &GcArray<i32>, iterations: u32) -> i32 {
    let mut sum: i32 = 0;
    for _ in 0..iterations {
        sum = sum.wrapping_add(values.get(0).unwrap());
    }
    sum
}

/// Writes successive i32 values to the first array element inside Rust.
///
/// # Panics
///
/// Panics if the array is empty and `iterations` is nonzero.
#[wasmgc]
pub fn bench_array_set_loop(values: &GcArray<i32>, value: i32, iterations: u32) {
    for index in 0..iterations {
        values.set(0, value.wrapping_add(index as i32));
    }
}

/// Host-reference storage shared by the externref benchmark cases.
#[wasmgc(constructor)]
pub struct ExternalValue {
    #[getter]
    #[setter]
    external: ExternRef,
}

/// Returns the same host value after passing it through Rust.
#[wasmgc]
pub fn bench_externref_identity(external: ExternRef) -> ExternRef {
    external
}

/// Creates an object containing the supplied host value.
#[wasmgc]
pub fn bench_externref_struct_new(external: ExternRef) -> Gc<ExternalValue> {
    Gc::new(ExternalValue { external })
}

/// Reads a host value from an object field.
#[wasmgc]
pub fn bench_externref_struct_get(value: &Gc<ExternalValue>) -> ExternRef {
    value.external()
}

/// Replaces the host value stored in an object field.
#[wasmgc]
pub fn bench_externref_struct_set(value: &Gc<ExternalValue>, external: &ExternRef) {
    value.set_external(external);
}

/// Creates a one-element array containing the supplied host value.
#[wasmgc]
pub fn bench_externref_array_new(external: ExternRef) -> GcArray<ExternRef> {
    GcArray::array_new(1, external)
}

/// Reads the host value stored in the first array element.
///
/// # Panics
///
/// Panics if the array is empty.
#[wasmgc]
pub fn bench_externref_array_get(values: &GcArray<ExternRef>) -> ExternRef {
    values.get(0).unwrap()
}

/// Replaces the host value stored in the first array element.
///
/// # Panics
///
/// Panics if the array is empty.
#[wasmgc]
pub fn bench_externref_array_set(values: &GcArray<ExternRef>, external: &ExternRef) {
    values.set(0, external);
}

/// Reads a host-value field repeatedly and returns the last value.
///
/// Returns a null host value when `iterations` is zero.
#[wasmgc]
pub fn bench_externref_struct_get_loop(value: &Gc<ExternalValue>, iterations: u32) -> ExternRef {
    let mut result = ExternRef::null();
    for _ in 0..iterations {
        result = value.external();
    }
    result
}

/// Alternates two host values in the same field inside Rust.
#[wasmgc]
pub fn bench_externref_struct_set_loop(
    value: &Gc<ExternalValue>,
    first: &ExternRef,
    second: &ExternRef,
    iterations: u32,
) {
    for index in 0..iterations {
        value.set_external(if index & 1 == 0 { first } else { second });
    }
}

/// Reads the first host-value array element repeatedly and returns the last value.
///
/// Returns a null host value when `iterations` is zero.
///
/// # Panics
///
/// Panics if the array is empty and `iterations` is nonzero.
#[wasmgc]
pub fn bench_externref_array_get_loop(values: &GcArray<ExternRef>, iterations: u32) -> ExternRef {
    let mut result = ExternRef::null();
    for _ in 0..iterations {
        result = values.get(0).unwrap();
    }
    result
}

/// Alternates two host values in the first array element inside Rust.
///
/// # Panics
///
/// Panics if the array is empty and `iterations` is nonzero.
#[wasmgc]
pub fn bench_externref_array_set_loop(
    values: &GcArray<ExternRef>,
    first: &ExternRef,
    second: &ExternRef,
    iterations: u32,
) {
    for index in 0..iterations {
        values.set(0, if index & 1 == 0 { first } else { second });
    }
}

#[wasmgc]
extern "wasmgc" {
    /// Calls the host counter's add method.
    ///
    /// # Safety
    ///
    /// `counter` must be produced by the runner's `make_counter`; its `add`
    /// method must accept and return an i32 without throwing.
    fn counter_add(counter: &ExternRef, delta: i32) -> i32;
}

/// Calls the host counter once using a host value passed directly to Rust.
#[wasmgc]
pub fn bench_externref_call(external: &ExternRef, delta: i32) -> i32 {
    // SAFETY: The runner passes a make_counter object; host.js implements
    // its add method with wrapping i32 arithmetic and no throwing operations.
    unsafe { counter_add(external, delta) }
}

/// Calls the host counter repeatedly using a host value passed directly to Rust.
///
/// Returns zero when `iterations` is zero.
#[wasmgc]
pub fn bench_externref_call_loop(external: &ExternRef, delta: i32, iterations: u32) -> i32 {
    let mut result = 0;
    for _ in 0..iterations {
        // SAFETY: Every iteration uses the runner's make_counter object and
        // the non-throwing host.js add implementation.
        result = unsafe { counter_add(external, delta) };
    }
    result
}

/// Reads a host counter from an object field and calls its add method.
#[wasmgc]
pub fn bench_externref_struct_call(value: &Gc<ExternalValue>, delta: i32) -> i32 {
    let external = value.external();
    // SAFETY: The runner stores a make_counter object in this field;
    // host.js supplies its non-throwing i32 add implementation.
    unsafe { counter_add(&external, delta) }
}

/// Reads and calls the stored host counter on each Rust iteration.
///
/// Returns zero when `iterations` is zero.
#[wasmgc]
pub fn bench_externref_struct_call_loop(
    value: &Gc<ExternalValue>,
    delta: i32,
    iterations: u32,
) -> i32 {
    let mut result = 0;
    for _ in 0..iterations {
        let external = value.external();
        // SAFETY: The runner initialized this field with make_counter and
        // does not replace it while the loop calls its non-throwing add method.
        result = unsafe { counter_add(&external, delta) };
    }
    result
}
