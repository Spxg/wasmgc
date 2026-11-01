//! Emits the tables and helpers that keep GC objects reachable from Rust.
//!
//! Rust owns numeric slots rather than native references. A live slot keeps
//! its object reachable; releasing it replaces the reference with a free-list
//! link. Cloning allocates another slot for the same object. These tables and
//! allocation cursors belong to one Wasm instance.

use super::{Input, function::FunctionRegistry, types::FunctionTypes};
use crate::schema::ValueTag;
use std::collections::BTreeMap;
use wasm_encoder::{
    AbstractHeapType, BlockType, ConstExpr, Function, GlobalType, HeapType, Instruction as I,
    RefType, TableType, ValType,
};

pub(super) const EQ: HeapType = HeapType::Abstract {
    shared: false,
    ty: AbstractHeapType::Eq,
};

pub(super) fn managed_ref(nullable: bool) -> ValType {
    ValType::Ref(RefType {
        nullable,
        heap_type: EQ,
    })
}

const GROW_BATCH: i32 = 64;

/// A table whose nonzero slots alternate between live values and free-list links.
///
/// Slot zero remains null. Two globals track the free-list head and the next
/// never-used slot; links occupy released slots as unsigned `i31` values.
pub(super) struct RootTable {
    pub index: u32,
    first_global: u32,
    heap_type: HeapType,
}

impl RootTable {
    pub fn new(index: u32, first_global: u32, heap_type: HeapType) -> Self {
        Self {
            index,
            first_global,
            heap_type,
        }
    }

    pub fn append_table(&self, tables: &mut Vec<TableType>) {
        tables.push(TableType {
            element_type: RefType {
                nullable: true,
                heap_type: self.heap_type,
            },
            table64: false,
            minimum: 1,
            maximum: None,
            shared: false,
        });
    }

    pub fn append_globals(&self, globals: &mut Vec<(GlobalType, ConstExpr)>) {
        // An empty free list and a cursor past the permanently reserved null slot.
        for initial in [0, 1] {
            globals.push((
                GlobalType {
                    val_type: ValType::I32,
                    mutable: true,
                    shared: false,
                },
                ConstExpr::i32_const(initial),
            ));
        }
    }

    /// Emits allocation of a slot, saving its index in the `i32` local `local`.
    ///
    /// The caller must write a live value into that slot before it is observed.
    /// Allocation traps on growth failure and never publishes an unallocated index.
    pub fn allocate(&self, function: &mut Function, local: u32) {
        let free = self.first_global;
        let next = free + 1;

        for instruction in [
            I::GlobalGet(free),
            I::If(BlockType::Empty),
            I::GlobalGet(free),
            I::LocalTee(local),
            I::TableGet(self.index),
        ] {
            function.instruction(&instruction);
        }

        // Only released slots contain converted i31 links. Live external values are
        // stored unchanged and are never passed through any.convert_extern here.
        if self.heap_type == HeapType::EXTERN {
            function.instruction(&I::AnyConvertExtern);
        }

        for instruction in [
            I::RefCastNonNull(HeapType::I31),
            I::I31GetU,
            I::GlobalSet(free),
            I::Else,
            I::GlobalGet(next),
            I::LocalTee(local),
            I::TableSize(self.index),
            I::I32Eq,
            I::If(BlockType::Empty),
            // All released-slot links must remain representable as unsigned i31 values.
            I::LocalGet(local),
            I::I32Const(i32::MAX),
            I::I32GeU,
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
            I::RefNull(self.heap_type),
            I::LocalGet(local),
            I::I32Const(i32::MAX - GROW_BATCH),
            I::I32LeU,
            I::If(BlockType::Result(ValType::I32)),
            I::I32Const(GROW_BATCH),
            I::Else,
            I::I32Const(i32::MAX),
            I::LocalGet(local),
            I::I32Sub,
            I::End,
            I::TableGrow(self.index),
            I::I32Const(-1),
            I::I32Eq,
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
            I::End,
            // Advance the cursor only after table.grow has succeeded.
            I::LocalGet(local),
            I::I32Const(1),
            I::I32Add,
            I::GlobalSet(next),
            I::End,
        ] {
            function.instruction(&instruction);
        }
    }

    /// Emits release of the live slot held in `local`.
    ///
    /// Replacing its value with a free-list link removes this root without
    /// affecting other slots or references to the same object.
    pub fn release(&self, function: &mut Function, local: u32) {
        for instruction in [
            I::LocalGet(local),
            I::GlobalGet(self.first_global),
            I::RefI31,
        ] {
            function.instruction(&instruction);
        }

        // An externref table stores a free-list link through extern.convert_any.
        if self.heap_type == HeapType::EXTERN {
            function.instruction(&I::ExternConvertAny);
        }

        for instruction in [
            I::TableSet(self.index),
            I::LocalGet(local),
            I::GlobalSet(self.first_global),
        ] {
            function.instruction(&instruction);
        }
    }
}

pub(super) struct Roots {
    enabled: bool,
    pub table: RootTable,
    pub lookup: u32,
    pub root: u32,
    pub unroot: u32,
}

impl Roots {
    pub fn new(
        input: &Input,
        table: u32,
        first_global: u32,
        types: &mut FunctionTypes,
        functions: &mut FunctionRegistry,
        exports: &mut BTreeMap<String, u32>,
    ) -> Self {
        let enabled = !input.operations.is_empty()
            || input
                .bridges
                .iter()
                .flat_map(|bridge| bridge.params.iter().chain(&bridge.result))
                .any(|value| value.tag.is_reference() && value.tag != ValueTag::ExternRef)
            || input
                .imports
                .iter()
                .filter_map(|(name, _)| name.as_deref())
                .any(|name| matches!(name, "ref_clone" | "ref_release" | "ref_eq"));
        let table = RootTable::new(table, first_global, HeapType::ANY);
        if !enabled {
            return Self {
                enabled,
                table,
                lookup: 0,
                root: 0,
                unroot: 0,
            };
        }

        let mut lookup = Function::new([]);

        for instruction in [
            I::LocalGet(0),
            I::TableGet(table.index),
            I::RefCastNonNull(EQ),
            I::End,
        ] {
            lookup.instruction(&instruction);
        }

        let lookup = functions.push(types.intern([ValType::I32], [managed_ref(false)]), lookup);
        exports.insert("wasmgc.ref_get".to_owned(), lookup);

        // Unlike external references, a nullable GC reference uses handle zero for
        // null and allocates slots only for actual objects.
        let mut root = Function::new([(1, ValType::I32)]);

        for instruction in [
            I::LocalGet(0),
            I::RefIsNull,
            I::If(BlockType::Empty),
            I::I32Const(0),
            I::Return,
            I::End,
        ] {
            root.instruction(&instruction);
        }

        table.allocate(&mut root, 1);

        for instruction in [
            I::LocalGet(1),
            I::LocalGet(0),
            I::TableSet(table.index),
            I::LocalGet(1),
            I::End,
        ] {
            root.instruction(&instruction);
        }

        let root = functions.push(types.intern([managed_ref(true)], [ValType::I32]), root);

        let mut unroot = Function::new([]);

        table.release(&mut unroot, 0);
        unroot.instruction(&I::End);

        let unroot = functions.push(types.intern([ValType::I32], []), unroot);

        Self {
            enabled,
            table,
            lookup,
            root,
            unroot,
        }
    }

    pub fn append_table(&self, tables: &mut Vec<TableType>) {
        if self.enabled {
            self.table.append_table(tables);
        }
    }

    pub fn append_globals(&self, globals: &mut Vec<(GlobalType, ConstExpr)>) {
        if self.enabled {
            self.table.append_globals(globals);
        }
    }
}
