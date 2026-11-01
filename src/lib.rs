//! Rust-owned references to WebAssembly GC objects.
//!
//! Define structs with [`macro@wasmgc`], allocate them with [`Gc`], and store
//! sequences in [`GcArray`]. Objects live in the engine's GC heap; Rust values
//! own the roots that keep those objects reachable. [`ExternRef`] stores host
//! values such as JavaScript objects alongside GC references and scalars.
//!
//! The runtime is intended for `wasm32-unknown-unknown` and
//! `wasm64-unknown-unknown`. Compile the application to Wasm, then run the
//! `wasmgc` post-link tool before instantiating it in an engine with WasmGC
//! support. The tool supplies the GC instructions and reference adapters that
//! ordinary Rust compilation cannot express directly.
//!
//! # Example
//!
//! A cloned root observes writes to the same counter:
//!
//! ```no_run
//! use wasmgc::{Gc, wasmgc};
//!
//! #[wasmgc]
//! struct Counter {
//!     value: i32,
//! }
//!
//! let counter = Gc::new(Counter { value: 1 });
//! let shared = counter.clone();
//! shared.set_value(2);
//! assert_eq!(counter.value(), 2);
//! assert!(Gc::ref_eq(&counter, &shared));
//! ```
//!
//! This example runs inside a post-processed Wasm module. It is not a native
//! host program.
//!
//! # Ownership and storage
//!
//! Moving a [`Gc`] transfers its root. Cloning allocates another root for the
//! same object, and dropping releases that root. An object becomes collectible
//! once no Rust root, host reference, or reachable GC object keeps it alive.
//! This permits cyclic object graphs without reference-counted cycles.
//!
//! A struct definition supplies construction values and a schema, rather than
//! an addressable Rust payload in the GC heap. Its generated getters copy
//! scalars or acquire owned references; setters borrow reference arguments
//! until the write completes. There is no `&T` into a GC object, and managed
//! objects do not run Rust destructors. Roots belong to their Wasm instance and
//! cannot be sent across threads.
//!
//! [`GcStruct`] describes a struct and its generated view. [`GcHeapObj`]
//! describes either a struct or an [`Array`] heap type. [`GcValue`] defines the
//! scalar and reference conversions used by fields, array elements, and
//! imported or exported functions. Most applications use the macro and the
//! built-in implementations of these traits.
//!
//! # Host functions
//!
//! Apply [`macro@wasmgc`] to a function to export it, or to an
//! `extern "wasmgc"` block to import host functions. After post-processing,
//! their reference parameters and results are actual Wasm references. The
//! macro documentation covers constructors, field exports, and host contracts.
//!
//! # Cargo features
//!
//! - **`alloc`** (enabled by default): adds conversions to and from `Vec`.
//!   Slice copies and GC allocations remain available without it.
//! - **`cli`**: enables the host-side `wasmgc::process` function and the
//!   `wasmgc` executable. It also enables `std` and the Wasm binary tools used
//!   by post-processing. Application dependencies normally leave it disabled.
//!
//! Without `cli`, this crate is `no_std`. It does not replace the Rust
//! allocator: Rust buffers and GC objects have separate storage and lifetimes.

#![cfg_attr(not(feature = "cli"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

mod array;
mod externref;
mod gc;
#[cfg(feature = "cli")]
mod postprocess;
mod schema;
mod value;

#[doc(hidden)]
pub mod __private {
    //! Definitions shared with generated code and custom value conversions.
    //!
    //! The public conversion contracts are documented on [`crate::GcValue`].

    pub use wasmgc_macros::BuildGcStruct;

    pub use crate::schema::{
        FUNCTION_BRIDGE_PREFIX, FUNCTION_DESC_PREFIX, Function, FunctionKind, Operation,
        OperationKind, STRUCT_EXPORT_DESC_PREFIX, Schema, SchemaKind, Struct, ValueKind, ValueTag,
    };
    pub use crate::value::GcAbi as Abi;

    pub use crate::abi::marker_void;

    /// Rejects managed struct definitions that implement Rust destruction.
    pub trait MustNotImplDrop {}

    // The derive emits another implementation for the schema type; coherence
    // rejects the overlap when that type also implements Drop.
    #[allow(drop_bounds)]
    impl<T: core::ops::Drop> MustNotImplDrop for T {}
}

pub use wasmgc_macros::wasmgc;

mod abi {
    //! Imports replaced by the post-link tool rather than supplied by the host.
    //!
    //! Marker calls keep descriptors and numeric parameters observable until
    //! the entire shim body is replaced. Their descriptor must name that shim;
    //! the argument tuple and its borrowed resources must survive the call.
    //!
    //! Reference operations accept live slots owned by this Wasm instance.
    //! Clone creates a distinct owned slot; release consumes one. GC and
    //! external-reference slots belong to separate tables and cannot be mixed.

    #[link(wasm_import_module = "wasmgc")]
    unsafe extern "C" {
        /// Marks a generated shim that has no return value.
        ///
        /// # Safety
        ///
        /// `operation` must identify this shim and its numeric parameter types.
        /// `arguments` must point to its complete parameter tuple, whose values
        /// and borrowed roots remain live through the call. The Wasm module
        /// must be post-processed before any such shim is executed.
        pub fn marker_void(operation: *const u8, arguments: *const u8);
        pub fn marker_i32(operation: *const u8, arguments: *const u8) -> i32;
        pub fn marker_i64(operation: *const u8, arguments: *const u8) -> i64;
        pub fn marker_f32(operation: *const u8, arguments: *const u8) -> f32;
        pub fn marker_f64(operation: *const u8, arguments: *const u8) -> f64;

        pub fn ref_clone(handle: u32) -> u32;
        pub fn ref_release(handle: u32);
        pub fn ref_eq(left: u32, right: u32) -> i32;

        pub fn externref_null() -> u32;
        pub fn externref_clone(handle: u32) -> u32;
        pub fn externref_is_null(handle: u32) -> i32;
        pub fn externref_release(handle: u32);
    }
}

pub use array::{Array, GcArray, GcArrayIter};
pub use externref::ExternRef;
pub use gc::{Gc, GcHeapObj, GcStruct};
#[cfg(feature = "cli")]
pub use postprocess::process;
pub use value::GcValue;
