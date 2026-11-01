import { execFileSync } from "node:child_process";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

export function build() {
    const repo = fileURLToPath(new URL("../..", import.meta.url));
    const target = resolve(repo, "target");
    const raw = resolve(target, "wasm32-unknown-unknown/release/palette.wasm");
    const output = resolve(target, "wasm32-unknown-unknown/release/palette.gc.wasm");

    function cargo(args) {
        execFileSync("cargo", args, { cwd: repo, stdio: "inherit" });
    }

    cargo([
        "build", "--locked", "-p", "palette",
        "--target", "wasm32-unknown-unknown", "--release", "--target-dir", target,
    ]);
    cargo([
        "run", "--locked", "-p", "wasmgc", "--features", "cli", "--bin", "wasmgc", "--target-dir", target,
        "--", raw, "-o", output,
    ]);

    return output;
}
