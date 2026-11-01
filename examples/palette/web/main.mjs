const width = 96;
const height = 64;
const channels = ["r", "g", "b"];
const canvasElement = document.querySelector("#canvas");
const controls = document.querySelector("#controls");
const resetButton = document.querySelector("#reset");
const cellSize = document.querySelector("#cell-size");
const status = document.querySelector("#status");
const picker = document.querySelector("#picker");
const paletteButtons = [...document.querySelectorAll("[data-color]")];
const sliders = channels.map(channel => document.querySelector(`#${channel}`));

canvasElement.style.setProperty("--columns", width);
cellSize.addEventListener("input", () => {
    canvasElement.style.setProperty("--cell-size", `${cellSize.value}px`);
    document.querySelector("#cell-size-value").value = `${cellSize.value} px`;
});

async function start() {
    const response = await fetch(new URL("./palette.wasm", import.meta.url));
    if (!response.ok) {
        throw new Error(`Could not load palette.wasm (HTTP ${response.status}).`);
    }

    const { instance } = await WebAssembly.instantiate(await response.arrayBuffer());
    const wasm = instance.exports;
    const canvas = wasm.create_canvas(width, height);
    const cells = Array.from({ length: width * height }, (_, index) => wasm.color_at(canvas, index));
    // These are the actual GC objects returned by Rust, also held by the canvas.
    const colors = cells.slice(0, 3);
    const initialColors = colors.map(rgb);
    let selected = 0;
    let focusedCell = 0;

    function rgb(color) {
        return channels.map(channel => wasm[`Color.get_${channel}`](color));
    }

    function setRgb(color, values) {
        channels.forEach((channel, index) => wasm[`Color.set_${channel}`](color, values[index]));
    }

    const tiles = cells.map((color, index) => {
        const tile = document.createElement("button");
        tile.type = "button";
        tile.className = "cell";
        tile.tabIndex = index === 0 ? 0 : -1;
        tile.addEventListener("click", () => {
            focusCell(index);
            selected = colors.indexOf(color);
            render();
        });
        return tile;
    });
    canvasElement.replaceChildren(...tiles);

    function focusCell(index) {
        tiles[focusedCell].tabIndex = -1;
        focusedCell = index;
        tiles[index].tabIndex = 0;
    }

    canvasElement.addEventListener("keydown", event => {
        let index = focusedCell;
        switch (event.key) {
            case "ArrowRight": index = Math.min(cells.length - 1, index + 1); break;
            case "ArrowLeft": index = Math.max(0, index - 1); break;
            case "ArrowDown": index = Math.min(cells.length - 1, index + width); break;
            case "ArrowUp": index = Math.max(0, index - width); break;
            case "Home": index -= index % width; break;
            case "End": index += width - 1 - index % width; break;
            default: return;
        }
        event.preventDefault();
        focusCell(index);
        tiles[index].focus();
    });

    function render() {
        // Read back from the GC objects; there is no separate JS color model.
        const palette = colors.map(rgb);
        const values = palette[selected];
        const hex = `#${values.map(value => value.toString(16).padStart(2, "0")).join("")}`;
        document.querySelector("#editor-title").textContent = `Color ${selected + 1}`;
        document.querySelector("#hex").value = hex.toUpperCase();
        picker.value = hex;

        paletteButtons.forEach((button, index) => {
            button.style.setProperty("--color", `rgb(${palette[index].join(",")})`);
            button.setAttribute("aria-pressed", String(index === selected));
        });
        tiles.forEach((tile, index) => {
            const color = cells[index];
            const colorIndex = colors.indexOf(color);
            const [r, g, b] = palette[colorIndex];
            tile.style.setProperty("--color", `rgb(${r},${g},${b})`);
            tile.setAttribute("aria-pressed", String(color === colors[selected]));
            tile.setAttribute("aria-label", `Tile ${index + 1}, color ${colorIndex + 1}, RGB ${r}, ${g}, ${b}`);
        });
        sliders.forEach((slider, index) => {
            slider.value = values[index];
            document.querySelector(`#${channels[index]}-value`).value = values[index];
        });
    }

    paletteButtons.forEach((button, index) => button.addEventListener("click", () => {
        selected = index;
        render();
    }));
    sliders.forEach((slider, index) => slider.addEventListener("input", () => {
        wasm[`Color.set_${channels[index]}`](colors[selected], Number(slider.value));
        render();
    }));
    picker.addEventListener("input", () => {
        setRgb(colors[selected], channels.map((_, index) => parseInt(picker.value.slice(1 + index * 2, 3 + index * 2), 16)));
        render();
    });
    resetButton.addEventListener("click", () => {
        colors.forEach((color, index) => setRgb(color, initialColors[index]));
        selected = 0;
        render();
    });
    render();
    controls.disabled = false;
    resetButton.disabled = false;
    canvasElement.setAttribute("aria-busy", "false");
    status.dataset.state = "ready";
    status.textContent = "Ready";
    status.hidden = true;
}

start().catch(error => {
    status.dataset.state = "error";
    status.textContent = "Could not start";
    canvasElement.setAttribute("aria-busy", "false");
    const message = document.querySelector("#error");
    message.textContent = error.message;
    message.hidden = false;
    console.error(error);
});
