// Render the Rust game through native GC references in an interactive terminal.

import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { emitKeypressEvents } from "node:readline";
import { fileURLToPath } from "node:url";

const { stdin, stdout } = process;
if (!stdin.isTTY || !stdout.isTTY) {
    throw new Error("Run this example in an interactive terminal: node examples/breakout/run.mjs");
}

const repo = fileURLToPath(new URL("../..", import.meta.url));
const target = resolve(repo, "target");
const raw = resolve(target, "wasm32-unknown-unknown/release/breakout.wasm");
const output = resolve(target, "wasm32-unknown-unknown/release/breakout.gc.wasm");

function cargo(args) {
    execFileSync("cargo", args, { cwd: repo, stdio: "inherit" });
}

cargo([
    "build", "--locked", "-p", "breakout",
    "--target", "wasm32-unknown-unknown", "--release", "--target-dir", target,
]);
cargo([
    "run", "--locked", "-p", "wasmgc", "--features", "cli", "--bin", "wasmgc", "--target-dir", target,
    "--", raw, "-o", output,
]);

const { instance } = await WebAssembly.instantiate(await readFile(output));
const wasm = instance.exports;
const width = 60;
const height = 18;
let game;
let ball;
let bricks;
let paused = false;

function reset() {
    game = wasm.create_game();
    // Cache object identities once; Rust updates their fields in place,
    // including marking bricks inactive instead of replacing them.
    ball = wasm["Game.get_ball"](game);
    const array = wasm["Game.get_bricks"](game);
    bricks = Array.from({ length: 30 }, (_, index) => wasm.brick_at(array, index));
    paused = false;
}

function render() {
    const columns = stdout.columns || 80;
    const rows = stdout.rows || 24;
    const grid_width = Math.max(12, Math.min(width, columns - 3));
    const grid_height = Math.max(6, Math.min(height, rows - 6));
    const grid = Array.from({ length: grid_height }, () => Array(grid_width).fill(" "));
    const column = x => Math.max(0, Math.min(grid_width - 1, Math.floor(x / width * grid_width)));
    const row = y => Math.max(0, Math.min(grid_height - 1, Math.floor(y / height * grid_height)));

    function draw(x, y, size, symbol) {
        const end = Math.min(grid_width, Math.ceil((x + size) / width * grid_width));
        grid[row(y)].fill(symbol, column(x), end);
    }

    let remaining = 0;
    for (const brick of bricks) {
        if (!wasm["Brick.get_alive"](brick)) continue;
        draw(wasm["Brick.get_x"](brick), wasm["Brick.get_y"](brick), 4, "#");
        remaining += 1;
    }

    draw(wasm["Game.get_paddle_x"](game), 17, 10, "=");
    grid[row(wasm["Ball.get_y"](ball))][column(wasm["Ball.get_x"](ball))] = "o";

    const status = wasm["Game.get_status"](game);
    const label = paused ? "PAUSED" : ["READY", "PLAYING", "YOU WIN", "GAME OVER"][status];
    const score = wasm["Game.get_score"](game);
    const lives = wasm["Game.get_lives"](game);
    const border = `+${"-".repeat(grid_width)}+`;
    const lines = [
        "wasmgc - breakout",
        `${label} | Score: ${score}/300 | Lives: ${lives} | Bricks: ${remaining}/30`,
        border,
        ...grid.map(cells => `|${cells.join("")}|`),
        border,
        "Left/Right or A/D: move paddle   #: brick   o: ball",
        "Space: launch/pause/resume   R: restart   Q/Esc/Ctrl+C: quit",
    ];
    stdout.write(`\x1b[H${lines.map(line => `\x1b[2K${line.slice(0, columns - 1)}`).join("\r\n")}\x1b[J`);
}

reset();
emitKeypressEvents(stdin);
const previous_raw_mode = Boolean(stdin.isRaw);
stdin.setRawMode(true);
stdout.write("\x1b[?1049h\x1b[?25l");
// Restore the caller's terminal even when quitting from an input handler.
process.once("exit", () => {
    stdin.setRawMode(previous_raw_mode);
    stdout.write("\x1b[?25h\x1b[?1049l");
});
process.once("SIGINT", () => process.exit(0));
process.once("SIGTERM", () => process.exit(0));

stdin.on("keypress", (_, key) => {
    if (key.name === "q" || key.name === "escape" || (key.ctrl && key.name === "c")) {
        process.exit(0);
    }

    switch (key.name) {
        case "left": case "a": wasm.move_paddle(game, -3); break;
        case "right": case "d": wasm.move_paddle(game, 3); break;
        case "space":
            if (wasm["Game.get_status"](game) === 0) {
                wasm.launch(game);
                paused = false;
            } else if (wasm["Game.get_status"](game) === 1) {
                paused = !paused;
            }
            break;
        case "r": reset(); break;
    }
    render();
});

stdout.on("resize", render);
let previous_time = performance.now();
setInterval(() => {
    const now = performance.now();
    const dt = Math.min((now - previous_time) / 1000, 0.05);
    previous_time = now;
    if (!paused) wasm.step(game, dt);
    render();
}, 1000 / 60);
render();
