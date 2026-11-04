use crate::schema::{OperationKind, SchemaKind, ValueTag};

pub struct Value {
    pub tag: ValueTag,
    pub target_addr: Option<u64>,
}

pub struct Schema {
    pub kind: SchemaKind,
    pub fields: Vec<Value>,
}

pub struct Operation {
    pub schema_addr: u64,
    pub kind: OperationKind,
    pub field_index: u32,
}

pub struct Struct {
    pub schema_addr: u64,
    pub kind: OperationKind,
    pub field_index: u32,
    pub name: String,
    pub function_index: u32,
}

pub struct FunctionExport {
    pub original_function_index: u32,
    pub function_index: u32,
    pub params: Vec<Value>,
    pub result: Option<Value>,
    pub name: String,
}

pub struct FunctionImport {
    pub original_function_index: u32,
    pub function_index: u32,
    pub type_index: u32,
    pub params: Vec<Value>,
    pub result: Option<Value>,
    pub module: String,
    pub name: String,
}
