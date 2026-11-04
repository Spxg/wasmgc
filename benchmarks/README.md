# Benchmarks

Measured on 2026-11-04: Intel Core Ultra 7 265 (one P core), Node.js 26.11.1,
Rust 1.99.1, wasm-bindgen 0.2.130, wasm-opt 130. Both release builds use LTO,
followed by `wasm-opt -O3 --enable-gc` after their binding generation.

Each cell shows **wasmgc / wbg (speedup)**, in **ns**. Speedup is wbg / wasmgc;
above 1 favors wasmgc. Times are medians of five independent per-run means.

Rust access: WasmGC storage vs imported JS objects and arrays (`Int32Array` for
`i32`). Structs have one field; arrays have one element.

| Type | Create | Read | Write |
|---|---:|---:|---:|
| Struct, `i32` | 11.20 / 13.45 (1.20×) | 13.26 / 11.94 (0.90×) | 13.76 / 12.51 (0.91×) |
| Array, `i32` | 11.33 / 53.51 (4.72×) | 14.30 / 12.55 (0.88×) | 14.74 / 12.90 (0.88×) |
| Struct, `externref` | 16.01 / 19.17 (1.20×) | 17.61 / 16.66 (0.95×) | 18.99 / 16.21 (0.85×) |
| Array, `externref` | 15.05 / 19.72 (1.31×) | 18.42 / 17.23 (0.94×) | 19.88 / 16.34 (0.82×) |

Rust loops: total time for **1,024 accesses** within one call.

| Type | Read | Write |
|---|---:|---:|
| Struct, `i32` | 222.81 / 2824.65 (12.68×) | 240.91 / 3282.56 (13.63×) |
| Array, `i32` | 223.72 / 3256.24 (14.55×) | 519.26 / 3668.90 (7.07×) |
| Struct, `externref` | 4118.71 / 7134.62 (1.73×) | 715.08 / 3498.87 (4.89×) |
| Array, `externref` | 4154.50 / 7360.20 (1.77×) | 1062.67 / 3971.23 (3.74×) |

JS/Wasm interop. Repeated host calls run inside one Rust call.

| Operation | Count | Time |
|---|---:|---:|
| Add one to an `i32` | 1 | 1.20 / 1.48 (1.23×) |
| Return the same `externref` | 1 | 6.34 / 8.00 (1.26×) |
| Call a JS method through a passed reference | 1 | 9.22 / 13.09 (1.42×) |
| Call a JS method through a passed reference | 1,024 | 3465.32 / 3704.66 (1.07×) |
| Call a JS method through a struct field | 1 | 19.99 / 19.56 (0.98×) |
| Call a JS method through a struct field | 1,024 | 7410.06 / 10819.90 (1.46×) |

JS direct access: native GC structs vs Rust objects through wbg wrappers.
Construction excludes explicit `free()` calls.

| Field | Create | Read | Write |
|---|---:|---:|---:|
| `i32` | 7.04 / 98.40 (13.97×) | 9.35 / 1.62 (0.17×) | 9.83 / 2.37 (0.24×) |
| `externref` | 7.08 / 102.15 (14.42×) | 9.52 / 8.60 (0.90×) | 10.34 / 7.86 (0.76×) |

Slower cases:

- Single Rust accesses still allocate, look up, and release root slots.
- This V8 optimizes wbg scalar wrappers with numeric pointers better than
  calls with typed GC parameters. Native GC accessors have no root-table lookup.
- Reading `externref` elements still creates roots. Small differences vary
  across runs.

Requires the `wasm32-unknown-unknown` Rust target, wasm-bindgen CLI
0.2.130, and wasm-opt 130.

```sh
cd benchmarks
npm ci
npm run bench
```

`npm run check` checks behavior without timing.
