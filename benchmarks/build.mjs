import { execFileSync } from "node:child_process";
import { copyFileSync, cpSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const directory = fileURLToPath(new URL(".", import.meta.url));
const repository = fileURLToPath(new URL("..", import.meta.url));

export function build() {
    const target = join(repository, "target", "benchmarks", "rust");
    const artifacts = join(target, "wasm32-unknown-unknown", "release");
    const output = join(repository, "target", "site", "benchmarks");

    function execute(command, args, env = process.env) {
        execFileSync(command, args, { cwd: repository, env, stdio: "inherit" });
    }

    execute("cargo", [
        "build", "--manifest-path", join(directory, "Cargo.toml"), "--workspace",
        "--target", "wasm32-unknown-unknown", "--release", "--locked", "--target-dir", target,
    ], { ...process.env, RUSTFLAGS: "-C target-feature=+reference-types" });

    execute("cargo", [
        "build", "--manifest-path", join(repository, "Cargo.toml"), "-p", "wasmgc",
        "--features", "cli", "--bin", "wasmgc", "--release", "--locked", "--target-dir", target,
    ]);

    rmSync(output, { recursive: true, force: true });
    mkdirSync(output, { recursive: true });
    execute(join(target, "release", "wasmgc"), [
        join(artifacts, "benchmark_wasmgc.wasm"), "-o", join(output, "wasmgc.wasm"),
    ]);
    execute("wasm-bindgen", [
        join(artifacts, "benchmark_wasm_bindgen.wasm"), "--target", "web",
        "--out-dir", join(output, "wasm-bindgen"), "--out-name", "wasm_bindgen", "--no-typescript",
    ]);
    for (const filename of ["wasmgc.wasm", "wasm-bindgen/wasm_bindgen_bg.wasm"]) {
        const wasm = join(output, filename);
        execute("wasm-opt", [wasm, "-O3", "--all-features", "-o", wasm]);
    }

    for (const filename of [
        "index.html", "main.mjs", "worker.mjs", "style.css", "suite.mjs", "check.mjs",
        "examples.mjs", "stats.mjs",
    ]) {
        copyFileSync(join(directory, filename), join(output, filename));
    }
    cpSync(join(directory, "node_modules", "mitata", "src"), join(output, "mitata"), { recursive: true });
    copyFileSync(join(directory, "node_modules", "mitata", "LICENSE"), join(output, "mitata", "LICENSE"));

    mkdirSync(join(output, "prism"));
    for (const [source, filename] of [
        ["components/prism-core.min.js", "core.js"],
        ["components/prism-rust.min.js", "rust.js"],
        ["themes/prism.css", "prism.css"],
        ["LICENSE", "LICENSE"],
    ]) {
        copyFileSync(join(directory, "node_modules", "prismjs", source), join(output, "prism", filename));
    }

    return output;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
    console.log(`Built ${build()}`);
}
