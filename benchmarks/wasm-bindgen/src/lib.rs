use std::{cell::Cell, rc::Rc};

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[wasm_bindgen]
impl Point {
    #[wasm_bindgen(constructor)]
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

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
}

#[wasm_bindgen]
pub struct State {
    value: JsValue,
}

#[wasm_bindgen]
impl State {
    #[wasm_bindgen(constructor)]
    pub fn new(value: JsValue) -> Self {
        Self { value }
    }

    #[wasm_bindgen(getter)]
    pub fn value(&self) -> JsValue {
        self.value.clone()
    }

    #[wasm_bindgen(setter)]
    pub fn set_value(&mut self, value: JsValue) {
        self.value = value;
    }
}

#[wasm_bindgen]
#[derive(Clone)]
pub struct Value {
    value: Rc<Cell<i32>>,
}

#[wasm_bindgen]
impl Value {
    #[wasm_bindgen(constructor)]
    pub fn new(value: i32) -> Self {
        Self {
            value: Rc::new(Cell::new(value)),
        }
    }

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
pub struct Holder {
    child: Value,
}

#[wasm_bindgen]
impl Holder {
    #[wasm_bindgen(constructor)]
    pub fn new(child: &Value) -> Self {
        Self {
            child: child.clone(),
        }
    }

    #[wasm_bindgen(getter)]
    pub fn child(&self) -> Value {
        self.child.clone()
    }

    #[wasm_bindgen(setter)]
    pub fn set_child(&mut self, child: &Value) {
        self.child = child.clone();
    }
}

#[wasm_bindgen]
pub fn bench_point_new(x: i32, y: i32) -> Point {
    Point::new(x, y)
}

#[wasm_bindgen]
pub fn bench_node_new(seed: i32, length: u32) -> Option<Node> {
    let mut next: Option<Node> = None;
    for index in (0..length).rev() {
        next = Some(Node {
            value: seed.wrapping_add(index as i32),
            next: next.map(Rc::new),
        });
    }
    next
}
