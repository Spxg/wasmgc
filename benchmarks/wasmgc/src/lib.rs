use wasmgc::{ExternRef, Gc, wasmgc};

#[wasmgc(constructor)]
pub struct Point {
    #[getter]
    #[setter]
    x: i32,
    #[getter]
    #[setter]
    y: i32,
}

#[wasmgc]
pub struct Node {
    #[getter]
    value: i32,
    #[getter]
    next: Option<Gc<Node>>,
}

#[wasmgc(constructor)]
pub struct State {
    #[getter]
    #[setter]
    value: ExternRef,
}

#[wasmgc(constructor)]
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
}

#[wasmgc]
pub fn bench_point_new(x: i32, y: i32) -> Gc<Point> {
    Gc::new(Point { x, y })
}

#[wasmgc]
pub fn bench_node_new(seed: i32, length: u32) -> Option<Gc<Node>> {
    let mut next = None;
    for index in (0..length).rev() {
        next = Some(Gc::new(Node {
            value: seed.wrapping_add(index as i32),
            next,
        }));
    }
    next
}
