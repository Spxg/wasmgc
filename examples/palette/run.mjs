// Edit three shared WasmGC colors and render their canvas references in a terminal.

import { readFile } from "node:fs/promises";
import { emitKeypressEvents } from "node:readline";
import { build } from "./build.mjs";

const { stdin, stdout } = process;
if (!stdin.isTTY || !stdout.isTTY) {
    throw new Error("Run this example in an interactive terminal: node examples/palette/run.mjs");
}

const { instance } = await WebAssembly.instantiate(await readFile(build()));
const wasm = instance.exports;
const width = 12;
const height = 6;
const channels = ["r", "g", "b"];
let canvas;
let cells;
let colors;
let selected = 0;
let channel = 0;

function reset() {
    canvas = wasm.create_canvas(width, height);
    cells = Array.from({ length: width * height }, (_, index) => wasm.color_at(canvas, index));
    // Each palette entry is taken from the canvas itself, so the controls
    // edit the objects already referenced by all matching cells.
    colors = cells.slice(0, 3);
    selected = 0;
    channel = 0;
}

function rgb(color) {
    return channels.map(channel => wasm[`Color.get_${channel}`](color));
}

function block(color, text) {
    return `\x1b[48;2;${rgb(color).join(";")}m${text}\x1b[0m`;
}

function adjust(amount) {
    const color = colors[selected];
    const name = channels[channel];
    const value = wasm[`Color.get_${name}`](color);
    wasm[`Color.set_${name}`](color, Math.max(0, Math.min(255, value + amount)));
}

function render() {
    const cell_width = Math.max(1, Math.min(4, Math.floor(((stdout.columns || 80) - 2) / width)));
    const border = `+${"-".repeat(width * cell_width)}+`;
    const lines = [
        "wasmgc - shared color palette",
        "3 Color objects, 72 canvas references. Edit one color; all its cells change.",
        ...colors.map((color, index) =>
            `${index === selected ? ">" : " "} ${index + 1} ${block(color, "    ")} RGB (${rgb(color).join(", ")})`),
        `Editing color ${selected + 1}, channel ${channels[channel].toUpperCase()}`,
        border,
    ];

    for (let y = 0; y < height; y++) {
        const row = cells.slice(y * width, (y + 1) * width);
        lines.push(`|${row.map(color => block(color, " ".repeat(cell_width))).join("")}|`);
    }

    lines.push(
        border,
        "1/2/3: select color   Tab: select R/G/B   Left/Right: adjust by 16",
        "R: reset   Q/Esc/Ctrl+C: quit",
    );
    stdout.write(`\x1b[H${lines.map(line => `\x1b[2K${line}`).join("\r\n")}\x1b[J`);
}

reset();
emitKeypressEvents(stdin);
const previous_raw_mode = Boolean(stdin.isRaw);
stdin.setRawMode(true);
stdout.write("\x1b[?1049h\x1b[?25l");
// Restore the caller's terminal even when quitting from an input handler.
process.once("exit", () => {
    stdin.setRawMode(previous_raw_mode);
    stdout.write("\x1b[0m\x1b[?25h\x1b[?1049l");
});
process.once("SIGINT", () => process.exit(0));
process.once("SIGTERM", () => process.exit(0));

stdin.on("keypress", (_, key) => {
    if (key.name === "q" || key.name === "escape" || (key.ctrl && key.name === "c")) {
        process.exit(0);
    }

    switch (key.name) {
        case "1": case "2": case "3": selected = Number(key.name) - 1; break;
        case "tab": channel = (channel + 1) % channels.length; break;
        case "left": adjust(-16); break;
        case "right": adjust(16); break;
        case "r": reset(); break;
    }
    render();
});

stdout.on("resize", render);
render();
