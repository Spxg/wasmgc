// Build both implementations, compare their observable results, then benchmark
// matched callbacks. --check performs the build and comparisons without timing.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFile, mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { parseArgs } from "node:util";
import { bench, do_not_optimize, run, summary } from "mitata";
import { counter_add, make_counter } from "./wasm-bindgen/src/host.js";

const { values: options } = parseArgs({
    options: {
        check: { type: "boolean", default: false },
    },
});
const directory = fileURLToPath(new URL(".", import.meta.url));
const repository = fileURLToPath(new URL("..", import.meta.url));
const output = join(repository, "target", "benchmarks");
const target = join(output, "rust");
const artifacts = join(target, "wasm32-unknown-unknown", "release");
// Loop cases cross the JS/Wasm boundary once for many Rust-side accesses.
const loopIterations = 1024;
const references = [{ value: 1 }, { value: 2 }];

function execute(command, args, env = process.env) {
    const result = spawnSync(command, args, {
        cwd: repository,
        env,
        stdio: "inherit",
    });
    if (result.error) throw result.error;
    if (result.status !== 0) throw new Error(`${command} failed with exit status ${result.status}`);
}

await mkdir(output, { recursive: true });
await writeFile(join(output, "package.json"), '{"type":"module"}\n');

execute("cargo", [
    "build", "--manifest-path", join(directory, "Cargo.toml"), "--workspace",
    "--target", "wasm32-unknown-unknown", "--release", "--locked",
    "--target-dir", target,
], { ...process.env, RUSTFLAGS: "-C target-feature=+reference-types" });

execute("cargo", [
    "build", "--manifest-path", join(repository, "Cargo.toml"), "-p", "wasmgc",
    "--features", "cli", "--bin", "wasmgc", "--release", "--locked", "--target-dir", target,
]);
const gcPath = join(output, "wasmgc.wasm");
execute(join(target, "release", "wasmgc"), [join(artifacts, "wasmgc_benchmarks.wasm"), "-o", gcPath]);

const bindgenOutput = join(output, "wasm-bindgen");
execute("wasm-bindgen", [
    join(artifacts, "wasm_bindgen_benchmarks.wasm"), "--target", "web",
    "--out-dir", bindgenOutput, "--out-name", "wasm_bindgen", "--no-typescript",
]);

const bindgenPath = join(bindgenOutput, "wasm_bindgen_bg.wasm");
for (const path of [gcPath, bindgenPath]) {
    const unoptimized = path.replace(/\.wasm$/, ".unoptimized.wasm");
    await copyFile(path, unoptimized);
    execute("wasm-opt", [unoptimized, "-O3", "--enable-gc", "-o", path]);
}

const { instance } = await WebAssembly.instantiate(await readFile(gcPath), {
    env: { counter_add },
});
const gc = instance.exports;
const wbg = await import(pathToFileURL(join(bindgenOutput, "wasm_bindgen.js")));
wbg.initSync({ module: await readFile(bindgenPath) });
const { ExportedValue, ExportedExternalValue } = wbg;

const nativeCases = [
    {
        name: "bench_js_struct",
        inputs: [1, 2],
        values: [-1, 0, 42, 0x7fffffff],
        gc: {
            create: gc["Value.new"],
            get: gc["Value.get_value"],
            set: gc["Value.set_value"],
            dispose() {},
        },
        wbg: {
            create: value => new ExportedValue(value),
            get: object => object.value,
            set(object, value) { object.value = value; },
            dispose: object => object.free(),
        },
    },
    {
        name: "bench_js_externref_struct",
        inputs: references,
        values: [null, undefined, 42, "value", ...references],
        gc: {
            create: gc["ExternalValue.new"],
            get: gc["ExternalValue.get_external"],
            set: gc["ExternalValue.set_external"],
            dispose() {},
        },
        wbg: {
            create: external => new ExportedExternalValue(external),
            get: object => object.external,
            set(object, external) { object.external = external; },
            dispose: object => object.free(),
        },
    },
];

const names = Object.keys(gc).filter(name => name.startsWith("bench_")).sort();
assert.deepEqual(names, Object.keys(wbg).filter(name => name.startsWith("bench_")).sort());

for (const api of [gc, wbg]) {
    for (const value of [-1, 0, 42, 0x7fffffff]) {
        assert.equal(api.bench_scalar(value), (value + 1) | 0);
        for (const type of ["struct", "array"]) {
            const object = api[`bench_${type}_new`](value);
            assert.equal(api[`bench_${type}_get`](object), value);
            const next = (value + 1) | 0;
            api[`bench_${type}_set`](object, next);
            assert.equal(api[`bench_${type}_get`](object), next);

            for (const iterations of [0, 1, 3, loopIterations]) {
                const object = api[`bench_${type}_new`](value);
                assert.equal(api[`bench_${type}_get_loop`](object, iterations), Math.imul(value, iterations));
                api[`bench_${type}_set_loop`](object, value, iterations);
                assert.equal(api[`bench_${type}_get`](object), iterations === 0 ? value : (value + iterations - 1) | 0);
            }
        }
    }

    for (const value of [null, undefined, 42, "value", ...references]) {
        assert.equal(api.bench_externref_identity(value), value);
        for (const type of ["struct", "array"]) {
            const prefix = `bench_externref_${type}`;
            const object = api[`${prefix}_new`](value);
            assert.equal(api[`${prefix}_get`](object), value);
            api[`${prefix}_set`](object, references[0]);
            assert.equal(api[`${prefix}_get`](object), references[0]);

            for (const iterations of [0, 1, 3, loopIterations]) {
                api[`${prefix}_set`](object, value);
                assert.equal(api[`${prefix}_get_loop`](object, iterations), iterations === 0 ? null : value);
                api[`${prefix}_set_loop`](object, references[0], references[1], iterations);
                assert.equal(api[`${prefix}_get`](object), iterations === 0 ? value : references[(iterations - 1) & 1]);
            }
        }
    }

    for (const name of ["bench_externref_call", "bench_externref_struct_call"]) {
        for (const value of [-1, 0, 0x7fffffff]) {
            for (const delta of [-1, 0, 1, 3]) {
                const counter = make_counter(value);
                const object = name.includes("_struct_") ? api.bench_externref_struct_new(counter) : counter;
                const next = (value + delta) | 0;
                assert.equal(api[name](object, delta), next);
                assert.equal(counter.value, next);

                for (const iterations of [0, 1, 3, loopIterations]) {
                    const next = (counter.value + Math.imul(delta, iterations)) | 0;
                    assert.equal(api[`${name}_loop`](object, delta, iterations), iterations === 0 ? 0 : next);
                    assert.equal(counter.value, next);
                }
            }
        }
    }
}
for (const type of nativeCases) {
    for (const api of [type.gc, type.wbg]) {
        for (const [index, value] of type.values.entries()) {
            const object = api.create(value);
            assert.equal(api.get(object), value);
            const next = type.values[(index + 1) % type.values.length];
            api.set(object, next);
            assert.equal(api.get(object), next);
            api.dispose(object);
        }
    }
}
console.log(`Checked ${names.length + nativeCases.length * 3} equivalent benchmark pairs.`);
console.log(`Loop cases run ${loopIterations} Rust accesses per iteration.`);

function externrefCallback(api, name) {
    const fn = api[name];
    const operation = name.replace(/_loop$/, "");
    let index = 0;
    if (operation.endsWith("_call")) {
        const counter = make_counter();
        const object = operation.includes("_struct_") ? api.bench_externref_struct_new(counter) : counter;
        return name.endsWith("_loop")
            ? () => do_not_optimize(fn(object, 1, loopIterations))
            : () => do_not_optimize(fn(object, 1));
    }
    if (operation.endsWith("_get") || operation.endsWith("_set")) {
        const object = api[operation.replace(/_(get|set)$/, "_new")](references[0]);
        if (name.endsWith("_loop")) {
            return operation.endsWith("_get")
                ? () => do_not_optimize(fn(object, loopIterations))
                : () => do_not_optimize(fn(object, references[0], references[1], loopIterations));
        }
        return operation.endsWith("_get")
            ? () => do_not_optimize(fn(object))
            : () => do_not_optimize(fn(object, references[index++ & 1]));
    }
    return () => do_not_optimize(fn(references[index++ & 1]));
}

function callback(api, name) {
    if (name.startsWith("bench_externref_")) return externrefCallback(api, name);
    const fn = api[name];
    const operation = name.replace(/_loop$/, "");
    let value = 0;
    if (operation.endsWith("_get") || operation.endsWith("_set")) {
        const object = api[operation.replace(/_(get|set)$/, "_new")](42);
        if (name.endsWith("_loop")) {
            return operation.endsWith("_get")
                ? () => do_not_optimize(fn(object, loopIterations))
                : () => do_not_optimize(fn(object, value = (value + 1) | 0, loopIterations));
        }
        return operation.endsWith("_get")
            ? () => do_not_optimize(fn(object))
            : () => do_not_optimize(fn(object, value = (value + 1) | 0));
    }
    return () => do_not_optimize(fn(value = (value + 1) | 0));
}

function nativeCallback(api, operation, inputs) {
    if (operation === "new") {
        return function* () {
            const pending = [];
            let index = 0;
            // Release wasm-bindgen wrappers between samples so explicit disposal
            // stays outside the timed constructor callback.
            function release() {
                for (const object of pending) api.dispose(object);
                pending.length = 0;
            }
            yield {
                [0]() {
                    release();
                    return inputs[index++ & 1];
                },
                bench(value) {
                    const object = api.create(value);
                    pending.push(object);
                    do_not_optimize(object);
                },
            };
            release();
        };
    }
    return async function* () {
        const object = api.create(inputs[0]);
        let index = 0;
        const callback = operation === "get"
            ? () => do_not_optimize(api.get(object))
            : () => do_not_optimize(api.set(object, inputs[index++ & 1]));
        // Warm up before mitata selects its batch size.
        for (let iteration = 0; iteration < 10_000; iteration++) callback();
        await new Promise(resolve => setImmediate(resolve));
        yield callback;
        api.dispose(object);
    };
}

for (const name of names) {
    summary(() => {
        bench(`${name} / wasmgc`, callback(gc, name)).baseline();
        bench(`${name} / wasm-bindgen`, callback(wbg, name));
    });
}

for (const type of nativeCases) {
    for (const operation of ["new", "get", "set"]) {
        summary(() => {
            bench(`${type.name}_${operation} / wasmgc`, nativeCallback(type.gc, operation, type.inputs)).baseline();
            bench(`${type.name}_${operation} / wasm-bindgen`, nativeCallback(type.wbg, operation, type.inputs));
        });
    }
}

if (!options.check) {
    await run({ throw: true });
}
