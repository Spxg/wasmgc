//! Maps schema storage types and interns generated function signatures.
//!
//! Input type indices and GC schema indices are reserved before this interner
//! starts. Signatures are emitted in insertion order; hash-map iteration never
//! determines the output order.

use super::{Input, Value};
use crate::schema::ValueTag;
use std::collections::HashMap;
use wasm_encoder::{FuncType, HeapType, RefType, StorageType, ValType};

pub(super) fn schema_index(input: &Input, schema: u32) -> u32 {
    input.types + schema
}

pub(super) fn storage_type(input: &Input, value: &Value) -> StorageType {
    match value.tag {
        ValueTag::I8 => StorageType::I8,
        ValueTag::I16 => StorageType::I16,
        ValueTag::I32
        | ValueTag::I64
        | ValueTag::F32
        | ValueTag::F64
        | ValueTag::Bool
        | ValueTag::ExternRef
        | ValueTag::Ref
        | ValueTag::NullableRef
        | ValueTag::Array
        | ValueTag::NullableArray => StorageType::Val(value_type(input, value)),
    }
}

pub(super) fn value_type(input: &Input, value: &Value) -> ValType {
    match value.tag {
        ValueTag::I8 | ValueTag::I16 | ValueTag::I32 | ValueTag::Bool => ValType::I32,
        ValueTag::I64 => ValType::I64,
        ValueTag::F32 => ValType::F32,
        ValueTag::F64 => ValType::F64,
        ValueTag::ExternRef => ValType::EXTERNREF,
        ValueTag::Ref | ValueTag::NullableRef | ValueTag::Array | ValueTag::NullableArray => {
            ValType::Ref(RefType {
                nullable: matches!(value.tag, ValueTag::NullableRef | ValueTag::NullableArray),
                heap_type: HeapType::Concrete(schema_index(
                    input,
                    value.target.expect("reference value target"),
                )),
            })
        }
    }
}

pub(super) struct FunctionTypes {
    first: u32,
    signatures: Vec<FuncType>,
    indices: HashMap<FuncType, u32>,
}

impl FunctionTypes {
    pub fn new(first: u32) -> Self {
        Self {
            first,
            signatures: Vec::new(),
            indices: HashMap::new(),
        }
    }

    pub fn intern(
        &mut self,
        params: impl IntoIterator<Item = ValType>,
        results: impl IntoIterator<Item = ValType>,
    ) -> u32 {
        let signature = FuncType::new(params, results);
        if let Some(&index) = self.indices.get(&signature) {
            return index;
        }

        let index = self.first + self.signatures.len() as u32;
        self.indices.insert(signature.clone(), index);
        self.signatures.push(signature);
        index
    }

    pub fn finish(self) -> Vec<FuncType> {
        self.signatures
    }
}
