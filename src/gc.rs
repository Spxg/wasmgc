use core::{fmt, marker::PhantomData, mem::ManuallyDrop, num::NonZeroU32, ops::Deref};

use crate::{
    __private, GcValue, abi,
    schema::{SchemaKind, ValueKind},
};

/// A type describing a struct or array on the WasmGC heap.
///
/// [`Gc<T>`] uses this description to own and pass references to objects of type
/// `T`. Structs receive an implementation through [`GcStruct`]; [`crate::Array`]
/// implements it for arrays. To define a struct, use the [`macro@crate::wasmgc`]
/// attribute rather than implementing this trait directly.
///
/// See [`Gc`] for an example of constructing and sharing an object.
///
/// # Safety
///
/// `KIND` must identify the aggregate described by `SCHEMA`. The schema, its
/// field descriptions, and every referenced schema must be immutable, valid
/// metadata with static lifetimes. They must describe exactly the heap type
/// used by every operation on `Gc<Self>`, including field order, storage kinds,
/// reference targets, and nullability. The postprocessor relies on this metadata
/// when replacing numeric handles with typed Wasm references.
pub unsafe trait GcHeapObj: Sized + 'static {
    /// The aggregate kind described by [`Self::SCHEMA`].
    const KIND: __private::SchemaKind;

    /// The schema used to generate this type's WasmGC representation.
    ///
    /// The address refers to metadata in the current Wasm artifact. It is not a
    /// stable type identifier across compilations.
    const SCHEMA: *const __private::Schema;
}

/// Construction data and field access for a WasmGC struct.
///
/// The [`macro@crate::wasmgc`] attribute implements this trait for a concrete
/// struct with named fields. Each field must implement [`GcValue`]. A Rust value
/// of the struct supplies the initial fields to [`Gc::new`]; the resulting
/// [`Gc<Self>`] owns the managed object.
///
/// The macro also generates a view with a getter and a `set_<field>` method for
/// each field. Accessors have the same visibility as their fields. Getters copy
/// values out of GC storage; getters for references create new owned roots.
/// No accessor returns a Rust reference into the managed object.
///
/// See [`Gc`] for a complete example.
///
/// # Safety
///
/// `SCHEMA` must describe a struct. It and every generated operation must agree
/// on the field order, storage kinds, reference targets, nullability, and ABI.
/// The schema and all metadata reachable from it must remain valid and immutable
/// for the lifetime of the program.
///
/// `allocate` must return ownership of a new, nonzero root slot in the current
/// Wasm instance, referring to an object of this schema. It must retain the
/// construction fields until all their values have been stored. Stored values
/// must remain valid after those Rust fields are dropped; collection of the
/// managed object does not run Rust destructors.
///
/// `view` must return a valid shared reference for the entire borrow of `root`.
/// It must neither take ownership of nor release that root, and safe operations
/// on the view must preserve the root's type and ownership invariants.
pub unsafe trait GcStruct: Sized + 'static {
    /// The field schema shared by construction and accessor operations.
    ///
    /// The macro emits static metadata and lets Rust resolve references between
    /// field types, including recursive references.
    const SCHEMA: *const __private::Schema;

    /// The field-access type borrowed by [`Gc::view`] and [`Deref`].
    ///
    /// The generated view transparently wraps `Gc<Self>`. Borrowing it does not
    /// allocate a root or move the object.
    type View;

    /// Consumes the initial fields and returns an owned root for a new object.
    ///
    /// The recipient must eventually release the root. Use [`Gc::new`] to manage
    /// that ownership automatically.
    fn allocate(self) -> u32;

    /// Borrows the field-access view of `root` without creating another root.
    ///
    /// Individual reference getters may create roots for their returned values.
    fn view(root: &Gc<Self>) -> &Self::View;
}

// SAFETY: GcStruct requires its static schema and operations to describe
// the same struct, which is exactly the Struct heap-object contract.
unsafe impl<T: GcStruct> GcHeapObj for T {
    const KIND: __private::SchemaKind = SchemaKind::Struct;

    const SCHEMA: *const __private::Schema = <T as GcStruct>::SCHEMA;
}

/// An owned root to a struct or array on the WasmGC heap.
///
/// `T` describes the object's heap type. [`Gc::new`] constructs structs;
/// [`crate::GcArray`] provides array constructors and operations. A root keeps
/// its object and the graph reachable from it alive.
///
/// Cloning a `Gc` creates another root to the same object. Mutations are visible
/// through every alias. Dropping a `Gc` releases its root; the engine can collect
/// the object once no roots, host references, or reachable GC objects retain it.
/// Forgetting a `Gc` leaks its root.
///
/// Struct roots dereference to the field-access view generated by
/// [`macro@crate::wasmgc`]. Their equality compares object identity. Array roots
/// compare element values; use [`Gc::ref_eq`] to compare array identity.
/// Roots belong to one Wasm instance and are neither [`Send`] nor [`Sync`].
///
/// # Examples
///
/// Compile this example for a Wasm target and postprocess the resulting module
/// with `wasmgc` before executing it.
///
/// ```no_run
/// use wasmgc::{Gc, wasmgc};
///
/// #[wasmgc]
/// struct Counter {
///     value: i32,
/// }
///
/// let counter = Gc::new(Counter { value: 1 });
/// let alias = counter.clone();
/// alias.set_value(2);
/// assert_eq!(counter.value(), 2);
/// assert!(Gc::ref_eq(&counter, &alias));
///
/// drop(counter);
/// assert_eq!(alias.value(), 2);
/// ```
#[must_use]
pub struct Gc<T: GcHeapObj> {
    handle: NonZeroU32,
    // The handle describes T; it does not own a Rust T in linear memory.
    marker: PhantomData<fn() -> T>,
    // Root indices cannot be sent to another instance or shared across threads.
    instance_local: PhantomData<*mut ()>,
}

impl<T: GcStruct> Gc<T> {
    /// Creates a struct object from `value` using `struct.new`.
    ///
    /// Construction copies each field into GC storage and then drops the Rust
    /// construction fields. Reference fields retain their objects through GC
    /// edges, independently of those initial root handles.
    ///
    /// See [`Gc`] for an example.
    pub fn new(value: T) -> Self {
        let handle = value.allocate();
        // SAFETY: GcStruct::allocate transfers one live, nonzero root of T.
        unsafe { Self::from_raw(handle) }
    }

    /// Borrows the generated field-access view.
    ///
    /// Usually accessors are available through [`Deref`]. Call this explicitly
    /// when an accessor has the same name as a `Gc` method.
    pub fn view(&self) -> &T::View {
        T::view(self)
    }
}

impl<T: GcHeapObj> Gc<T> {
    /// Returns the root slot without transferring ownership.
    ///
    /// The number remains usable only while this root is alive. Released slots
    /// may be reused, so handles are not object identities.
    ///
    /// The postprocessed export `wasmgc.ref_get(handle)` retrieves the underlying
    /// GC reference without releasing this root. A host that retains the returned
    /// reference keeps the object alive independently.
    pub fn as_raw(&self) -> u32 {
        self.handle.get()
    }

    /// Transfers ownership of the root slot to the caller.
    ///
    /// The slot stays allocated until its recipient releases it. To recover Rust
    /// ownership, pass the result to [`Self::from_raw`] exactly once.
    ///
    /// # Examples
    ///
    /// Compile for a Wasm target and postprocess before execution.
    ///
    /// ```no_run
    /// use wasmgc::GcArray;
    ///
    /// let array = GcArray::<i32>::new_default(3);
    /// let handle = array.into_raw();
    /// // SAFETY: into_raw transferred this live array root, and it has no other owner.
    /// let array = unsafe { GcArray::<i32>::from_raw(handle) };
    /// assert_eq!(array.len(), 3);
    /// ```
    pub fn into_raw(self) -> u32 {
        ManuallyDrop::new(self).handle.get()
    }

    /// Takes ownership of a root slot.
    ///
    /// See [`Self::into_raw`] for a round-trip example.
    ///
    /// # Safety
    ///
    /// `handle` must be a nonzero, live slot in this Wasm instance's GC root
    /// table, containing an object of heap type `T`. The caller must transfer
    /// exclusive ownership of the slot: no other owner may release it, and this
    /// function must not be called again for the same ownership transfer.
    ///
    /// A handle borrowed from [`Self::as_raw`], a released handle, an externref
    /// handle, or a handle from another Wasm instance does not meet this contract.
    pub unsafe fn from_raw(handle: u32) -> Self {
        Self {
            handle: NonZeroU32::new(handle).expect("wasmgc: a root cannot be null"),
            marker: PhantomData,
            instance_local: PhantomData,
        }
    }

    /// Returns whether both roots refer to the same object, using `ref.eq`.
    ///
    /// Cloned roots compare equal even though they occupy different slots.
    /// This also compares array identity independently of array contents.
    /// See [`Gc`] for an example.
    pub fn ref_eq(this: &Self, other: &Self) -> bool {
        // SAFETY: Both borrows retain live GC slots from this instance through the call.
        unsafe { abi::ref_eq(this.handle.get(), other.handle.get()) != 0 }
    }
}

impl<T: GcStruct> Deref for Gc<T> {
    type Target = T::View;

    fn deref(&self) -> &Self::Target {
        T::view(self)
    }
}

impl<T: GcHeapObj> Clone for Gc<T> {
    fn clone(&self) -> Self {
        // SAFETY: self retains the source slot while ref_clone allocates a distinct
        // slot to the same T object and transfers ownership of that slot.
        unsafe { Self::from_raw(abi::ref_clone(self.handle.get())) }
    }
}

impl<T: GcHeapObj> Drop for Gc<T> {
    fn drop(&mut self) {
        // SAFETY: This Gc owns the slot exclusively; Drop runs once and cannot
        // leave a usable owner of the released slot.
        unsafe { abi::ref_release(self.handle.get()) }
    }
}

impl<T: GcStruct> PartialEq for Gc<T> {
    fn eq(&self, other: &Self) -> bool {
        Self::ref_eq(self, other)
    }
}

impl<T: GcStruct> Eq for Gc<T> {}

impl<T: GcStruct> fmt::Debug for Gc<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Gc")
            .field("type", &core::any::type_name::<T>())
            .field("root", &self.handle)
            .finish()
    }
}

// SAFETY: KIND carries T's heap schema and uses the u32 GC-root ABI.
// Borrowed conversions retain this slot, into_abi transfers its ownership,
// and from_abi takes the caller's owned non-null T slot exactly once.
unsafe impl<T: GcHeapObj> GcValue for Gc<T> {
    type Abi = u32;

    type SetterValue<'a> = &'a Self;

    const KIND: ValueKind = reference_kind::<T>(false);

    fn to_abi(&self) -> u32 {
        self.as_raw()
    }

    fn setter_abi(value: &&Self) -> u32 {
        value.as_raw()
    }

    fn into_abi(self) -> u32 {
        Gc::into_raw(self)
    }

    unsafe fn from_abi(value: u32) -> Self {
        // SAFETY: GcValue::from_abi requires the caller to transfer a live,
        // nonzero GC root of the declared heap type T.
        unsafe { Self::from_raw(value) }
    }
}

// SAFETY: The nullable kind uses zero for None and T's exact schema for
// nonzero GC slots. Only Some transfers or acquires a slot; borrowed encodings
// keep that slot owned by the source.
unsafe impl<T: GcHeapObj> GcValue for Option<Gc<T>> {
    type Abi = u32;

    type SetterValue<'a> = Option<&'a Gc<T>>;

    const KIND: ValueKind = reference_kind::<T>(true);

    fn to_abi(&self) -> u32 {
        self.as_ref().map_or(0, Gc::as_raw)
    }

    fn setter_abi(value: &Option<&Gc<T>>) -> u32 {
        value.map_or(0, Gc::as_raw)
    }

    fn into_abi(self) -> u32 {
        self.map_or(0, Gc::into_raw)
    }

    unsafe fn from_abi(value: u32) -> Self {
        if value == 0 {
            None
        } else {
            // SAFETY: This branch excludes null; from_abi's caller transfers
            // exclusive ownership of a live T root in the current instance.
            Some(unsafe { Gc::from_raw(value) })
        }
    }
}

const fn reference_kind<T: GcHeapObj>(nullable: bool) -> ValueKind {
    match T::KIND {
        SchemaKind::Struct => ValueKind::reference(T::SCHEMA, nullable),
        SchemaKind::Array => ValueKind::array(T::SCHEMA, nullable),
    }
}
