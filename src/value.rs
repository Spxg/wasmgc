use crate::{__private, abi, schema};

mod sealed {
    pub trait Abi {}
}

/// A sealed numeric ABI used by generated operation shims.
///
/// The implementations are `i32`, `u32`, `i64`, `f32`, and `f64`.
/// [`GcValue::Abi`] selects one of them; it does not use Rust's in-memory
/// representation of the Rust value type.
#[doc(hidden)]
pub trait GcAbi: sealed::Abi + Copy {
    /// Marks an operation shim for replacement by the postprocessor.
    ///
    /// # Safety
    ///
    /// `operation` must point to immutable static metadata for this exact shim,
    /// including its numeric parameter and result types. `arguments` must point
    /// to a live tuple retaining all shim parameters throughout the call.
    /// The containing Wasm function must be replaced before execution.
    #[doc(hidden)]
    unsafe fn marker(operation: *const u8, arguments: *const u8) -> Self;
}

macro_rules! abi {
    ($ty:ty, $marker:ident) => {
        impl sealed::Abi for $ty {}

        impl GcAbi for $ty {
            #[inline(always)]
            unsafe fn marker(operation: *const u8, arguments: *const u8) -> Self {
                // SAFETY: The caller supplies this shim's descriptor and live parameter
                // tuple; this implementation forwards them to the matching numeric marker.
                unsafe { abi::$marker(operation, arguments) as Self }
            }
        }
    };
}

abi!(i32, marker_i32);
abi!(u32, marker_i32);
abi!(i64, marker_i64);
abi!(f32, marker_f32);
abi!(f64, marker_f64);

/// A value storable in GC fields and arrays and passable across Wasm boundaries.
///
/// Implementations describe a Wasm storage kind and convert between Rust values
/// and the numeric ABI used by generated shims. The postprocessor replaces the
/// shims with GC instructions and host adapters.
///
/// The built-in scalar implementations are `i8`, `u8`, `i16`, `u16`, `i32`,
/// `u32`, `i64`, `u64`, `f32`, `f64`, and `bool`. Rooted values use [`crate::Gc<T>`],
/// `Option<Gc<T>>`, or [`crate::ExternRef`]. The [`crate::GcArray<T>`] alias is
/// also a supported reference value. A struct's Rust construction data
/// implements [`crate::GcStruct`]; its rooted reference implements `GcValue`.
///
/// Custom types can implement this trait by lowering to an existing storage kind.
/// GC storage cannot run Rust destructors or retain an ordinary Rust allocation
/// merely by storing its address.
///
/// # Safety
///
/// An implementation must uphold all of the following:
///
/// - `KIND` and `Abi` agree on storage and calling convention. Packed integers,
///   32-bit integers, and booleans use `i32` or `u32`; 64-bit integers use `i64`;
///   floats use their corresponding numeric type. References use `u32` roots
///   from the correct table, with the declared target schema and nullability.
/// - [`to_abi`](Self::to_abi) and [`setter_abi`](Self::setter_abi) produce valid
///   borrowed representations. Keeping the source value alive must keep every
///   required root or resource alive until the generated operation finishes.
///   These conversions must not consume a root still owned by their source.
/// - [`into_abi`](Self::into_abi) consumes the value. For a reference it must
///   transfer one live root with exclusive ownership, without releasing it.
///   The recipient can then decode the representation exactly once.
/// - [`from_abi`](Self::from_abi) accepts every value obtainable from the declared
///   storage or function boundary, including storage defaults and values not
///   produced by this implementation's encoding methods. It takes ownership of
///   a transferred reference root exactly once.
/// - Stored values remain valid after construction fields and setter values
///   are dropped. Nullable GC references encode null as zero. External references
///   use a nonzero owned slot even when their host value is null.
///
/// # Examples
///
/// A numeric wrapper can reuse `f64` storage without changing its Rust API.
/// Compile for a Wasm target and postprocess before execution.
///
/// ```no_run
/// use wasmgc::{Gc, GcValue, __private::ValueKind, wasmgc};
///
/// struct Distance(f64);
///
/// // SAFETY: Distance accepts every f64; its conversions use F64 storage and
/// // carry no roots or other resources across the numeric ABI.
/// unsafe impl GcValue for Distance {
///     type Abi = f64;
///     type SetterValue<'a> = Self;
///     const KIND: ValueKind = ValueKind::F64;
///
///     fn to_abi(&self) -> f64 { self.0 }
///
///     fn setter_abi(value: &Self) -> f64 { value.0 }
///
///     fn into_abi(self) -> f64 { self.0 }
///
///     unsafe fn from_abi(value: f64) -> Self { Self(value) }
/// }
///
/// #[wasmgc]
/// struct Position {
///     distance: Distance,
/// }
///
/// let position = Gc::new(Position { distance: Distance(1.0) });
/// position.set_distance(Distance(2.5));
/// assert_eq!(position.distance().0, 2.5);
/// ```
pub unsafe trait GcValue: Sized + 'static {
    /// The numeric representation used by generated Rust-to-Wasm shims.
    ///
    /// References use a `u32` root handle. The postprocessor converts that handle
    /// to or from the actual Wasm reference at the operation boundary.
    type Abi: GcAbi;

    /// The input accepted by generated field setters and single-value array writes.
    ///
    /// Scalars accept themselves. `Gc<T>` accepts `&'a Gc<T>`;
    /// `Option<Gc<T>>` accepts `Option<&'a Gc<T>>`. [`crate::ExternRef`] also borrows
    /// its root. The complete setter value stays alive until the write finishes.
    type SetterValue<'a>;

    /// The Wasm storage kind, including reference target and nullability.
    ///
    /// For custom implementations, choose a kind matching [`Self::Abi`] and the
    /// invariants in this trait's safety contract.
    const KIND: __private::ValueKind;

    /// Encodes a borrowed value without transferring ownership.
    ///
    /// Construction, slice copies, and imported function calls retain `self`
    /// until the generated operation has consumed this representation.
    fn to_abi(&self) -> Self::Abi;

    /// Encodes a borrowed setter value without transferring ownership.
    ///
    /// Generated setters and array writes retain `value` until the write finishes.
    fn setter_abi(value: &Self::SetterValue<'_>) -> Self::Abi;

    /// Consumes a value and transfers its representation to the ABI recipient.
    ///
    /// Exported functions use this for their return values. A reference
    /// implementation transfers an owned root rather than borrowing its handle.
    fn into_abi(self) -> Self::Abi;

    /// Decodes an ABI value, taking ownership of any transferred root.
    ///
    /// Generated getters and function adapters call this after producing a value
    /// of the declared storage kind.
    ///
    /// # Safety
    ///
    /// `value` must be a valid ABI result for [`Self::KIND`]. Every nonzero
    /// reference handle must identify a live root of the declared type in the
    /// correct root table of the current Wasm instance. The caller must transfer
    /// exclusive ownership of that root exactly once.
    ///
    /// Non-null GC references and all external-reference handles must be nonzero.
    /// Zero represents null only for a nullable GC-reference kind. Numeric kinds
    /// have no root-ownership requirement.
    unsafe fn from_abi(value: Self::Abi) -> Self;
}

macro_rules! scalar {
    ($ty:ty, $abi:ty, $kind:ident) => {
        // SAFETY: Each invocation pairs a built-in scalar with its Wasm storage
        // kind and numeric ABI. Casts implement integer truncation/bit interpretation;
        // all decoded bit patterns are valid scalars and carry no root ownership.
        unsafe impl GcValue for $ty {
            type Abi = $abi;

            type SetterValue<'a> = Self;

            const KIND: schema::ValueKind = schema::ValueKind::$kind;

            #[inline]
            fn to_abi(&self) -> Self::Abi {
                *self as $abi
            }

            #[inline]
            fn setter_abi(value: &Self) -> Self::Abi {
                *value as $abi
            }

            #[inline]
            fn into_abi(self) -> Self::Abi {
                self.to_abi()
            }

            #[inline]
            unsafe fn from_abi(value: Self::Abi) -> Self {
                value as Self
            }
        }
    };
}

scalar!(i8, i32, I8);
scalar!(u8, i32, I8);
scalar!(i16, i32, I16);
scalar!(u16, i32, I16);
scalar!(i32, i32, I32);
scalar!(u32, i32, I32);
scalar!(i64, i64, I64);
scalar!(u64, i64, I64);
scalar!(f32, f32, F32);
scalar!(f64, f64, F64);

// SAFETY: BOOL uses the i32 ABI. Encoding emits 0 or 1, and decoding maps
// every i32 to a valid bool without reinterpreting its Rust memory representation.
unsafe impl GcValue for bool {
    type Abi = i32;

    type SetterValue<'a> = bool;

    const KIND: schema::ValueKind = schema::ValueKind::BOOL;

    #[inline]
    fn to_abi(&self) -> i32 {
        i32::from(*self)
    }

    #[inline]
    fn setter_abi(value: &bool) -> i32 {
        i32::from(*value)
    }

    #[inline]
    fn into_abi(self) -> i32 {
        self.to_abi()
    }

    #[inline]
    unsafe fn from_abi(value: i32) -> Self {
        value != 0
    }
}
