//! wasm-bindgen operations paired with the WasmGC benchmark crate.
//!
//! Host-backed values use the storage defined in `host.js`. The `ExportedValue`
//! types additionally measure JavaScript access to Rust-owned objects through
//! wasm-bindgen wrappers. The Node.js runner pairs cases by their `bench_` names.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "/src/host.js")]
extern "C" {
    /// A JavaScript object with one i32-valued property.
    pub type Value;

    fn make_value(value: i32) -> Value;

    #[wasm_bindgen(method, getter, structural)]
    fn value(this: &Value) -> i32;

    #[wasm_bindgen(method, setter, structural)]
    fn set_value(this: &Value, value: i32);

    /// A JavaScript Int32Array used for scalar array operations.
    pub type Values;

    fn make_array(value: i32) -> Values;

    fn array_get(values: &Values, index: u32) -> i32;

    fn array_set(values: &Values, index: u32, value: i32);

    /// A JavaScript object holding an arbitrary host value.
    pub type ExternalValue;

    fn make_external_value(external: &JsValue) -> ExternalValue;

    #[wasm_bindgen(method, getter, structural)]
    fn external(this: &ExternalValue) -> JsValue;

    #[wasm_bindgen(method, setter, structural)]
    fn set_external(this: &ExternalValue, external: &JsValue);

    /// A JavaScript array used for host-reference operations.
    pub type ExternalValues;

    fn make_external_array(external: &JsValue) -> ExternalValues;

    fn external_array_get(values: &ExternalValues, index: u32) -> JsValue;

    fn external_array_set(values: &ExternalValues, index: u32, external: &JsValue);

    fn counter_add(counter: &JsValue, delta: i32) -> i32;
}

/// Adds one with i32 wrapping to measure a scalar call across the boundary.
#[wasm_bindgen]
pub fn bench_scalar(value: i32) -> i32 {
    value.wrapping_add(1)
}

/// Creates a single-field object and returns it to JavaScript.
#[wasm_bindgen]
pub fn bench_struct_new(value: i32) -> Value {
    make_value(value)
}

/// Reads one scalar field during a single Rust call.
#[wasm_bindgen]
pub fn bench_struct_get(value: &Value) -> i32 {
    value.value()
}

/// Writes one scalar field during a single Rust call.
#[wasm_bindgen]
pub fn bench_struct_set(value: &Value, new_value: i32) {
    value.set_value(new_value);
}

/// Creates a one-element scalar array and returns it to JavaScript.
#[wasm_bindgen]
pub fn bench_array_new(value: i32) -> Values {
    make_array(value)
}

/// Reads the first scalar array element during a single Rust call.
#[wasm_bindgen]
pub fn bench_array_get(values: &Values) -> i32 {
    array_get(values, 0)
}

/// Writes the first scalar array element during a single Rust call.
#[wasm_bindgen]
pub fn bench_array_set(values: &Values, value: i32) {
    array_set(values, 0, value);
}

/// Sums repeated field reads with wrapping arithmetic inside Rust.
#[wasm_bindgen]
pub fn bench_struct_get_loop(value: &Value, iterations: u32) -> i32 {
    let mut sum: i32 = 0;
    for _ in 0..iterations {
        sum = sum.wrapping_add(value.value());
    }
    sum
}

/// Writes successive i32 values to the same field inside Rust.
#[wasm_bindgen]
pub fn bench_struct_set_loop(value: &Value, new_value: i32, iterations: u32) {
    for index in 0..iterations {
        value.set_value(new_value.wrapping_add(index as i32));
    }
}

/// Sums repeated reads of the first array element inside Rust.
#[wasm_bindgen]
pub fn bench_array_get_loop(values: &Values, iterations: u32) -> i32 {
    let mut sum: i32 = 0;
    for _ in 0..iterations {
        sum = sum.wrapping_add(array_get(values, 0));
    }
    sum
}

/// Writes successive i32 values to the first array element inside Rust.
#[wasm_bindgen]
pub fn bench_array_set_loop(values: &Values, value: i32, iterations: u32) {
    for index in 0..iterations {
        array_set(values, 0, value.wrapping_add(index as i32));
    }
}

/// Returns the same host value after passing it through Rust.
#[wasm_bindgen]
pub fn bench_externref_identity(external: JsValue) -> JsValue {
    external
}

/// Creates an object containing the supplied host value.
#[wasm_bindgen]
pub fn bench_externref_struct_new(external: JsValue) -> ExternalValue {
    make_external_value(&external)
}

/// Reads a host value from an object field.
#[wasm_bindgen]
pub fn bench_externref_struct_get(value: &ExternalValue) -> JsValue {
    value.external()
}

/// Replaces the host value stored in an object field.
#[wasm_bindgen]
pub fn bench_externref_struct_set(value: &ExternalValue, external: &JsValue) {
    value.set_external(external);
}

/// Creates a one-element array containing the supplied host value.
#[wasm_bindgen]
pub fn bench_externref_array_new(external: JsValue) -> ExternalValues {
    make_external_array(&external)
}

/// Reads the host value stored in the first array element.
#[wasm_bindgen]
pub fn bench_externref_array_get(values: &ExternalValues) -> JsValue {
    external_array_get(values, 0)
}

/// Replaces the host value stored in the first array element.
#[wasm_bindgen]
pub fn bench_externref_array_set(values: &ExternalValues, external: &JsValue) {
    external_array_set(values, 0, external);
}

/// Reads a host-value field repeatedly and returns the last value.
///
/// Returns a null host value when `iterations` is zero.
#[wasm_bindgen]
pub fn bench_externref_struct_get_loop(value: &ExternalValue, iterations: u32) -> JsValue {
    let mut result = JsValue::NULL;
    for _ in 0..iterations {
        result = value.external();
    }
    result
}

/// Alternates two host values in the same field inside Rust.
#[wasm_bindgen]
pub fn bench_externref_struct_set_loop(
    value: &ExternalValue,
    first: &JsValue,
    second: &JsValue,
    iterations: u32,
) {
    for index in 0..iterations {
        value.set_external(if index & 1 == 0 { first } else { second });
    }
}

/// Reads the first host-value array element repeatedly and returns the last value.
///
/// Returns a null host value when `iterations` is zero.
#[wasm_bindgen]
pub fn bench_externref_array_get_loop(values: &ExternalValues, iterations: u32) -> JsValue {
    let mut result = JsValue::NULL;
    for _ in 0..iterations {
        result = external_array_get(values, 0);
    }
    result
}

/// Alternates two host values in the first array element inside Rust.
#[wasm_bindgen]
pub fn bench_externref_array_set_loop(
    values: &ExternalValues,
    first: &JsValue,
    second: &JsValue,
    iterations: u32,
) {
    for index in 0..iterations {
        external_array_set(values, 0, if index & 1 == 0 { first } else { second });
    }
}

/// Calls the host counter once using a host value passed directly to Rust.
#[wasm_bindgen]
pub fn bench_externref_call(external: &JsValue, delta: i32) -> i32 {
    counter_add(external, delta)
}

/// Calls the host counter repeatedly using a host value passed directly to Rust.
///
/// Returns zero when `iterations` is zero.
#[wasm_bindgen]
pub fn bench_externref_call_loop(external: &JsValue, delta: i32, iterations: u32) -> i32 {
    let mut result = 0;
    for _ in 0..iterations {
        result = counter_add(external, delta);
    }
    result
}

/// Reads a host counter from an object field and calls its add method.
#[wasm_bindgen]
pub fn bench_externref_struct_call(value: &ExternalValue, delta: i32) -> i32 {
    let external = value.external();
    counter_add(&external, delta)
}

/// Reads and calls the stored host counter on each Rust iteration.
///
/// Returns zero when `iterations` is zero.
#[wasm_bindgen]
pub fn bench_externref_struct_call_loop(value: &ExternalValue, delta: i32, iterations: u32) -> i32 {
    let mut result = 0;
    for _ in 0..iterations {
        let external = value.external();
        result = counter_add(&external, delta);
    }
    result
}

/// Rust-owned scalar storage exposed to JavaScript by wasm-bindgen.
#[wasm_bindgen]
pub struct ExportedValue {
    /// Scalar field exposed through the generated JavaScript accessors.
    pub value: i32,
}

#[wasm_bindgen]
impl ExportedValue {
    /// Creates the Rust-owned object for direct JavaScript field access.
    #[wasm_bindgen(constructor)]
    pub fn new(value: i32) -> Self {
        Self { value }
    }
}

/// Rust-owned host-reference storage exposed through a JavaScript wrapper.
#[wasm_bindgen]
pub struct ExportedExternalValue {
    external: JsValue,
}

#[wasm_bindgen]
impl ExportedExternalValue {
    /// Creates a Rust-owned object containing the supplied host value.
    #[wasm_bindgen(constructor)]
    pub fn new(external: JsValue) -> Self {
        Self { external }
    }

    /// Returns another handle to the stored host value.
    #[wasm_bindgen(getter)]
    pub fn external(&self) -> JsValue {
        self.external.clone()
    }

    /// Replaces the stored host value.
    #[wasm_bindgen(setter)]
    pub fn set_external(&mut self, external: JsValue) {
        self.external = external;
    }
}
