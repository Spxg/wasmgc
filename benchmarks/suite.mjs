import { check, references } from "./check.mjs";

const childReads = 1024;
const nodeLength = 1024;
const pointValues = [[17, -29], [-103, 211]];
const seeds = [17, -103];
const sharedHolders = 16;
const sharedUpdates = 64;
const noop = () => {};

function nextTask() {
    return new Promise(resolve => {
        const channel = new MessageChannel();
        channel.port1.onmessage = () => {
            channel.port1.close();
            channel.port2.close();
            resolve();
        };
        channel.port2.postMessage(0);
    });
}

function sample(setup) {
    return async function* () {
        const { run, dispose = noop } = setup();
        try {
            for (let index = 0; index < 10_000; index++) run();
            await nextTask();
            yield run;
        } finally {
            dispose();
        }
    };
}

function construction(create, inputs, consume) {
    return function* () {
        let index = 0;
        yield {
            [0]() {
                return inputs[index++ & 1];
            },
            bench(input) {
                consume(create(input));
            },
        };
    };
}

function clearReferences(references, dispose) {
    for (let index = 0; index < references.length; index++) {
        if (references[index] != null) dispose(references[index]);
        references[index] = null;
    }
}

function childRead({ holder, value }, consume) {
    return function* () {
        const children = seeds.map(value.create);
        const holders = children.map(holder.create);
        const references = new Array(childReads).fill(null);

        try {
            yield {
                manual() {
                    clearReferences(references, value.dispose);
                    const start = performance.now();
                    for (let index = 0; index < childReads; index++) {
                        references[index] = holder.get(holders[index & 1]);
                    }
                    const duration = (performance.now() - start) * 1e6;
                    consume(references);
                    return duration;
                },
            };
        } finally {
            clearReferences(references, value.dispose);
            holders.forEach(holder.dispose);
            children.forEach(value.dispose);
        }
    };
}

function sharedUpdate({ holder, value }, consume) {
    return function* () {
        const shared = value.create(0);
        const holders = Array.from({ length: sharedHolders }, () => holder.create(shared));
        const references = new Array(sharedUpdates * sharedHolders).fill(null);
        let current = 0;

        try {
            yield {
                manual() {
                    clearReferences(references, value.dispose);
                    const start = performance.now();
                    let sum = 0;
                    for (let update = 0; update < sharedUpdates; update++) {
                        current = (current + 1) | 0;
                        value.set(shared, current);
                        for (let index = 0; index < holders.length; index++) {
                            const child = holder.get(holders[index]);
                            references[update * sharedHolders + index] = child;
                            sum = (sum + value.get(child)) | 0;
                        }
                    }
                    const duration = (performance.now() - start) * 1e6;
                    consume(sum);
                    return duration;
                },
            };
        } finally {
            clearReferences(references, value.dispose);
            holders.forEach(holder.dispose);
            value.dispose(shared);
        }
    };
}

function nodeTraversal(node, consume) {
    return function* () {
        const roots = seeds.map(seed => node.create(seed, nodeLength));
        const references = new Array(nodeLength).fill(null);
        let index = 0;

        try {
            yield {
                manual() {
                    clearReferences(references, node.dispose);
                    const start = performance.now();
                    let current = roots[index++ & 1];
                    let sum = 0;
                    for (let offset = 0; offset < nodeLength; offset++) {
                        sum = (sum + node.value(current)) | 0;
                        current = node.next(current);
                        references[offset] = current;
                    }
                    const duration = (performance.now() - start) * 1e6;
                    consume(sum);
                    return duration;
                },
            };
        } finally {
            clearReferences(references, node.dispose);
            roots.forEach(node.dispose);
        }
    };
}

function dataApi(api, native = false) {
    const dispose = native ? noop : object => object.free();
    return {
        point: {
            create: api.bench_point_new,
            get: native ? api["Point.get_x"] : object => object.x,
            set: native ? api["Point.set_x"] : (object, value) => { object.x = value; },
            dispose,
        },
        state: {
            create: native ? api["State.new"] : value => new api.State(value),
            get: native ? api["State.get_value"] : object => object.value,
            set: native ? api["State.set_value"] : (object, value) => { object.value = value; },
            dispose,
        },
        value: {
            create: native ? api["Value.new"] : value => new api.Value(value),
            get: native ? api["Value.get_value"] : object => object.value,
            set: native ? api["Value.set_value"] : (object, value) => { object.value = value; },
            dispose,
        },
        holder: {
            create: native ? api["Holder.new"] : child => new api.Holder(child),
            get: native ? api["Holder.get_child"] : object => object.child,
            set: native ? api["Holder.set_child"] : (object, child) => { object.child = child; },
            dispose,
        },
        node: {
            create: api.bench_node_new,
            value: native ? api["Node.get_value"] : object => object.value,
            next: native ? api["Node.get_next"] : object => object.next,
            dispose,
        },
    };
}

export function createSuite(gc, wbg, consume) {
    const data = { wasmgc: dataApi(gc, true), "wasm-bindgen": dataApi(wbg) };
    const cases = [];

    function addCase(name, group, operations, callback) {
        cases.push({
            name, group, operations,
            callbacks: Object.fromEntries(Object.entries(data).map(([name, api]) => [name, callback(api)])),
        });
    }

    function addData(name, setup) {
        addCase(name, "js", 1, api => sample(() => setup(api)));
    }

    function finish(objects, resource, observe = noop) {
        for (const object of objects) {
            observe(object);
            consume(object);
            resource.dispose(object);
        }
    }

    addCase("bench_point_new", "js", 1, ({ point }) =>
        construction(([x, y]) => point.create(x, y), pointValues, consume));

    addData("bench_point_get", ({ point }) => {
        const objects = pointValues.map(([x, y]) => point.create(x, y));
        let index = 0;
        return {
            run: () => consume(point.get(objects[index++ & 1])),
            dispose: () => finish(objects, point),
        };
    });
    addData("bench_point_set", ({ point }) => {
        const objects = pointValues.map(([x, y]) => point.create(x, y));
        let value = 0;
        return {
            run() {
                value = (value + 1) | 0;
                point.set(objects[value & 1], value);
            },
            dispose: () => finish(objects, point, object => consume(point.get(object))),
        };
    });
    addData("bench_state_get", ({ state }) => {
        const objects = references.map(state.create);
        let index = 0;
        return {
            run: () => consume(state.get(objects[index++ & 1])),
            dispose: () => finish(objects, state),
        };
    });
    addData("bench_state_set", ({ state }) => {
        const objects = references.map(state.create);
        let index = 0;
        return {
            run() {
                const slot = index++ & 1;
                state.set(objects[slot], references[(index >>> 1) & 1]);
            },
            dispose: () => finish(objects, state, object => consume(state.get(object))),
        };
    });

    addCase("bench_child_get", "objects", childReads, api => childRead(api, consume));
    addCase("bench_child_set", "objects", 1, ({ holder, value }) => sample(() => {
        const children = seeds.map(value.create);
        const holders = children.map(holder.create);
        let index = 0;
        return {
            run() {
                const slot = index++ & 1;
                holder.set(holders[slot], children[(index >>> 1) & 1]);
            },
            dispose() {
                for (const object of holders) {
                    const child = holder.get(object);
                    consume(value.get(child));
                    value.dispose(child);
                }
                finish(holders, holder);
                finish(children, value);
            },
        };
    }));
    addCase("bench_shared_update", "objects", sharedUpdates, api => sharedUpdate(api, consume));
    addCase("bench_node_walk", "objects", 1, api => nodeTraversal(api.node, consume));

    return { cases, check: () => check(gc, wbg) };
}
