//! Emits conversions between native external references and owned Rust handles.
//!
//! External references use a separate table from struct and array roots. Null
//! is an ordinary stored external value and receives a nonzero handle too.
//! Native-only accessors need no conversion; these helpers serve operations
//! that cross Rust's numeric ABI.

use super::{Input, function::FunctionRegistry, root::RootTable, types::FunctionTypes};
use crate::schema::ValueTag;
use std::collections::BTreeMap;
use wasm_encoder::{
    ConstExpr, Function, GlobalType, HeapType, Instruction as I, TableType, ValType,
};

pub(super) struct ExternRef {
    enabled: bool,
    new: u32,
    release: u32,
    table: RootTable,
}

impl ExternRef {
    pub fn new(
        input: &Input,
        table: u32,
        first_global: u32,
        types: &mut FunctionTypes,
        functions: &mut FunctionRegistry,
        exports: &mut BTreeMap<String, u32>,
    ) -> Self {
        let enabled = input
            .imports
            .iter()
            .filter_map(|(name, _)| name.as_deref())
            .any(|name| name.starts_with("externref_"))
            || input
                .operations
                .values()
                .map(|operation| &input.schemas[operation.schema as usize])
                .flat_map(|schema| &schema.fields)
                .any(|field| field.tag == ValueTag::ExternRef)
            || input
                .bridges
                .iter()
                .flat_map(|bridge| bridge.params.iter().chain(&bridge.result))
                .any(|value| value.tag == ValueTag::ExternRef);

        let table = RootTable::new(table, first_global, HeapType::EXTERN);
        if !enabled {
            return Self {
                enabled,
                new: 0,
                release: 0,
                table,
            };
        }

        let mut new = Function::new([(1, ValType::I32)]);

        table.allocate(&mut new, 1);

        for instruction in [
            I::LocalGet(1),
            I::LocalGet(0),
            I::TableSet(table.index),
            I::LocalGet(1),
            I::End,
        ] {
            new.instruction(&instruction);
        }

        let new = functions.push(types.intern([ValType::EXTERNREF], [ValType::I32]), new);
        exports.insert("wasmgc.externref_new".to_owned(), new);

        let mut release = Function::new([]);

        table.release(&mut release, 0);
        release.instruction(&I::End);

        let release = functions.push(types.intern([ValType::I32], []), release);

        // Keep the reference live in a local while its slot is released.
        let mut take = Function::new([(1, ValType::EXTERNREF)]);

        take.instruction(&I::LocalGet(0))
            .instruction(&I::TableGet(table.index))
            .instruction(&I::LocalSet(1))
            .instruction(&I::LocalGet(0))
            .instruction(&I::Call(release))
            .instruction(&I::LocalGet(1))
            .instruction(&I::End);

        let take = functions.push(types.intern([ValType::I32], [ValType::EXTERNREF]), take);
        exports.insert("wasmgc.externref_take".to_owned(), take);

        let mut get = Function::new([]);

        get.instruction(&I::LocalGet(0))
            .instruction(&I::TableGet(table.index))
            .instruction(&I::End);

        let get = functions.push(types.intern([ValType::I32], [ValType::EXTERNREF]), get);
        exports.insert("wasmgc.externref_get".to_owned(), get);

        Self {
            enabled,
            new,
            release,
            table,
        }
    }

    pub fn append_tables(&self, tables: &mut Vec<TableType>) {
        if self.enabled {
            self.table.append_table(tables);
        }
    }

    pub fn append_globals(&self, globals: &mut Vec<(GlobalType, ConstExpr)>) {
        if self.enabled {
            self.table.append_globals(globals);
        }
    }

    pub fn intrinsic(&self, name: &str) -> Option<Function> {
        let mut function = Function::new([]);

        match name {
            "externref_null" => {
                function.instruction(&I::RefNull(HeapType::EXTERN));
                self.value_output(&mut function);
            }
            "externref_is_null" => {
                self.value_input(&mut function, 0);
                function.instruction(&I::RefIsNull);
            }
            "externref_clone" => {
                self.value_input(&mut function, 0);
                self.value_output(&mut function);
            }
            "externref_release" => {
                function
                    .instruction(&I::LocalGet(0))
                    .instruction(&I::Call(self.release));
            }
            _ => return None,
        }

        function.instruction(&I::End);

        Some(function)
    }

    /// Emits a borrowed lookup of the root handle in `local`.
    ///
    /// Leaves the external reference on the operand stack without releasing
    /// or transferring the slot, so callers may still use the Rust handle.
    pub fn value_input(&self, function: &mut Function, local: u32) {
        function
            .instruction(&I::LocalGet(local))
            .instruction(&I::TableGet(self.table.index));
    }

    /// Emits release of the owned root handle in `local`.
    ///
    /// Values already on the operand stack remain available to the caller.
    pub fn value_release(&self, function: &mut Function, local: u32) {
        function
            .instruction(&I::LocalGet(local))
            .instruction(&I::Call(self.release));
    }

    /// Consumes a stack external reference and leaves a newly owned root handle.
    ///
    /// Null receives a slot just like any other external reference.
    pub fn value_output(&self, function: &mut Function) {
        function.instruction(&I::Call(self.new));
    }
}
