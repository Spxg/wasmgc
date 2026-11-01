# Benchmarks

Measured on 2026-11-02: Intel Core Ultra 7 265 (one P core), Node.js 26.11.1,
Rust 1.99.1, release builds with LTO. `wbg` means wasm-bindgen 0.2.129.

Each cell shows **wasmgc / wbg (speedup)**, in **ns**. Speedup is wbg / wasmgc;
above 1 favors wasmgc. Times are medians of five per-run means; JS reads and
writes use three warmed-up rechecks.

Rust access: WasmGC storage vs imported JS objects and arrays (`Int32Array` for
`i32`). Structs have one field; arrays have one element.

| Type | Create | Read | Write |
|---|---:|---:|---:|
| Struct, `i32` | 11.48 / 14.45 (1.26×) | 15.41 / 11.93 (0.77×) | 15.77 / 12.48 (0.79×) |
| Array, `i32` | 11.58 / 54.86 (4.74×) | 15.89 / 12.53 (0.79×) | 16.28 / 12.91 (0.79×) |
| Struct, `externref` | 16.09 / 20.20 (1.26×) | 19.89 / 16.67 (0.84×) | 20.84 / 16.18 (0.78×) |
| Array, `externref` | 16.08 / 20.77 (1.29×) | 20.08 / 17.19 (0.86×) | 21.51 / 16.31 (0.76×) |

Rust loops: total time for **1,024 accesses** within one call.

| Type | Read | Write |
|---|---:|---:|
| Struct, `i32` | 223.66 / 2831.68 (12.66×) | 243.19 / 3263.20 (13.42×) |
| Array, `i32` | 224.56 / 3253.47 (14.49×) | 517.18 / 3617.47 (6.99×) |
| Struct, `externref` | 4089.75 / 7055.78 (1.73×) | 691.28 / 3504.81 (5.07×) |
| Array, `externref` | 4126.78 / 7354.37 (1.78×) | 1029.10 / 3984.78 (3.87×) |

JS/Wasm interop. Repeated host calls run inside one Rust call.

| Operation | Count | Time |
|---|---:|---:|
| Add one to an `i32` | 1 | 1.21 / 1.48 (1.22×) |
| Return the same `externref` | 1 | 6.67 / 7.77 (1.17×) |
| Call a JS method through a passed reference | 1 | 9.69 / 13.08 (1.35×) |
| Call a JS method through a passed reference | 1,024 | 3475.96 / 3709.75 (1.07×) |
| Call a JS method through a struct field | 1 | 21.36 / 19.76 (0.93×) |
| Call a JS method through a struct field | 1,024 | 7357.16 / 10829.84 (1.47×) |

JS direct access: native GC structs vs Rust objects through wbg wrappers.
Construction excludes explicit `free()` calls.

| Field | Create | Read | Write |
|---|---:|---:|---:|
| `i32` | 7.01 / 99.39 (14.18×) | 9.35 / 1.63 (0.17×) | 9.78 / 2.39 (0.24×) |
| `externref` | 7.09 / 104.89 (14.80×) | 9.45 / 8.80 (0.93×) | 10.09 / 8.59 (0.85×) |

Slower cases:

- Single Rust accesses pay root-slot allocation, lookup, and release; wbg uses
  borrowed stack slots. Loops amortize the entry cost.
- The tested V8 cannot inline JS→Wasm wrappers with typed GC references, while
  wbg's pointer parameters allow it.
  Native GC accessors have no root-table lookup.
- wbg also manages reference-table slots for `externref` fields, narrowing the
  gap. Small differences vary across runs.

Requires the `wasm32-unknown-unknown` Rust target, `wasm-ld`, and wasm-bindgen CLI
0.2.129.

```sh
cd benchmarks
npm ci
npm run bench
```

`npm run check` checks behavior without timing.
