import { execFileSync, spawnSync } from "node:child_process";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import { setImmediate } from "node:timers/promises";
import { fileURLToPath } from "node:url";

if (typeof globalThis.gc !== "function") {
    const result = spawnSync(process.execPath, [
        "--expose-gc",
        ...process.execArgv,
        fileURLToPath(import.meta.url),
        ...process.argv.slice(2),
    ], { stdio: "inherit" });

    if (result.error) throw result.error;
    process.exit(result.status ?? 1);
}

const repo = fileURLToPath(new URL("..", import.meta.url));
const target = resolve(repo, "target");
const raw = resolve(target, "wasm32-unknown-unknown/release/examples/wasm_tests.wasm");
const output = resolve(target, "wasm_tests.gc.wasm");

function cargo(args) {
    execFileSync("cargo", args, { cwd: repo, stdio: "inherit" });
}

cargo([
    "build", "--locked", "-p", "wasmgc", "--example", "wasm_tests",
    "--target", "wasm32-unknown-unknown", "--release", "--target-dir", target,
]);
cargo([
    "run", "--locked", "-p", "wasmgc", "--features", "cli", "--bin", "wasmgc",
    "--target-dir", target, "--", raw, "-o", output,
]);

const module = await WebAssembly.compile(readFileSync(output));

function instantiate() {
    return new WebAssembly.Instance(module, {
        env: {
            host_object: () => ({
                value: 0,
                add(delta) {
                    this.value += delta;
                    return this.value;
                },
            }),
            host_add: (value, delta) => value.add(delta),
            host_identity: value => value,
        },
    }).exports;
}

async function collect() {
    // A WeakRef target is kept alive until the current JavaScript job ends.
    await setImmediate();
    globalThis.gc();
}

async function assertCollected(...references) {
    for (let attempt = 0; attempt < 10; attempt++) {
        await collect();
        if (references.every(reference => reference.deref() === undefined)) return;
    }
    assert.fail("GC objects remain reachable after collection");
}

function assertInstanceAlive(wasm) {
    // Use the same instance after collection so its tables cannot disappear with it.
    assert.equal(wasm.increment_counter(wasm.make_counter(1)), 2);
}

for (const { name, kind } of WebAssembly.Module.exports(module)) {
    if (kind === "function" && name.startsWith("test_")) {
        test(name, () => {
            const wasm = instantiate();
            wasm[name]();
        });
    }
}

test("test_struct_exports", () => {
    const wasm = instantiate();
    const counter = wasm["Counter.new"](1);

    assert.equal(wasm["Counter.get_value"](counter), 1);
    wasm["Counter.set_value"](counter, 2);
    assert.equal(wasm["Counter.get_value"](counter), 2);
    assert.equal(wasm.increment_counter(counter), 3);
    assert.equal(wasm["Counter.get_value"](counter), 3);
});

test("test_function_exports", () => {
    const wasm = instantiate();
    const counter = wasm.make_counter(5);

    assert.equal(wasm.identity_counter(counter), counter);
    assert.equal(wasm.nullable_counter(counter), counter);
    assert.equal(wasm.nullable_counter(null), null);
    assert.equal(wasm.increment_counter(counter), 6);
    assert.equal(wasm["Counter.get_value"](counter), 6);
});

test("test_externref_exports", () => {
    const wasm = instantiate();

    for (const value of [{}, null, undefined, () => {}]) {
        assert.equal(wasm.identity_externref(value), value);
    }
});

test("test_gc_export_result", async () => {
    const wasm = instantiate();
    const reference = new WeakRef(wasm.make_counter(1));

    await assertCollected(reference);
    assertInstanceAlive(wasm);
});

test("test_gc_roots", async () => {
    const wasm = instantiate();
    const original = wasm.gc_root_new(42);
    const reference = new WeakRef(wasm["wasmgc.ref_get"](original));

    await collect();
    assert.notEqual(reference.deref(), undefined);
    assert.equal(wasm["Counter.get_value"](reference.deref()), 42);

    const alias = wasm.gc_root_clone(original);
    wasm.gc_root_drop(original);

    await collect();
    assert.notEqual(reference.deref(), undefined);
    assert.equal(wasm["Counter.get_value"](reference.deref()), 42);

    wasm.gc_root_drop(alias);
    await assertCollected(reference);
    assertInstanceAlive(wasm);
});

test("test_gc_cycle", async () => {
    const wasm = instantiate();
    const references = (() => {
        const cycle = wasm.gc_make_cycle();
        const children = wasm["GcCycle.get_children"](cycle);
        assert.equal(wasm["GcCycle.get_value"](cycle), 7);
        return [new WeakRef(cycle), new WeakRef(children)];
    })();

    await assertCollected(...references);
    assertInstanceAlive(wasm);
});

test("test_gc_array", async () => {
    const wasm = instantiate();
    const reference = new WeakRef(wasm.gc_make_array());

    await assertCollected(reference);
    assertInstanceAlive(wasm);
});

test("test_gc_externref", async () => {
    const wasm = instantiate();
    const reference = new WeakRef(wasm.identity_externref({}));

    await assertCollected(reference);
    assertInstanceAlive(wasm);
});
