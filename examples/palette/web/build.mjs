import { copyFileSync, mkdirSync, rmSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { build as buildWasm } from "../build.mjs";

export function build() {
    const wasm = buildWasm();
    const output = fileURLToPath(new URL("../../../target/site/examples/palette/", import.meta.url));

    rmSync(output, { recursive: true, force: true });
    mkdirSync(output, { recursive: true });
    for (const filename of ["index.html", "main.mjs", "style.css"]) {
        copyFileSync(new URL(filename, import.meta.url), join(output, filename));
    }
    copyFileSync(wasm, join(output, "palette.wasm"));

    return output;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
    console.log(`Built ${build()}`);
}
