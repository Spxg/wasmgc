import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function build() {
    const repo = fileURLToPath(new URL("../../../", import.meta.url));
    const target = resolve(repo, "target");
    const raw = resolve(target, "wasm32-unknown-unknown/release/tree.wasm");
    const wasm = resolve(target, "wasm32-unknown-unknown/release/tree.gc.wasm");
    const output = resolve(target, "site/examples/tree");

    function cargo(args) {
        execFileSync("cargo", args, { cwd: repo, stdio: "inherit" });
    }

    cargo([
        "build", "--locked", "-p", "tree",
        "--target", "wasm32-unknown-unknown", "--release", "--target-dir", target,
    ]);
    cargo([
        "run", "--locked", "-p", "wasmgc", "--features", "cli", "--bin", "wasmgc", "--target-dir", target,
        "--", raw, "-o", wasm,
    ]);

    rmSync(output, { recursive: true, force: true });
    mkdirSync(output, { recursive: true });
    for (const filename of ["index.html", "main.mjs", "style.css"]) {
        copyFileSync(new URL(filename, import.meta.url), resolve(output, filename));
    }
    copyFileSync(wasm, resolve(output, "tree.wasm"));

    return output;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
    console.log(`Built ${build()}`);
}
