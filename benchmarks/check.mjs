export const references = [{ value: 17 }, { value: -29 }];
const referenceValues = [null, undefined, ...references, 17, 'value'];

function equal(actual, expected, context) {
    if (!Object.is(actual, expected)) {
        throw new Error(`${context}: expected ${String(expected)}, got ${String(actual)}`);
    }
}

function bindgenObjects(api) {
    return {
        getX: point => point.x,
        getY: point => point.y,
        setX(point, value) { point.x = value; },
        setY(point, value) { point.y = value; },
        state: value => new api.State(value),
        getRef: state => state.value,
        setRef(state, value) { state.value = value; },
        nodeValue: node => node.value,
        nodeNext: node => node.next,
        value: value => new api.Value(value),
        getValue: object => object.value,
        setValue(object, value) { object.value = value; },
        holder: child => new api.Holder(child),
        getChild: holder => holder.child,
        setChild(holder, child) { holder.child = child; },
    };
}

function gcObjects(gc) {
    return {
        getX: gc['Point.get_x'],
        getY: gc['Point.get_y'],
        setX: gc['Point.set_x'],
        setY: gc['Point.set_y'],
        state: gc['State.new'],
        getRef: gc['State.get_value'],
        setRef: gc['State.set_value'],
        nodeValue: gc['Node.get_value'],
        nodeNext: gc['Node.get_next'],
        value: gc['Value.new'],
        getValue: gc['Value.get_value'],
        setValue: gc['Value.set_value'],
        holder: gc['Holder.new'],
        getChild: gc['Holder.get_child'],
        setChild: gc['Holder.set_child'],
    };
}

function checkChildren(api) {
    const same = (actual, expected, operation) => equal(actual, expected, `${api.name}: ${operation}`);
    const children = [api.value(17), api.value(-103)];
    const holders = [];
    const aliases = [];
    try {
        holders.push(api.holder(children[0]), api.holder(children[0]));
        const left = api.getChild(holders[0]);
        aliases.push(left);
        const right = api.getChild(holders[1]);
        aliases.push(right);
        same(api.getValue(left), 17, 'Holder constructor preserves child');

        for (const value of [-0x80000000, -1, 0, 1, 0x7fffffff]) {
            api.setValue(left, value);
            same(api.getValue(right), value, 'Holder aliases share mutations');
            same(api.getValue(children[0]), value, 'original child remains shared');
        }

        api.setChild(holders[0], children[1]);
        const replacement = api.getChild(holders[0]);
        aliases.push(replacement);
        same(api.getValue(replacement), -103, 'Holder setter replaces child');
        api.setValue(replacement, 42);
        same(api.getValue(children[1]), 42, 'replacement child remains shared');
        same(api.getValue(right), 0x7fffffff, 'other Holder retains original child');

        api.setValue(left, -29);
        const original = api.getChild(holders[1]);
        aliases.push(original);
        same(api.getValue(original), -29, 'retained alias still updates original child');
        same(api.getValue(replacement), 42, 'replacement is isolated from original');

        while (holders.length) api.dispose(holders.pop());
        while (children.length) api.dispose(children.pop());
        api.setValue(left, 211);
        same(api.getValue(right), 211, 'child aliases survive Holder and input disposal');
        same(api.getValue(replacement), 42, 'replacement survives Holder disposal');
    } finally {
        aliases.reverse().forEach(api.dispose);
        holders.forEach(api.dispose);
        children.forEach(api.dispose);
    }
}

function checkNode(api) {
    const { functions } = api;
    const same = (actual, expected, operation) => equal(actual, expected, `${api.name}: ${operation}`);
    for (const seed of [17, -103, 0x7fffffff]) {
        for (const length of [0, 1, 3, 1024]) {
            const root = functions.bench_node_new(seed, length);
            let node = root;
            let count = 0;
            try {
                while (node != null) {
                    same(count < length, true, 'Node list ends within its length');
                    same(api.nodeValue(node), (seed + count) | 0, `Node value [${count}]`);
                    const next = api.nodeNext(node);
                    if (node !== root) api.dispose(node);
                    node = next;
                    count++;
                }
                same(count, length, 'Node list length');
            } finally {
                if (node != null && node !== root) api.dispose(node);
                if (root != null) api.dispose(root);
            }
        }
    }

    let root = functions.bench_node_new(17, 3);
    const retained = [];
    try {
        const child = api.nodeNext(root);
        retained.push(child);
        api.dispose(root);
        root = null;
        same(api.nodeValue(child), 18, 'retained child survives root disposal');
        const tail = api.nodeNext(child);
        retained.push(tail);
        same(api.nodeValue(tail), 19, 'retained child preserves its next node');
        const end = api.nodeNext(tail);
        if (end != null) retained.push(end);
        same(end == null, true, 'retained list ends');
    } finally {
        retained.reverse().forEach(node => {
            if (node != null) api.dispose(node);
        });
        if (root != null) api.dispose(root);
    }
}

export function check(gc, wbg) {
    const implementations = [
        { name: 'wasmgc', functions: gc, ...gcObjects(gc), dispose() {} },
        { name: 'wasm-bindgen', functions: wbg, ...bindgenObjects(wbg), dispose: value => value.free() },
    ];

    for (const api of implementations) {
        const { functions, dispose } = api;
        const same = (actual, expected, operation) => equal(actual, expected, `${api.name}: ${operation}`);
        const points = [functions.bench_point_new(17, -29), functions.bench_point_new(-103, 211)];
        try {
            same(api.getX(points[0]), 17, 'Point.x');
            same(api.getY(points[0]), -29, 'Point.y');
            api.setX(points[0], -0x80000000);
            same(api.getX(points[0]), -0x80000000, 'Point.set_x');
            same(api.getY(points[0]), -29, 'Point field isolation');
            api.setY(points[0], 0x7fffffff);
            same(api.getY(points[0]), 0x7fffffff, 'Point.set_y');
            same(api.getX(points[0]), -0x80000000, 'Point field isolation');
            same(api.getX(points[1]), -103, 'Point object isolation');
            same(api.getY(points[1]), 211, 'Point object isolation');
        } finally {
            points.forEach(dispose);
        }
        checkChildren(api);
        checkNode(api);

        const states = references.map(value => api.state(value));
        try {
            same(api.getRef(states[0]), references[0], 'State constructor identity');
            for (const value of referenceValues) {
                api.setRef(states[0], value);
                same(api.getRef(states[0]), value, 'State reference identity');
                same(api.getRef(states[1]), references[1], 'State object isolation');
            }
        } finally {
            states.forEach(dispose);
        }
    }
}
