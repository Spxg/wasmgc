function compare(description, wasmgc, wbg, note) {
    return {
        description,
        note,
        implementations: {
            wasmgc,
            "wasm-bindgen": wbg,
        },
    };
}

const gcPoint = `#[wasmgc(constructor)]
pub struct Point {
    #[getter]
    #[setter]
    x: i32,
    #[getter]
    #[setter]
    y: i32,
}`;

const wbgPoint = `#[wasm_bindgen]
pub struct Point {
    pub x: i32,
    pub y: i32,
}`;

const gcState = `#[wasmgc(constructor)]
pub struct State {
    #[getter]
    #[setter]
    value: ExternRef,
}`;

const wbgState = `#[wasm_bindgen]
pub struct State {
    value: JsValue,
}`;

const gcHolder = `#[wasmgc(constructor)]
pub struct Value {
    #[getter]
    #[setter]
    value: i32,
}

#[wasmgc(constructor)]
pub struct Holder {
    #[getter]
    #[setter]
    child: Gc<Value>,
}`;

const wbgHolder = `use std::{cell::Cell, rc::Rc};

#[wasm_bindgen]
#[derive(Clone)]
pub struct Value {
    value: Rc<Cell<i32>>,
}

#[wasm_bindgen]
pub struct Holder {
    child: Value,
}`;

const examples = {
    bench_point_new: compare(
        "Create a Point with two i32 fields and return it to JS.",
        `${gcPoint}

#[wasmgc]
pub fn bench_point_new(x: i32, y: i32) -> Gc<Point> {
    Gc::new(Point { x, y })
}`,
        `${wbgPoint}

#[wasm_bindgen]
impl Point {
    #[wasm_bindgen(constructor)]
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[wasm_bindgen]
pub fn bench_point_new(x: i32, y: i32) -> Point {
    Point::new(x, y)
}`,
    ),
    bench_point_get: compare(
        "Read Point.x from JS.",
        gcPoint,
        wbgPoint,
    ),
    bench_point_set: compare(
        "Write Point.x from JS.",
        gcPoint,
        wbgPoint,
    ),
    bench_state_get: compare(
        "Read an existing JS object reference from State.value.",
        gcState,
        `${wbgState}

#[wasm_bindgen]
impl State {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> JsValue {
        self.value.clone()
    }
}`,
    ),
    bench_state_set: compare(
        "Store an existing JS object reference in State.value.",
        gcState,
        `${wbgState}

#[wasm_bindgen]
impl State {
    #[wasm_bindgen(setter)]
    pub fn set_value(&mut self, value: JsValue) {
        self.value = value;
    }
}`,
    ),
    bench_child_get: compare(
        "Read an existing shared child reference from Holder.child.",
        gcHolder,
        `${wbgHolder}

#[wasm_bindgen]
impl Holder {
    #[wasm_bindgen(getter)]
    pub fn child(&self) -> Value {
        self.child.clone()
    }
}`,
        "wasmgc returns the existing child; wasm-bindgen clones its Rc and creates a JS wrapper.",
    ),
    bench_child_set: compare(
        "Replace Holder.child with one of two existing shared children.",
        gcHolder,
        `${wbgHolder}

#[wasm_bindgen]
impl Holder {
    #[wasm_bindgen(setter)]
    pub fn set_child(&mut self, child: &Value) {
        self.child = child.clone();
    }
}`,
        "Both implementations retain the shared child; replacing it does not copy its value.",
    ),
    bench_shared_update: compare(
        "Update one shared Value, then sum its value through 16 Holders in JS.",
        gcHolder,
        `${wbgHolder}

#[wasm_bindgen]
impl Value {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> i32 {
        self.value.get()
    }

    #[wasm_bindgen(setter)]
    pub fn set_value(&self, value: i32) {
        self.value.set(value);
    }
}

#[wasm_bindgen]
impl Holder {
    #[wasm_bindgen(getter)]
    pub fn child(&self) -> Value {
        self.child.clone()
    }
}`,
        "All 16 holders share one Value. Each result covers one write, 16 child reads and 16 value reads.",
    ),
    bench_node_walk: compare(
        "Follow next through 1,024 nodes in JS and sum their value fields.",
        `use wasmgc::{Gc, wasmgc};

#[wasmgc]
pub struct Node {
    #[getter]
    value: i32,
    #[getter]
    next: Option<Gc<Node>>,
}`,
        `use std::rc::Rc;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
#[derive(Clone)]
pub struct Node {
    value: i32,
    next: Option<Rc<Node>>,
}

#[wasm_bindgen]
impl Node {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> i32 {
        self.value
    }

    #[wasm_bindgen(getter)]
    pub fn next(&self) -> Option<Node> {
        self.next.as_deref().cloned()
    }
}`,
        "wasmgc returns existing nodes; wasm-bindgen shallow-clones each next node and creates a JS wrapper, sharing its Rc link.",
    ),
};

export function exampleFor(test) {
    return examples[test.name];
}
