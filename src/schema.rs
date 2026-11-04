//! Metadata connecting Rust type checking to Wasm code generation.
//!
//! The runtime and procedural macro emit these `repr(C)` descriptors into the
//! linked module's linear memory. Their pointers are resolved by the linker;
//! the postprocessor reads the resulting bytes without executing the module.
//! [`Layout`] describes both pointer widths, independently of the host ABI.
//!
//! Schemas describe Wasm storage, not Rust object layouts. Numeric shims pass
//! reference handles, while generated GC fields and native imports and exports
//! contain actual Wasm references. This protocol is an implementation detail
//! shared by the runtime, macro, and postprocessor.

/// The byte length of each descriptor signature.
pub const MAGIC_LEN: usize = 16;

/// The signature at the start of a [`Schema`] descriptor.
pub const SCHEMA_MAGIC: [u8; MAGIC_LEN] = *b"wasmgc.schema\0\0\0";

/// The signature at the start of an [`Operation`] descriptor.
pub const OPERATION_MAGIC: [u8; MAGIC_LEN] = *b"wasmgc.opcode\0\0\0";

/// The signature at the start of a [`Struct`] descriptor.
pub const STRUCT_MAGIC: [u8; MAGIC_LEN] = *b"wasmgc.struct\0\0\0";

/// The signature at the start of a [`Function`] descriptor.
pub const FUNCTION_MAGIC: [u8; MAGIC_LEN] = *b"wasmgc.func\0\0\0\0\0";

/// The prefix of temporary globals exposing [`Struct`] descriptor addresses.
pub const STRUCT_EXPORT_DESC_PREFIX: &str = "__wasmgc_struct_export_desc.";

/// The prefix of temporary exports exposing numeric Rust bridge functions.
pub const FUNCTION_BRIDGE_PREFIX: &str = "__wasmgc_function_bridge.";

/// The prefix of temporary globals exposing [`Function`] descriptor addresses.
pub const FUNCTION_DESC_PREFIX: &str = "__wasmgc_function_desc.";

/// Descriptor sizes and member offsets in the linked module's linear memory.
///
/// Offsets and sizes are measured in bytes. Pointers use the target's pointer
/// width; tags and counts remain little-endian `u32` values on both targets.
/// Padding follows the target C ABI rather than the host's Rust layout.
///
/// The decoder selects [`WASM32`](Self::WASM32) or [`WASM64`](Self::WASM64)
/// from the module's memory type. Inline value arrays use the target pointer
/// alignment even when the preceding header contains no pointers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// The width of a data pointer or a function-table slot.
    pub pointer_size: u32,
    /// The stride of [`ValueKind`] entries.
    pub value_kind_size: u32,
    /// The offset of a value storage tag.
    pub value_kind_tag_offset: u32,
    /// The offset of a referenced schema pointer.
    pub value_kind_target_offset: u32,
    /// The size of a [`Schema`] header, including padding.
    pub schema_size: u32,
    /// The offset of [`SCHEMA_MAGIC`].
    pub schema_magic_offset: u32,
    /// The offset of the [`SchemaKind`] discriminant.
    pub schema_kind_offset: u32,
    /// The offset of the schema field count.
    pub schema_field_count_offset: u32,
    /// The offset of the inline field descriptors in [`SchemaData`].
    pub schema_fields_offset: u32,
    /// The complete size of an [`Operation`] descriptor.
    pub operation_size: u32,
    /// The offset of [`OPERATION_MAGIC`].
    pub operation_magic_offset: u32,
    /// The offset of the shim function pointer, encoded as a table slot.
    pub operation_function_offset: u32,
    /// The offset of the operation schema pointer.
    pub operation_schema_offset: u32,
    /// The offset of the [`OperationKind`] discriminant.
    pub operation_kind_offset: u32,
    /// The offset of the operation field index.
    pub operation_field_index_offset: u32,
    /// The size of a [`Struct`] header, including padding.
    pub struct_export_size: u32,
    /// The offset of [`STRUCT_MAGIC`].
    pub struct_export_magic_offset: u32,
    /// The offset of the exported operation schema pointer.
    pub struct_export_schema_offset: u32,
    /// The offset of the exported [`OperationKind`] discriminant.
    pub struct_export_kind_offset: u32,
    /// The offset of the exported operation field index.
    pub struct_export_field_offset: u32,
    /// The offset of the inline export name in [`StructData`].
    pub struct_export_name_offset: u32,
    /// The offset of the export name byte count.
    pub struct_export_name_len_offset: u32,
    /// The size of a [`Function`] header, including padding.
    pub function_size: u32,
    /// The offset of [`FUNCTION_MAGIC`].
    pub function_magic_offset: u32,
    /// The offset of the [`FunctionKind`] discriminant.
    pub function_kind_offset: u32,
    /// The offset of the parameter count.
    pub function_param_count_offset: u32,
    /// The offset of inline parameter descriptors in [`FunctionData`].
    pub function_params_offset: u32,
    /// The offset of the result count, which is zero or one.
    pub function_result_count_offset: u32,
    /// The offset of the import module name byte count.
    pub function_module_len_offset: u32,
    /// The offset of the native function name byte count.
    pub function_name_len_offset: u32,
}

impl Layout {
    /// The descriptor layout for a module with 32-bit linear-memory pointers.
    pub const WASM32: Self = Self {
        pointer_size: 4,
        value_kind_size: 8,
        value_kind_tag_offset: 0,
        value_kind_target_offset: 4,
        schema_size: 24,
        schema_magic_offset: 0,
        schema_kind_offset: 16,
        schema_field_count_offset: 20,
        schema_fields_offset: 24,
        operation_size: 32,
        operation_magic_offset: 0,
        operation_function_offset: 16,
        operation_schema_offset: 20,
        operation_kind_offset: 24,
        operation_field_index_offset: 28,
        struct_export_size: 32,
        struct_export_magic_offset: 0,
        struct_export_schema_offset: 16,
        struct_export_kind_offset: 20,
        struct_export_field_offset: 24,
        struct_export_name_offset: 32,
        struct_export_name_len_offset: 28,
        function_size: 36,
        function_magic_offset: 0,
        function_kind_offset: 16,
        function_param_count_offset: 20,
        function_params_offset: 36,
        function_result_count_offset: 24,
        function_module_len_offset: 28,
        function_name_len_offset: 32,
    };

    /// The descriptor layout for a module with 64-bit linear-memory pointers.
    pub const WASM64: Self = Self {
        pointer_size: 8,
        value_kind_size: 16,
        value_kind_tag_offset: 0,
        value_kind_target_offset: 8,
        schema_size: 24,
        schema_magic_offset: 0,
        schema_kind_offset: 16,
        schema_field_count_offset: 20,
        schema_fields_offset: 24,
        operation_size: 40,
        operation_magic_offset: 0,
        operation_function_offset: 16,
        operation_schema_offset: 24,
        operation_kind_offset: 32,
        operation_field_index_offset: 36,
        struct_export_size: 40,
        struct_export_magic_offset: 0,
        struct_export_schema_offset: 16,
        struct_export_kind_offset: 24,
        struct_export_field_offset: 28,
        struct_export_name_offset: 40,
        struct_export_name_len_offset: 32,
        function_size: 36,
        function_magic_offset: 0,
        function_kind_offset: 16,
        function_param_count_offset: 20,
        function_params_offset: 40,
        function_result_count_offset: 24,
        function_module_len_offset: 28,
        function_name_len_offset: 32,
    };
}

/// The storage class encoded in a [`ValueKind`].
///
/// Tags describe Wasm storage for fields, array elements, and function values,
/// independently of the Rust wrapper implementing [`GcValue`](crate::GcValue).
/// Reference tags distinguish struct, array, and external references so the
/// encoder can select the appropriate concrete type or root table.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ValueTag {
    /// An `i32` value.
    I32 = 1,
    /// An `i64` value.
    I64 = 2,
    /// An `f32` value.
    F32 = 3,
    /// An `f64` value.
    F64 = 4,
    /// An `i32` value interpreted by Rust as a boolean.
    Bool = 5,
    /// A non-null reference to a concrete struct type.
    Ref = 6,
    /// A nullable reference to a concrete struct type.
    NullableRef = 7,
    /// A non-null reference to a concrete array type.
    Array = 8,
    /// A nullable reference to a concrete array type.
    NullableArray = 10,
    /// A nullable `externref`, with no target schema.
    ExternRef = 11,
    /// Packed `i8` storage, read with zero extension.
    I8 = 12,
    /// Packed `i16` storage, read with zero extension.
    I16 = 13,
}

impl ValueTag {
    /// Returns the tag with this protocol value, or `None` if it is unknown.
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::I32),
            2 => Some(Self::I64),
            3 => Some(Self::F32),
            4 => Some(Self::F64),
            5 => Some(Self::Bool),
            6 => Some(Self::Ref),
            7 => Some(Self::NullableRef),
            8 => Some(Self::Array),
            10 => Some(Self::NullableArray),
            11 => Some(Self::ExternRef),
            12 => Some(Self::I8),
            13 => Some(Self::I16),
            _ => None,
        }
    }

    /// Returns whether the value needs a reference-table conversion in the Rust ABI.
    pub const fn is_reference(self) -> bool {
        matches!(
            self,
            Self::Ref | Self::NullableRef | Self::Array | Self::NullableArray | Self::ExternRef
        )
    }
}

/// An operation whose numeric Rust shim is replaced by WasmGC instructions.
///
/// Reference operands are looked up in instance-local root tables, and
/// reference results receive owned roots. The struct and array operations
/// below use the named Wasm instruction; the slice and ABI-buffer operations
/// combine multiple instructions in a generated loop.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OperationKind {
    /// Allocates a struct with `struct.new`.
    StructNew = 0,
    /// Reads a field with `struct.get`, or `struct.get_u` for packed storage.
    StructGet = 1,
    /// Writes a field with `struct.set`.
    StructSet = 2,
    /// Allocates repeated elements with `array.new`.
    ArrayNew = 3,
    /// Allocates an empty array with `array.new_fixed` and an immediate length of zero.
    ArrayNewFixed = 4,
    /// Allocates zero or null elements with `array.new_default`.
    ArrayNewDefault = 13,
    /// Reads the element count with `array.len`.
    ArrayLen = 5,
    /// Reads an element with `array.get`, or `array.get_u` for packed storage.
    ArrayGet = 6,
    /// Writes an element with `array.set`.
    ArraySet = 7,
    /// Fills the entire array with `array.len` followed by `array.fill`.
    ArrayFill = 8,
    /// Copies a range with `array.copy`, which permits overlapping ranges.
    ArrayCopy = 9,

    /// Allocates an array from scalar linear-memory values.
    ///
    /// Uses `array.new_default`, then loads each scalar and stores it with
    /// `array.set`. This is a library operation rather than a bulk-copy instruction.
    ArrayFromSlice = 10,
    /// Copies scalars to linear memory with `array.get`/`array.get_u` and scalar stores.
    ArrayRead = 11,
    /// Copies scalars from linear memory with scalar loads and `array.set`.
    ArrayWrite = 12,
    /// Copies elements to a numeric ABI buffer in linear memory.
    ///
    /// Uses `array.get`/`array.get_u` and scalar stores. Reference elements
    /// are converted to newly owned root handles rather than copied as references.
    ArrayReadAbi = 14,
    /// Copies elements from a numeric ABI buffer into the array.
    ///
    /// Uses scalar loads and `array.set`. Reference handles are borrowed
    /// from their root tables; writing an element does not consume its input root.
    ArrayWriteAbi = 15,
}

impl OperationKind {
    /// Returns the operation with this protocol value, or `None` if it is unknown.
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::StructNew),
            1 => Some(Self::StructGet),
            2 => Some(Self::StructSet),
            3 => Some(Self::ArrayNew),
            4 => Some(Self::ArrayNewFixed),
            5 => Some(Self::ArrayLen),
            6 => Some(Self::ArrayGet),
            7 => Some(Self::ArraySet),
            8 => Some(Self::ArrayFill),
            9 => Some(Self::ArrayCopy),
            10 => Some(Self::ArrayFromSlice),
            11 => Some(Self::ArrayRead),
            12 => Some(Self::ArrayWrite),
            13 => Some(Self::ArrayNewDefault),
            14 => Some(Self::ArrayReadAbi),
            15 => Some(Self::ArrayWriteAbi),
            _ => None,
        }
    }
}

/// The Wasm storage type of a field, array element, or function value.
///
/// Scalar constants select a built-in storage type. [`reference`](Self::reference)
/// and [`array`](Self::array) additionally name a target schema. GC references
/// use numeric root handles at Rust call sites and become native references
/// in the generated Wasm.
///
/// A custom [`GcValue`](crate::GcValue) usually reuses the `KIND` of its
/// underlying value, keeping storage and ABI conversion consistent.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ValueKind {
    tag: ValueTag,
    target: *const Schema,
}

impl ValueKind {
    /// Packed `i8` storage with an `i32` ABI and zero-extended reads.
    pub const I8: Self = Self::scalar(ValueTag::I8);

    /// Packed `i16` storage with an `i32` ABI and zero-extended reads.
    pub const I16: Self = Self::scalar(ValueTag::I16);

    /// Full-width `i32` storage and an `i32`/`u32` Rust ABI.
    pub const I32: Self = Self::scalar(ValueTag::I32);

    /// Full-width `i64` storage and ABI.
    pub const I64: Self = Self::scalar(ValueTag::I64);

    /// IEEE 754 binary32 storage and ABI.
    pub const F32: Self = Self::scalar(ValueTag::F32);

    /// IEEE 754 binary64 storage and ABI.
    pub const F64: Self = Self::scalar(ValueTag::F64);

    /// An `i32` value interpreted as false for zero and true otherwise.
    pub const BOOL: Self = Self::scalar(ValueTag::Bool);

    /// Nullable `externref` storage, represented in Rust by an owned table slot.
    ///
    /// Even a null external reference receives a nonzero handle; zero is not
    /// an external-reference root.
    pub const EXTERNREF: Self = Self::scalar(ValueTag::ExternRef);

    const fn scalar(tag: ValueTag) -> Self {
        Self {
            tag,
            target: core::ptr::null(),
        }
    }

    /// Returns whether Wasm permits zero or null initialization of this type.
    ///
    /// Non-null struct and array references require an explicit value. This
    /// describes the Wasm storage default, which need not equal a custom Rust
    /// wrapper's `Default` implementation.
    pub const fn is_defaultable(self) -> bool {
        matches!(
            self.tag,
            ValueTag::I8
                | ValueTag::I16
                | ValueTag::I32
                | ValueTag::I64
                | ValueTag::F32
                | ValueTag::F64
                | ValueTag::Bool
                | ValueTag::NullableRef
                | ValueTag::NullableArray
                | ValueTag::ExternRef
        )
    }

    /// Describes a reference to the struct schema at `target`.
    ///
    /// `nullable` controls whether a zero Rust handle represents null. A
    /// non-null handle must name an object of the target schema. The descriptor
    /// is read after linking; this function does not dereference `target`.
    ///
    /// For ordinary Rust wrappers, reuse `<Gc<T> as GcValue>::KIND` or
    /// `<Option<Gc<T>> as GcValue>::KIND` instead of assembling a descriptor.
    pub const fn reference(target: *const Schema, nullable: bool) -> Self {
        Self {
            tag: if nullable {
                ValueTag::NullableRef
            } else {
                ValueTag::Ref
            },
            target,
        }
    }

    /// Describes a reference to the array schema at `target`.
    ///
    /// The target must describe exactly one element type. Nullability and
    /// handle representation follow [`reference`](Self::reference).
    pub const fn array(target: *const Schema, nullable: bool) -> Self {
        Self {
            tag: if nullable {
                ValueTag::NullableArray
            } else {
                ValueTag::Array
            },
            target,
        }
    }
}

/// The aggregate declaration represented by a [`Schema`].
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchemaKind {
    /// A struct with an ordered sequence of mutable fields.
    Struct = 0,
    /// An array with one mutable element storage type.
    Array = 1,
}

impl SchemaKind {
    /// Returns the aggregate kind with this protocol value, or `None` if unknown.
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Struct),
            1 => Some(Self::Array),
            _ => None,
        }
    }
}

/// The header of a struct or array declaration.
///
/// [`SchemaData`] stores its field descriptors inline after this header.
/// References between schemas may be recursive; the postprocessor emits
/// mutually dependent declarations in the same Wasm recursion group.
#[repr(C)]
pub struct Schema {
    /// The descriptor signature, initialized to `SCHEMA_MAGIC`.
    pub magic: [u8; MAGIC_LEN],
    /// Whether the inline values describe struct fields or an array element.
    pub kind: SchemaKind,
    /// The inline descriptor count; one for arrays.
    pub field_count: u32,
}

/// A declaration and its field descriptions in one metadata allocation.
#[repr(C)]
pub struct SchemaData<const N: usize> {
    /// The declaration header at the start of this allocation.
    pub header: Schema,
    /// Field descriptions in declaration order, or one array element type.
    pub fields: [ValueKind; N],
}

impl<const N: usize> SchemaData<N> {
    /// Describes a struct with these fields in declaration order.
    ///
    /// # Panics
    ///
    /// Panics if the field count exceeds `u32::MAX`.
    pub const fn new(fields: [ValueKind; N]) -> Self {
        assert!(N <= u32::MAX as usize);
        Self {
            header: Schema {
                magic: SCHEMA_MAGIC,
                kind: SchemaKind::Struct,
                field_count: N as u32,
            },
            fields,
        }
    }
}

impl SchemaData<1> {
    /// Describes an array with the supplied element storage type.
    pub const fn array(element: ValueKind) -> Self {
        Self {
            header: Schema {
                magic: SCHEMA_MAGIC,
                kind: SchemaKind::Array,
                field_count: 1,
            },
            fields: [element],
        }
    }
}

/// A request to replace one Rust shim with a generated GC operation.
///
/// A marker call keeps this descriptor reachable from the shim's body. The
/// decoder discovers the operation by that body and its descriptor address;
/// it does not resolve the stored function pointer through the function table.
#[repr(C)]
pub struct Operation {
    /// The descriptor signature, initialized to `OPERATION_MAGIC`.
    pub magic: [u8; MAGIC_LEN],
    /// The shim address retained to keep its callable ABI observable to the compiler.
    ///
    /// Rust function pointers become function-table slots in the linked Wasm.
    /// Such a slot is distinct from the module function index used by `call`.
    pub function: *const (),
    /// The linked address of the operation owner schema.
    pub schema: *const Schema,
    /// The operation to emit in place of the shim body.
    pub kind: OperationKind,
    /// The zero-based struct field index, or zero for other operations.
    pub field_index: u32,
}

impl Operation {
    /// Binds a shim address to its schema, operation, and optional field index.
    pub const fn new(
        schema: *const Schema,
        kind: OperationKind,
        field_index: u32,
        function: *const (),
    ) -> Self {
        Self {
            magic: OPERATION_MAGIC,
            function,
            schema,
            kind,
            field_index,
        }
    }
}

/// The header of a native struct constructor, getter, or setter export.
///
/// The postprocessor generates this function directly from the schema.
/// Its parameters and result use Wasm values, including native references,
/// rather than the numeric handle ABI used by Rust shims. [`StructData`]
/// stores the export name inline after this header.
#[repr(C)]
pub struct Struct {
    /// The descriptor signature, initialized to `STRUCT_MAGIC`.
    pub magic: [u8; MAGIC_LEN],
    /// The linked address of the exported struct schema.
    pub schema: *const Schema,
    /// `StructNew`, `StructGet`, or `StructSet`.
    pub kind: OperationKind,
    /// The zero-based field index for accessors; zero for constructors.
    pub field: u32,
    /// The byte length of the export name.
    pub name_len: u32,
}

/// A struct export request and its name in one metadata allocation.
#[repr(C)]
pub struct StructData<const N: usize> {
    /// The export header at the start of this allocation.
    pub header: Struct,
    /// The UTF-8 export name.
    pub name: [u8; N],
}

impl<const N: usize> StructData<N> {
    /// Requests an operation exported under `name`.
    ///
    /// Constructors use [`OperationKind::StructNew`] and field index zero.
    /// Accessors use a valid field index in the target struct schema.
    ///
    /// # Panics
    ///
    /// Panics if the export name exceeds `u32::MAX` bytes.
    pub const fn new(
        schema: *const Schema,
        kind: OperationKind,
        field: u32,
        name: [u8; N],
    ) -> Self {
        assert!(N <= u32::MAX as usize);
        Self {
            header: Struct {
                magic: STRUCT_MAGIC,
                schema,
                kind,
                field,
                name_len: N as u32,
            },
            name,
        }
    }
}

/// The direction in which a native function bridge crosses the Rust ABI.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionKind {
    /// The host calls a Rust function using its native Wasm signature.
    Export = 0,
    /// Rust calls a host function through a generated numeric-value adapter.
    Import = 1,
}

impl FunctionKind {
    /// Returns the direction with this protocol value, or `None` if it is unknown.
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Export),
            1 => Some(Self::Import),
            _ => None,
        }
    }
}

/// The header of a native signature associated with a numeric Rust bridge.
///
/// Exported bridges and signature-address globals have matching temporary
/// names. Import wrappers instead refer to this descriptor through a marker
/// call, allowing the linker to discard unused imports. The postprocessor
/// replaces these markers and temporary exports with native signatures.
/// [`FunctionData`] stores parameter types, result types, and names inline.
#[repr(C)]
pub struct Function {
    /// The descriptor signature, initialized to `FUNCTION_MAGIC`.
    pub magic: [u8; MAGIC_LEN],
    /// Whether the native function imports from or exports to the host.
    pub kind: FunctionKind,
    /// The number of inline parameter descriptors.
    pub param_count: u32,
    /// The number of inline result descriptors, which is zero or one.
    pub result_count: u32,
    /// The byte length of the import module name.
    pub module_len: u32,
    /// The byte length of the native function name.
    pub name_len: u32,
}

/// A function signature and its names in one metadata allocation.
#[repr(C)]
pub struct FunctionData<const P: usize, const R: usize, const M: usize, const N: usize> {
    /// The signature header at the start of this allocation.
    pub header: Function,
    /// Parameter descriptions in argument order.
    pub params: [ValueKind; P],
    /// Zero or one result description.
    pub result: [ValueKind; R],
    /// The UTF-8 host module name; empty for exports.
    pub module: [u8; M],
    /// The UTF-8 function name.
    pub name: [u8; N],
}

impl<const P: usize, const R: usize, const M: usize, const N: usize> FunctionData<P, R, M, N> {
    /// Describes a function with these parameters and at most one result.
    ///
    /// `module` is the host module name for imports and is empty for exports.
    /// The numeric wrapper and this native signature must describe the same
    /// values, with reference handles converted at the boundary.
    ///
    /// # Panics
    ///
    /// Panics if there is more than one result, or the parameter count or either
    /// name length exceeds `u32::MAX`.
    pub const fn new(
        kind: FunctionKind,
        params: [ValueKind; P],
        result: [ValueKind; R],
        module: [u8; M],
        name: [u8; N],
    ) -> Self {
        assert!(P <= u32::MAX as usize);
        assert!(R <= 1);
        assert!(M <= u32::MAX as usize);
        assert!(N <= u32::MAX as usize);
        Self {
            header: Function {
                magic: FUNCTION_MAGIC,
                kind,
                param_count: P as u32,
                result_count: R as u32,
                module_len: M as u32,
                name_len: N as u32,
            },
            params,
            result,
            module,
            name,
        }
    }
}

// SAFETY: This descriptor stores a tag and an opaque pointer. Sharing it never
// dereferences the target.
unsafe impl Sync for ValueKind {}

// SAFETY: The function and schema pointers are metadata. Sharing this descriptor
// neither calls the function nor dereferences the schema.
unsafe impl Sync for Operation {}

// SAFETY: The schema pointer is opaque metadata. No safe operation on this
// descriptor dereferences it.
unsafe impl Sync for Struct {}

// Check the target C layouts at compile time so descriptor reads use rustc's
// actual offsets, including pointer-width-dependent padding.
const _: () = {
    use core::mem::{align_of, offset_of, size_of};

    const fn align_up(size: usize, alignment: usize) -> usize {
        size.next_multiple_of(alignment)
    }

    let layout = match size_of::<*const ()>() {
        4 => Layout::WASM32,
        8 => Layout::WASM64,
        _ => panic!("unsupported descriptor pointer width"),
    };
    assert!(size_of::<extern "C" fn()>() == layout.pointer_size as usize);
    assert!(size_of::<ValueKind>() == layout.value_kind_size as usize);
    assert!(offset_of!(ValueKind, target) == layout.value_kind_target_offset as usize);
    assert!(size_of::<Schema>() == layout.schema_size as usize);
    assert!(offset_of!(Schema, magic) == layout.schema_magic_offset as usize);
    assert!(offset_of!(Schema, kind) == layout.schema_kind_offset as usize);
    assert!(offset_of!(Schema, field_count) == layout.schema_field_count_offset as usize);
    assert!(offset_of!(SchemaData<0>, header) == 0);
    assert!(offset_of!(SchemaData<0>, fields) == layout.schema_fields_offset as usize);
    assert!(offset_of!(SchemaData<3>, fields) == layout.schema_fields_offset as usize);
    assert!(size_of::<SchemaData<0>>() == layout.schema_fields_offset as usize);
    assert!(
        size_of::<SchemaData<3>>()
            == layout.schema_fields_offset as usize + 3 * layout.value_kind_size as usize
    );
    assert!(size_of::<Operation>() == layout.operation_size as usize);
    assert!(offset_of!(Operation, function) == layout.operation_function_offset as usize);
    assert!(size_of::<Struct>() == layout.struct_export_size as usize);
    assert!(offset_of!(Struct, magic) == layout.struct_export_magic_offset as usize);
    assert!(offset_of!(Struct, schema) == layout.struct_export_schema_offset as usize);
    assert!(offset_of!(Struct, kind) == layout.struct_export_kind_offset as usize);
    assert!(offset_of!(Struct, field) == layout.struct_export_field_offset as usize);
    assert!(offset_of!(Struct, name_len) == layout.struct_export_name_len_offset as usize);
    assert!(offset_of!(StructData<0>, header) == 0);
    assert!(offset_of!(StructData<0>, name) == layout.struct_export_name_offset as usize);
    assert!(offset_of!(StructData<5>, name) == layout.struct_export_name_offset as usize);
    assert!(size_of::<StructData<0>>() == layout.struct_export_size as usize);
    assert!(
        size_of::<StructData<5>>()
            == align_up(
                layout.struct_export_name_offset as usize + 5,
                align_of::<StructData<5>>()
            )
    );
    assert!(size_of::<Function>() == layout.function_size as usize);
    assert!(offset_of!(Function, magic) == layout.function_magic_offset as usize);
    assert!(offset_of!(Function, kind) == layout.function_kind_offset as usize);
    assert!(offset_of!(Function, param_count) == layout.function_param_count_offset as usize);
    assert!(offset_of!(Function, result_count) == layout.function_result_count_offset as usize);
    assert!(offset_of!(Function, module_len) == layout.function_module_len_offset as usize);
    assert!(offset_of!(Function, name_len) == layout.function_name_len_offset as usize);

    type EmptyFunction = FunctionData<0, 0, 0, 0>;
    type NamedFunction = FunctionData<2, 1, 3, 5>;
    type VoidFunction = FunctionData<1, 0, 0, 1>;
    let params = layout.function_params_offset as usize;
    let value_size = layout.value_kind_size as usize;
    assert!(offset_of!(EmptyFunction, header) == 0);
    assert!(offset_of!(EmptyFunction, params) == params);
    assert!(offset_of!(EmptyFunction, result) == params);
    assert!(offset_of!(EmptyFunction, module) == params);
    assert!(offset_of!(EmptyFunction, name) == params);
    assert!(size_of::<EmptyFunction>() == params);
    assert!(offset_of!(NamedFunction, params) == params);
    assert!(offset_of!(NamedFunction, result) == params + 2 * value_size);
    assert!(offset_of!(NamedFunction, module) == params + 3 * value_size);
    assert!(offset_of!(NamedFunction, name) == params + 3 * value_size + 3);
    assert!(
        size_of::<NamedFunction>()
            == align_up(params + 3 * value_size + 3 + 5, align_of::<NamedFunction>())
    );
    assert!(offset_of!(VoidFunction, result) == params + value_size);
    assert!(offset_of!(VoidFunction, module) == params + value_size);
    assert!(offset_of!(VoidFunction, name) == params + value_size);
    assert!(
        size_of::<VoidFunction>() == align_up(params + value_size + 1, align_of::<VoidFunction>())
    );
};
