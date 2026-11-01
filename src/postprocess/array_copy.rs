//! Emits element loops between GC arrays and linear-memory buffers.
//!
//! Scalar buffers use Rust element widths; ABI buffers use the numeric
//! representation selected by `GcValue`. Reference elements are resolved or
//! rooted when they cross an ABI buffer. Each generated loop looks up the
//! array once and keeps the reference in a Wasm local while copying elements.

use super::root::Roots;
use crate::schema::{OperationKind, ValueTag};
use wasm_encoder::{
    BlockType, Function, HeapType, Instruction as I, MemArg, MemArgTarget, RefType, ValType,
};

// ABI writes have four parameters, followed by the array and cursor locals.
const ABI_VALUE_LOCAL: u32 = 6;

fn memory_address(function: &mut Function, pointer: u32, cursor: u32, memory64: bool, stride: u32) {
    function
        .instruction(&I::LocalGet(pointer))
        .instruction(&I::LocalGet(cursor));

    if memory64 {
        function.instruction(&I::I64ExtendI32U);

        if stride != 1 {
            function
                .instruction(&I::I64Const(i64::from(stride)))
                .instruction(&I::I64Mul);
        }

        function.instruction(&I::I64Add);
    } else {
        if stride != 1 {
            function
                .instruction(&I::I32Const(stride as i32))
                .instruction(&I::I32Mul);
        }

        function.instruction(&I::I32Add);
    }
}

struct MemoryCopy<'a> {
    ty: u32,
    memory64: bool,
    tag: ValueTag,
    abi: bool,
    value_input: &'a dyn Fn(&mut Function, u32),
    value_output: &'a dyn Fn(&mut Function),
}

impl MemoryCopy<'_> {
    // ABI buffers contain numeric handles and widened packed scalars. Raw
    // scalar buffers instead keep Rust's byte/word element representation.
    fn memory_tag(&self) -> ValueTag {
        if self.abi {
            match self.tag {
                ValueTag::I64 | ValueTag::F32 | ValueTag::F64 => self.tag,
                ValueTag::I8
                | ValueTag::I16
                | ValueTag::I32
                | ValueTag::Bool
                | ValueTag::ExternRef
                | ValueTag::Ref
                | ValueTag::NullableRef
                | ValueTag::Array
                | ValueTag::NullableArray => ValueTag::I32,
            }
        } else {
            self.tag
        }
    }

    fn emit(
        &self,
        function: &mut Function,
        array: u32,
        cursor: u32,
        memory_range: (u32, u32),
        offset: Option<u32>,
        read_to_memory: bool,
    ) {
        let (pointer, length) = memory_range;
        let memory = MemArg {
            offset: 0,
            align: 0,
            target: MemArgTarget::Memory(0),
            ordering: None,
        };
        let tag = self.memory_tag();
        let stride = match tag {
            ValueTag::I8 | ValueTag::Bool => 1,
            ValueTag::I16 => 2,
            ValueTag::I32 | ValueTag::F32 => 4,
            ValueTag::I64 | ValueTag::F64 => 8,
            _ => unreachable!("scalar memory copy storage"),
        };

        function
            .instruction(&I::Block(BlockType::Empty))
            .instruction(&I::Loop(BlockType::Empty))
            .instruction(&I::LocalGet(cursor))
            .instruction(&I::LocalGet(length))
            .instruction(&I::I32GeU)
            .instruction(&I::BrIf(1));

        if read_to_memory {
            memory_address(function, pointer, cursor, self.memory64, stride);
        }

        function.instruction(&I::LocalGet(array));

        if let Some(offset) = offset {
            function
                .instruction(&I::LocalGet(offset))
                .instruction(&I::LocalGet(cursor))
                .instruction(&I::I32Add);
        } else {
            function.instruction(&I::LocalGet(cursor));
        }

        if read_to_memory {
            if matches!(self.tag, ValueTag::I8 | ValueTag::I16) {
                function.instruction(&I::ArrayGetU(self.ty));
            } else {
                function.instruction(&I::ArrayGet(self.ty));
            }

            if self.abi {
                (self.value_output)(function);
            } else if tag == ValueTag::Bool {
                // Native code may store any i32 in a boolean element. Canonicalize the
                // value before writing Rust bool storage, which only permits 0 and 1.
                function.instruction(&I::I32Eqz).instruction(&I::I32Eqz);
            }

            function.instruction(&match tag {
                ValueTag::I8 | ValueTag::Bool => I::I32Store8(memory),
                ValueTag::I16 => I::I32Store16(memory),
                ValueTag::I32 => I::I32Store(memory),
                ValueTag::I64 => I::I64Store(memory),
                ValueTag::F32 => I::F32Store(memory),
                ValueTag::F64 => I::F64Store(memory),
                _ => unreachable!("scalar memory copy storage"),
            });
        } else {
            memory_address(function, pointer, cursor, self.memory64, stride);
            function.instruction(&match tag {
                ValueTag::I8 | ValueTag::Bool => I::I32Load8U(memory),
                ValueTag::I16 => I::I32Load16U(memory),
                ValueTag::I32 => I::I32Load(memory),
                ValueTag::I64 => I::I64Load(memory),
                ValueTag::F32 => I::F32Load(memory),
                ValueTag::F64 => I::F64Load(memory),
                _ => unreachable!("scalar memory copy storage"),
            });

            if self.abi {
                function.instruction(&I::LocalSet(ABI_VALUE_LOCAL));
                (self.value_input)(function, ABI_VALUE_LOCAL);
            }

            function.instruction(&I::ArraySet(self.ty));
        }

        function
            .instruction(&I::LocalGet(cursor))
            .instruction(&I::I32Const(1))
            .instruction(&I::I32Add)
            .instruction(&I::LocalSet(cursor))
            .instruction(&I::Br(0))
            .instruction(&I::End)
            .instruction(&I::End);
    }
}

pub(super) fn memory_operation(
    kind: OperationKind,
    ty: u32,
    memory64: bool,
    roots: &Roots,
    tag: ValueTag,
    value_input: impl Fn(&mut Function, u32),
    value_output: impl Fn(&mut Function),
) -> Function {
    let abi = matches!(
        kind,
        OperationKind::ArrayReadAbi | OperationKind::ArrayWriteAbi
    );
    let copy = MemoryCopy {
        ty,
        memory64,
        tag,
        abi,
        value_input: &value_input,
        value_output: &value_output,
    };
    let array_ref = ValType::Ref(RefType {
        nullable: true,
        heap_type: HeapType::Concrete(ty),
    });
    let mut locals = vec![(1, array_ref), (1, ValType::I32)];

    if kind == OperationKind::ArrayWriteAbi {
        let value_type = match copy.memory_tag() {
            ValueTag::I64 => ValType::I64,
            ValueTag::F32 => ValType::F32,
            ValueTag::F64 => ValType::F64,
            ValueTag::I8
            | ValueTag::I16
            | ValueTag::I32
            | ValueTag::Bool
            | ValueTag::ExternRef
            | ValueTag::Ref
            | ValueTag::NullableRef
            | ValueTag::Array
            | ValueTag::NullableArray => ValType::I32,
        };
        locals.push((1, value_type));
    }

    let from_slice = kind == OperationKind::ArrayFromSlice;
    let (pointer, length, array, cursor, offset) = if from_slice {
        (0, 1, 2, 3, None)
    } else {
        (2, 3, 4, 5, Some(1))
    };
    let read_to_memory = matches!(kind, OperationKind::ArrayRead | OperationKind::ArrayReadAbi);
    let mut function = Function::new(locals);

    if from_slice {
        function
            .instruction(&I::LocalGet(length))
            .instruction(&I::ArrayNewDefault(ty))
            .instruction(&I::LocalSet(array));
    } else {
        // Hold the array reference in a local for the entire loop. Element copies
        // may allocate result roots without invalidating this local reference.
        function
            .instruction(&I::LocalGet(0))
            .instruction(&I::TableGet(roots.table.index))
            .instruction(&I::RefCastNonNull(HeapType::Concrete(ty)))
            .instruction(&I::LocalSet(array));
    }

    copy.emit(
        &mut function,
        array,
        cursor,
        (pointer, length),
        offset,
        read_to_memory,
    );

    if from_slice {
        function
            .instruction(&I::LocalGet(array))
            .instruction(&I::Call(roots.root));
    }

    function.instruction(&I::End);

    function
}
