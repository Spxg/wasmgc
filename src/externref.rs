use core::{fmt, marker::PhantomData, mem::ManuallyDrop, num::NonZeroU32};

use crate::{GcValue, abi, schema::ValueKind};

/// An owned root to a host value represented by Wasm `externref`.
///
/// An `ExternRef` keeps its host value alive while Rust stores or passes it.
/// Cloning creates a second root to the same value; dropping a root releases
/// only that root. GC fields and arrays store the actual external reference,
/// so dropping the input root after a write does not invalidate the stored value.
///
/// The [`macro@crate::wasmgc`] attribute converts external references at function
/// boundaries. Host code passes and receives the value itself, rather than a
/// numeric handle. This type does not provide methods for inspecting or invoking
/// the host value; declare host operations in an `extern "wasmgc"` block.
///
/// Every root has a nonzero handle, including a root containing host `null`.
/// External-reference handles use a separate table from [`crate::Gc`] handles.
/// They belong to one Wasm instance and are neither [`Send`] nor [`Sync`].
///
/// # Examples
///
/// After Wasm compilation and postprocessing, `identity` accepts and returns
/// the host value directly. For JavaScript, calling it with an object returns
/// the same object.
///
/// ```no_run
/// use wasmgc::{ExternRef, wasmgc};
///
/// #[wasmgc]
/// pub fn identity(value: ExternRef) -> ExternRef {
///     value
/// }
/// ```
///
/// # Raw host access
///
/// When the module uses external-reference root slots, the postprocessor exports:
///
/// - `wasmgc.externref_new(value)`: allocates an owned root and returns its handle.
/// - `wasmgc.externref_get(handle)`: reads a value without releasing its root.
/// - `wasmgc.externref_take(handle)`: returns the value and releases its root.
///
/// Use these exports only when working with raw handles. `externref_take` must
/// receive ownership transferred by [`Self::into_raw`] or `externref_new`;
/// consuming a handle borrowed from [`Self::as_raw`] invalidates its Rust owner.
#[must_use]
pub struct ExternRef {
    handle: NonZeroU32,
    local: PhantomData<*mut ()>,
}

impl ExternRef {
    /// Creates a root containing host `null`, using `ref.null extern`.
    ///
    /// This allocates a root slot even though the stored value is null.
    ///
    /// # Examples
    ///
    /// Compile for a Wasm target and postprocess before execution.
    ///
    /// ```no_run
    /// use wasmgc::ExternRef;
    ///
    /// let value = ExternRef::null();
    /// assert!(value.is_null());
    /// assert_ne!(value.as_raw(), 0);
    /// ```
    pub fn null() -> Self {
        // SAFETY: externref_null allocates and transfers a new nonzero slot
        // in this instance's external-reference table, containing host null.
        unsafe { Self::from_raw(abi::externref_null()) }
    }

    /// Returns whether the host value is null, using `ref.is_null`.
    ///
    /// See [`Self::null`] for an example.
    pub fn is_null(&self) -> bool {
        // SAFETY: The shared borrow keeps this owned external-reference slot live.
        unsafe { abi::externref_is_null(self.as_raw()) != 0 }
    }
}

impl ExternRef {
    /// Returns the external-reference slot without transferring ownership.
    ///
    /// The handle is valid only while this root is alive. It identifies a root
    /// slot, not the host value, and may be reused after the root is released.
    #[inline]
    pub fn as_raw(&self) -> u32 {
        self.handle.get()
    }

    /// Transfers ownership of the root slot to the caller.
    ///
    /// The recipient must eventually release it, either by recovering Rust
    /// ownership with [`Self::from_raw`] or by calling the postprocessed export
    /// `wasmgc.externref_take(handle)`.
    #[inline]
    pub fn into_raw(self) -> u32 {
        ManuallyDrop::new(self).handle.get()
    }

    /// Takes ownership of an external-reference slot.
    ///
    /// # Safety
    ///
    /// `handle` must be a live, nonzero slot in this Wasm instance's
    /// external-reference table. Its owner must transfer exclusive ownership:
    /// no other owner may release it, and the transfer may be consumed only once.
    /// Such ownership can come from [`Self::into_raw`], a generated ABI adapter,
    /// or the postprocessed export `wasmgc.externref_new(value)`.
    ///
    /// GC-object handles, borrowed handles, released handles, and handles from
    /// other instances are invalid. A null host value still requires a nonzero
    /// external-reference handle.
    #[inline]
    pub unsafe fn from_raw(handle: u32) -> Self {
        Self {
            handle: NonZeroU32::new(handle).expect("wasmgc: an externref root cannot be null"),
            local: PhantomData,
        }
    }
}

impl Clone for ExternRef {
    fn clone(&self) -> Self {
        // SAFETY: self retains the source slot while externref_clone creates
        // a distinct owned slot containing the same external reference.
        unsafe { Self::from_raw(abi::externref_clone(self.as_raw())) }
    }
}

impl Drop for ExternRef {
    fn drop(&mut self) {
        // SAFETY: This wrapper exclusively owns its live external-reference slot,
        // and Drop consumes that ownership once.
        unsafe { abi::externref_release(self.as_raw()) }
    }
}

impl Default for ExternRef {
    fn default() -> Self {
        Self::null()
    }
}

impl fmt::Debug for ExternRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExternRef")
            .field("root", &self.handle)
            .finish()
    }
}

// SAFETY: EXTERNREF uses u32 handles into the external-reference table,
// including a nonzero handle for host null. Borrowed conversions retain their
// slot; into_abi transfers it, and from_abi consumes one transferred slot.
unsafe impl GcValue for ExternRef {
    type Abi = u32;

    type SetterValue<'a> = &'a Self;

    const KIND: ValueKind = ValueKind::EXTERNREF;

    #[inline]
    fn to_abi(&self) -> u32 {
        self.as_raw()
    }

    #[inline]
    fn setter_abi(value: &Self::SetterValue<'_>) -> u32 {
        value.as_raw()
    }

    #[inline]
    fn into_abi(self) -> u32 {
        self.into_raw()
    }

    #[inline]
    unsafe fn from_abi(value: u32) -> Self {
        // SAFETY: The GcValue caller transfers a live, nonzero owned slot
        // from this instance's external-reference table.
        unsafe { Self::from_raw(value) }
    }
}
