//! Registers generated functions for the function and code sections.
//!
//! Definitions receive indices in insertion order. Each entry contains both its
//! signature index and its complete body, so both sections use the same order.

use wasm_encoder::Function;

pub(super) struct Definition {
    pub ty: u32,
    pub body: Function,
}

pub(super) struct FunctionRegistry {
    first: u32,
    definitions: Vec<Definition>,
}

impl FunctionRegistry {
    pub fn new(first: u32) -> Self {
        Self {
            first,
            definitions: Vec::new(),
        }
    }

    pub fn push(&mut self, ty: u32, body: Function) -> u32 {
        let index = self.first + self.definitions.len() as u32;
        self.definitions.push(Definition { ty, body });
        index
    }

    pub fn finish(self) -> Vec<Definition> {
        self.definitions
    }
}
