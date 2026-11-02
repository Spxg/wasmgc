const svgNamespace = "http://www.w3.org/2000/svg";
const previousTree = document.querySelector("#previous-tree");
const currentTree = document.querySelector("#current-tree");
const editor = document.querySelector("#editor");
const valueInput = document.querySelector("#value");
const status = document.querySelector("#status");

function svgElement(tag, attributes, text) {
    const element = document.createElementNS(svgNamespace, tag);
    for (const [name, value] of Object.entries(attributes)) {
        element.setAttribute(name, value);
    }
    if (text !== undefined) element.textContent = text;
    return element;
}

function position(index) {
    const depth = Math.floor(Math.log2(index));
    const count = 2 ** depth;
    return { x: (index - count + 0.5) * 400 / count, y: 32 + depth * 72 };
}

async function start() {
    const response = await fetch(new URL("./tree.wasm", import.meta.url));
    if (!response.ok) {
        throw new Error(`Could not load tree.wasm (HTTP ${response.status}).`);
    }

    const { instance } = await WebAssembly.instantiate(await response.arrayBuffer());
    const wasm = instance.exports;
    const identities = new WeakMap();
    const discarded = [];
    let nextIdentity = 1;
    let previousRoot = null;
    let currentRoot = wasm.create_tree();
    let selectedIndex = 8;
    let editingIndex = null;

    function identity(object) {
        if (!identities.has(object)) identities.set(object, nextIdentity++);
        return identities.get(object);
    }

    function readTree(root, index = 1, nodes = []) {
        if (root === null) return nodes;
        nodes.push({ object: root, index, value: wasm["Node.get_value"](root) });
        readTree(wasm["Node.get_left"](root), index * 2, nodes);
        readTree(wasm["Node.get_right"](root), index * 2 + 1, nodes);
        return nodes;
    }

    function appendVersion(root) {
        const candidates = new Set();
        for (const { object } of readTree(previousRoot)) candidates.add(object);

        previousRoot = currentRoot;
        currentRoot = root;

        const retained = new Set();
        for (const saved of [previousRoot, currentRoot]) {
            for (const { object } of readTree(saved)) retained.add(object);
        }
        for (const object of candidates) {
            if (retained.has(object)) continue;
            discarded.push({
                identity: identity(object),
                value: wasm["Node.get_value"](object),
                reference: new WeakRef(object),
                collected: false,
            });
        }
        renderDiscarded();
    }

    function renderDiscarded() {
        const fragment = document.createDocumentFragment();
        let collected = 0;
        for (const record of discarded) {
            const item = document.createElement("li");
            item.dataset.identity = record.identity;
            item.dataset.value = record.value;
            item.dataset.state = record.collected ? "collected" : "waiting";

            const label = document.createElement("strong");
            label.textContent = `#${record.identity}`;
            const value = document.createElement("span");
            value.textContent = `Value ${record.value}`;
            const state = document.createElement("span");
            state.className = "gc-state";
            state.textContent = record.collected ? "Collected" : "Waiting for GC";
            item.append(label, value, state);
            fragment.append(item);
            if (record.collected) collected++;
        }

        document.querySelector("#discarded-nodes").replaceChildren(fragment);
        document.querySelector("#gc-empty").hidden = discarded.length > 0;
        const summary = document.querySelector("#gc-summary");
        summary.dataset.waiting = discarded.length - collected;
        summary.dataset.collected = collected;
        summary.textContent = `${discarded.length - collected} waiting for GC · ${collected} collected`;
    }

    function checkDiscarded() {
        let changed = false;
        for (const record of discarded) {
            if (!record.collected && record.reference.deref() === undefined) {
                record.collected = true;
                changed = true;
            }
        }
        if (changed) renderDiscarded();
    }

    function drawTree(element, nodes, shared, current, comparing) {
        const fragment = document.createDocumentFragment();
        for (const { index } of nodes) {
            if (index === 1) continue;
            const parent = position(Math.floor(index / 2));
            const child = position(index);
            fragment.append(svgElement("line", {
                x1: parent.x,
                y1: parent.y + 22,
                x2: child.x,
                y2: child.y - 22,
                class: "edge",
            }));
        }

        for (const { object, index, value } of nodes) {
            const { x, y } = position(index);
            const number = identity(object);
            const kind = comparing ? (shared.has(object) ? "shared" : current ? "new" : "old") : "";
            const selected = current && index === selectedIndex;
            const node = svgElement("g", {
                class: `node ${kind}${selected ? " selected" : ""}`,
                transform: `translate(${x}, ${y})`,
            });
            if (current) {
                node.dataset.index = index;
                node.setAttribute("role", "button");
                node.setAttribute("tabindex", "0");
                node.setAttribute("aria-pressed", selected);
                node.setAttribute("aria-label", `Node ${index}, value ${value}, object #${number}${kind ? `, ${kind}` : ""}`);
            }

            node.append(svgElement("title", {}, `Node ${index}: value ${value}, object #${number}`));
            node.append(svgElement("rect", { x: -22, y: -22, width: 44, height: 44, rx: 2 }));
            const label = svgElement("text", { x: 0, y: -1, class: "value" }, value);
            if (String(value).length > 3) {
                label.setAttribute("textLength", "36");
                label.setAttribute("lengthAdjust", "spacingAndGlyphs");
            }
            node.append(label);
            node.append(svgElement("text", { x: 0, y: 15, class: "identity" }, `#${number}`));
            fragment.append(node);
        }

        if (nodes.length === 0) {
            fragment.append(svgElement("text", {
                x: 200, y: 140, class: "empty",
            }, "No previous version"));
        }
        element.replaceChildren(fragment);
    }

    function render() {
        cancelEdit();
        const current = readTree(currentRoot);
        const previous = readTree(previousRoot);
        const previousObjects = new Set(previous.map(node => node.object));
        const shared = new Set(current.filter(node => previousObjects.has(node.object)).map(node => node.object));
        drawTree(previousTree, previous, shared, false, previous.length > 0);
        drawTree(currentTree, current, shared, true, previous.length > 0);

        document.querySelector("#counts").textContent = previous.length > 0
            ? `${current.length} nodes · ${shared.size} shared · ${current.length - shared.size} new`
            : `${current.length} nodes · no previous version`;

        const referenced = new Set();
        for (const root of [previousRoot, currentRoot]) {
            for (const { object } of readTree(root)) referenced.add(object);
        }
        document.querySelector("#object-count").textContent = `Objects referenced by these versions: ${referenced.size}`;
    }

    function cancelEdit(restoreFocus = false) {
        const index = editingIndex;
        editingIndex = null;
        editor.hidden = true;
        delete editor.dataset.index;
        if (restoreFocus && index !== null) {
            currentTree.querySelector(`[data-index="${index}"]`).focus();
        }
    }

    function positionEditor() {
        const node = currentTree.querySelector(`[data-index="${editingIndex}"]`).getBoundingClientRect();
        const tree = editor.parentElement.getBoundingClientRect();
        const left = Math.max(8, Math.min(
            node.left + node.width / 2 - tree.left - editor.offsetWidth / 2,
            tree.width - editor.offsetWidth - 8,
        ));
        editor.style.left = `${left}px`;
        editor.style.top = `${node.bottom - tree.top + 6}px`;
    }

    function editNode(index) {
        selectedIndex = index;
        render();
        editingIndex = index;
        editor.dataset.index = index;
        document.querySelector("#editor-label").textContent = `Node ${index} value`;
        valueInput.value = readTree(currentRoot).find(node => node.index === index).value;
        editor.hidden = false;
        positionEditor();
        valueInput.focus();
        valueInput.select();
        editor.scrollIntoView({ block: "nearest" });
    }

    currentTree.addEventListener("click", event => {
        const node = event.target.closest("[data-index]");
        if (node) editNode(Number(node.dataset.index));
    });
    currentTree.addEventListener("keydown", event => {
        if (event.key !== "Enter" && event.key !== " ") return;
        const node = event.target.closest("[data-index]");
        if (!node) return;
        event.preventDefault();
        editNode(Number(node.dataset.index));
    });

    editor.addEventListener("submit", event => {
        event.preventDefault();
        const index = editingIndex;
        const value = Number(valueInput.value);
        const currentValue = readTree(currentRoot).find(node => node.index === index).value;
        if (value === currentValue) {
            cancelEdit(true);
            return;
        }

        const root = wasm.edit_tree(currentRoot, index, value);
        appendVersion(root);
        render();
        currentTree.querySelector(`[data-index="${index}"]`).focus();
    });
    editor.addEventListener("keydown", event => {
        if (event.key !== "Escape") return;
        event.preventDefault();
        cancelEdit(true);
    });
    document.querySelector("#cancel-edit").addEventListener("click", () => {
        cancelEdit(true);
    });
    window.addEventListener("resize", () => {
        if (editingIndex !== null) positionEditor();
    });

    render();
    renderDiscarded();
    setInterval(checkDiscarded, 1000);
    status.dataset.state = "ready";
    status.textContent = "Ready";
    status.hidden = true;
}

start().catch(error => {
    status.dataset.state = "error";
    status.textContent = "Could not start";
    const message = document.querySelector("#error");
    message.textContent = error.message;
    message.hidden = false;
    console.error(error);
});
