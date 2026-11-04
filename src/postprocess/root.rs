use super::{Resolver, Transformer};
use crate::schema::{OperationKind, ValueTag};
use wasm_encoder::{
    AbstractHeapType, BlockType, ConstExpr, FuncType, Function, GlobalSection, GlobalType,
    HeapType, Instruction as I, RefType, TableSection, TableType, ValType,
};

pub const EQ: HeapType = HeapType::Abstract {
    shared: false,
    ty: AbstractHeapType::Eq,
};

const GROW_BATCH: i32 = 64;

pub fn managed_ref(nullable: bool) -> ValType {
    ValType::Ref(RefType {
        nullable,
        heap_type: EQ,
    })
}

pub struct GcTable {
    pub index: u32,
    pub global_start: u32,
    pub root: u32,
}

impl Resolver<'_> {
    pub fn resolve_tables(&mut self) {
        self.gc_table = (self
            .intrinsics
            .iter()
            .any(|&(_, _, name)| matches!(name, "ref_clone" | "ref_release" | "ref_eq"))
            || !self.operations.is_empty()
            || self
                .function_exports
                .iter()
                .flat_map(|export| export.params.iter().chain(&export.result))
                .chain(
                    self.function_imports
                        .iter()
                        .flat_map(|import| import.params.iter().chain(&import.result)),
                )
                .any(|value| value.tag.is_reference() && value.tag != ValueTag::ExternRef))
        .then_some(GcTable {
            index: self.original_tables,
            global_start: self.new_global_start,
            root: self.new_function_start,
        });
        self.externref_table = (self.intrinsics.iter().any(|&(_, _, name)| {
            matches!(
                name,
                "externref_null" | "externref_is_null" | "externref_clone" | "externref_release"
            )
        }) || self.operations.values().any(|operation| {
            let fields = &self.schema_set[&operation.schema_addr].fields;
            match operation.kind {
                OperationKind::StructNew => {
                    fields.iter().any(|value| value.tag == ValueTag::ExternRef)
                }
                OperationKind::StructGet | OperationKind::StructSet => {
                    fields[operation.field_index as usize].tag == ValueTag::ExternRef
                }
                OperationKind::ArrayNew
                | OperationKind::ArrayGet
                | OperationKind::ArraySet
                | OperationKind::ArrayFill
                | OperationKind::ArrayReadAbi
                | OperationKind::ArrayWriteAbi => fields[0].tag == ValueTag::ExternRef,
                _ => false,
            }
        }) || self
            .function_exports
            .iter()
            .flat_map(|export| export.params.iter().chain(&export.result))
            .chain(
                self.function_imports
                    .iter()
                    .flat_map(|import| import.params.iter().chain(&import.result)),
            )
            .any(|value| value.tag == ValueTag::ExternRef))
        .then_some(GcTable {
            index: self.original_tables + u32::from(self.gc_table.is_some()),
            global_start: self.new_global_start + 2 * u32::from(self.gc_table.is_some()),
            root: self.new_function_start + u32::from(self.gc_table.is_some()),
        });
    }
}

impl Transformer<'_> {
    pub fn intrinsic(&mut self) {
        for &(_, ty, name) in &self.resolver.intrinsics {
            let external = name.starts_with("externref_");
            let table = if external {
                self.resolver.externref_table.as_ref().unwrap()
            } else {
                self.resolver.gc_table.as_ref().unwrap()
            };
            let mut function = Function::new([]);
            match name {
                "ref_clone" => {
                    self.ref_input(&mut function, 0);
                    function.instruction(&I::Call(table.root));
                }
                "ref_release" | "externref_release" => {
                    self.release(&mut function, table, 0);
                }
                "ref_eq" => {
                    self.ref_input(&mut function, 0);
                    self.ref_input(&mut function, 1);
                    function.instruction(&I::RefEq);
                }
                "externref_null" => {
                    function
                        .instruction(&I::RefNull(HeapType::EXTERN))
                        .instruction(&I::Call(table.root));
                }
                "externref_is_null" | "externref_clone" => {
                    function
                        .instruction(&I::LocalGet(0))
                        .instruction(&I::TableGet(table.index))
                        .instruction(&if name == "externref_clone" {
                            I::Call(table.root)
                        } else {
                            I::RefIsNull
                        });
                }
                _ => unreachable!("unsupported reference intrinsic"),
            }

            function.instruction(&I::End);
            self.functions.push((ty, function));
        }
    }

    pub fn release(&self, function: &mut Function, table: &GcTable, local: u32) {
        for instruction in [
            I::LocalGet(local),
            I::GlobalGet(table.global_start),
            I::RefI31,
        ] {
            function.instruction(&instruction);
        }
        if self
            .resolver
            .externref_table
            .as_ref()
            .is_some_and(|external| external.index == table.index)
        {
            function.instruction(&I::ExternConvertAny);
        }
        for instruction in [
            I::TableSet(table.index),
            I::LocalGet(local),
            I::GlobalSet(table.global_start),
        ] {
            function.instruction(&instruction);
        }
    }

    fn ref_input(&self, function: &mut Function, local: u32) {
        function
            .instruction(&I::LocalGet(local))
            .instruction(&I::TableGet(self.resolver.gc_table.as_ref().unwrap().index))
            .instruction(&I::RefCastNonNull(EQ));
    }

    pub fn root(&mut self) {
        for (table, heap_type, param) in [
            (&self.resolver.gc_table, HeapType::ANY, managed_ref(true)),
            (
                &self.resolver.externref_table,
                HeapType::EXTERN,
                ValType::EXTERNREF,
            ),
        ] {
            let Some(table) = table else {
                continue;
            };
            let ty = self.resolver.original_types
                + self.resolver.type_indices.len() as u32
                + self.signatures.len() as u32;
            let body = self.root_function(table, heap_type);
            self.signatures.push(FuncType::new([param], [ValType::I32]));
            self.functions.push((ty, body));
        }
    }

    pub fn root_function(&self, table: &GcTable, heap_type: HeapType) -> Function {
        let free = table.global_start;
        let next = free + 1;
        let mut function = Function::new([(1, ValType::I32)]);

        if heap_type == HeapType::ANY {
            for instruction in [
                I::LocalGet(0),
                I::RefIsNull,
                I::If(BlockType::Empty),
                I::I32Const(0),
                I::Return,
                I::End,
            ] {
                function.instruction(&instruction);
            }
        }

        for instruction in [
            I::GlobalGet(free),
            I::If(BlockType::Empty),
            I::GlobalGet(free),
            I::LocalTee(1),
            I::TableGet(table.index),
        ] {
            function.instruction(&instruction);
        }
        if heap_type == HeapType::EXTERN {
            function.instruction(&I::AnyConvertExtern);
        }
        for instruction in [
            I::RefCastNonNull(HeapType::I31),
            I::I31GetU,
            I::GlobalSet(free),
            I::Else,
            I::GlobalGet(next),
            I::LocalTee(1),
            I::TableSize(table.index),
            I::I32Eq,
            I::If(BlockType::Empty),
            I::RefNull(heap_type),
            I::I32Const(GROW_BATCH),
            I::TableGrow(table.index),
            I::I32Const(-1),
            I::I32Eq,
            I::If(BlockType::Empty),
            I::Unreachable,
            I::End,
            I::End,
            I::LocalGet(1),
            I::I32Const(1),
            I::I32Add,
            I::GlobalSet(next),
            I::End,
            I::LocalGet(1),
            I::LocalGet(0),
            I::TableSet(table.index),
            I::LocalGet(1),
            I::End,
        ] {
            function.instruction(&instruction);
        }

        function
    }

    pub fn append_tables(&self, tables: &mut TableSection) {
        let table = |heap_type| TableType {
            element_type: RefType {
                nullable: true,
                heap_type,
            },
            table64: false,
            minimum: 1,
            maximum: Some(i32::MAX as u64),
            shared: false,
        };

        for (descriptor, heap_type) in [
            (&self.resolver.gc_table, HeapType::ANY),
            (&self.resolver.externref_table, HeapType::EXTERN),
        ] {
            if descriptor.is_some() {
                tables.table(table(heap_type));
            }
        }
    }

    pub fn append_globals(&self, globals: &mut GlobalSection) {
        for _ in [&self.resolver.gc_table, &self.resolver.externref_table]
            .into_iter()
            .flatten()
        {
            for initial in [0, 1] {
                globals.global(
                    GlobalType {
                        val_type: ValType::I32,
                        mutable: true,
                        shared: false,
                    },
                    &ConstExpr::i32_const(initial),
                );
            }
        }
    }
}
