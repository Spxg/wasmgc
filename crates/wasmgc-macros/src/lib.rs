//! Attribute macros for the `wasmgc` runtime.
//!
//! Use [`wasmgc`] through the `wasmgc` crate, which supplies its generated
//! code with the matching runtime and schema definitions.

use proc_macro::TokenStream;
use syn::{Item, parse_macro_input};

mod function;
mod r#struct;

/// Defines a GC struct or a function interface for WebAssembly callers.
///
/// The compiled module must be processed by the `wasmgc` command before it is
/// instantiated. Processing replaces numeric Rust bridges with WasmGC
/// instructions and native WebAssembly reference parameters and results.
///
/// # Structs
///
/// On a struct with named fields, implements `GcStruct` and generates a
/// `TypeRef` view. A `Gc<Type>` dereferences to this view, whose methods read and
/// write the GC object's fields. The Rust struct supplies initial field values
/// to `Gc::new`; it is not the in-memory layout of the GC object.
///
/// Each field gets `field()` and `set_field(value)` methods with the field's
/// Rust visibility. Getters copy scalars or return a new root to the same
/// reference value. Setters take the field type's `GcValue::SetterValue`;
/// reference fields can therefore borrow an existing root.
///
/// The following attributes additionally expose operations to WebAssembly
/// callers:
///
/// - `#[wasmgc(constructor)]` exports `Type.new`, with one parameter per field
///   in declaration order.
/// - `#[getter]` on a field exports `Type.get_field`.
/// - `#[setter]` on a field exports `Type.set_field`.
///
/// These exports are generated during processing. They are independent of the
/// Rust methods: omitting `#[setter]`, for example, does not remove `set_field`
/// from the Rust view. Field exports are selected independently, and a
/// constructor export is optional even when Rust constructs instances.
///
/// ```no_run
/// use wasmgc::{Gc, wasmgc};
///
/// #[wasmgc(constructor)]
/// pub struct Counter {
///     #[getter]
///     #[setter]
///     pub value: i32,
/// }
///
/// let counter = Gc::new(Counter { value: 1 });
/// counter.set_value(counter.value() + 1);
/// assert_eq!(counter.value(), 2);
/// ```
///
/// After processing, JavaScript can use the same GC object directly:
///
/// ```javascript
/// const counter = instance.exports["Counter.new"](1);
/// instance.exports["Counter.set_value"](counter, 2);
/// console.log(instance.exports["Counter.get_value"](counter)); // 2
/// ```
///
/// Structs must be concrete, have named fields, and have no [`Drop`]
/// implementation. Every field must implement `GcValue`. Other attributes,
/// including derives and conditional field attributes, are preserved.
///
/// # Exported functions
///
/// On a free function, exports that function under its Rust name. The original
/// function remains callable from Rust. No `extern`, `no_mangle`, or
/// `export_name` attribute is needed.
///
/// ```no_run
/// use wasmgc::{GcArray, wasmgc};
///
/// #[wasmgc]
/// pub fn first(values: &GcArray<i32>) -> i32 {
///     values.get(0).unwrap_or(0)
/// }
/// ```
///
/// Parameters may be owned `GcValue` values or shared borrows of them. Results
/// must be owned `GcValue` values or `()`. Reference parameters and results
/// become native WebAssembly references at the host boundary; Rust root handles
/// remain internal to the module. An owned reference parameter owns a temporary
/// Rust root, while a borrowed parameter borrows one for the call.
///
/// Functions must be synchronous, non-generic free functions. Mutable borrows,
/// borrowed results, variadics, `const fn`, and diverging results are unsupported.
/// Export names do not include the Rust module path, so functions with the same
/// name cannot both be exported from one module. An unsafe export retains its
/// original safety requirements for host callers.
///
/// # Imported functions
///
/// On an `extern "wasmgc"` block, generates Rust wrappers for host functions
/// imported from the `env` module. The import name is the Rust function name.
/// `"wasmgc"` is consumed by this macro; it is not an ABI understood by rustc.
/// The block does not need an `unsafe` prefix, including in Rust 2024.
///
/// ```no_run
/// use wasmgc::{ExternRef, wasmgc};
///
/// #[wasmgc]
/// extern "wasmgc" {
///     /// Adds to a host counter and returns its updated value.
///     ///
///     /// # Safety
///     ///
///     /// `counter` must refer to an object whose `add` method accepts an i32,
///     /// returns an i32, and returns normally.
///     fn counter_add(counter: &ExternRef, delta: i32) -> i32;
/// }
/// ```
///
/// The host supplies the corresponding import when instantiating the module:
///
/// ```javascript
/// const { instance } = await WebAssembly.instantiate(bytes, {
///     env: { counter_add: (counter, delta) => counter.add(delta) },
/// });
/// ```
///
/// Imports are unsafe to call by default. Their declarations should document
/// any host preconditions in a `# Safety` section. Declaring an import as
/// `safe fn` asserts that the host implementation is safe for every argument
/// permitted by its Rust types. Host implementations must return normally;
/// unwinding through the generated bridges is unsupported, so exceptions must
/// be handled on the host side. Imported functions use the same parameter and
/// result restrictions as exports.
#[proc_macro_attribute]
pub fn wasmgc(arguments: TokenStream, input: TokenStream) -> TokenStream {
    let arguments = arguments.into();
    let result = match parse_macro_input!(input as Item) {
        Item::Struct(item) => r#struct::expand(arguments, item),
        item => function::expand(arguments, item),
    };
    match result {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

#[doc(hidden)]
#[proc_macro_derive(BuildGcStruct, attributes(constructor, getter, setter))]
pub fn build_gc_struct(input: TokenStream) -> TokenStream {
    match r#struct::derive(parse_macro_input!(input as syn::ItemStruct)) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}
