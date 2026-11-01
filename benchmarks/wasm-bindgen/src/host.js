// JavaScript storage used by the wasm-bindgen cases. Counter objects are also
// passed to WasmGC cases, so both implementations call the same host method.

export function make_value(value) {
  return { value };
}

export function make_array(value) {
  return new Int32Array([value]);
}

export function array_get(values, index) {
  return values[index];
}

export function array_set(values, index, value) {
  values[index] = value;
}

export function make_external_value(external) {
  return { external };
}

export function make_external_array(external) {
  return [external];
}

export function external_array_get(values, index) {
  return values[index];
}

export function external_array_set(values, index, external) {
  values[index] = external;
}

export function make_counter(value = 0) {
  return {
    value,
    add(delta) {
      // Match Rust's wrapping i32 arithmetic, including overflow.
      this.value = (this.value + delta) | 0;
      return this.value;
    },
  };
}

export function counter_add(counter, delta) {
  return counter.add(delta);
}
