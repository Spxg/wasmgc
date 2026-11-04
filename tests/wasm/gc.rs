use wasmgc::{Gc, GcArray, wasmgc};

use crate::Counter;

#[wasmgc]
pub struct GcCycle {
    #[getter]
    pub value: i32,
    #[getter]
    pub children: GcArray<Gc<GcCycle>>,
}

#[wasmgc]
pub fn gc_root_new(counter: Gc<Counter>) -> u32 {
    counter.into_raw()
}

/// Creates another root and leaves the original handle live.
///
/// # Safety
///
/// `handle` must transfer ownership of a live `Counter` root in this instance
/// for the call. Ownership of the original handle returns to the caller.
#[wasmgc]
pub unsafe fn gc_root_clone(handle: u32) -> u32 {
    // SAFETY: The caller transfers this live Counter root for the call.
    let root = unsafe { Gc::<Counter>::from_raw(handle) };
    let alias = root.clone();
    let _ = root.into_raw();
    alias.into_raw()
}

/// Releases the root transferred by the caller.
///
/// # Safety
///
/// `handle` must transfer ownership of a live `Counter` root in this instance.
/// It must not be used or released again after this call.
#[wasmgc]
pub unsafe fn gc_root_drop(handle: u32) {
    // SAFETY: The caller transfers sole ownership of this root handle.
    drop(unsafe { Gc::<Counter>::from_raw(handle) });
}

#[wasmgc]
pub fn gc_make_cycle() -> Gc<GcCycle> {
    let cycle = Gc::new(GcCycle {
        value: 7,
        children: GcArray::empty(),
    });
    let children = GcArray::from_slice([cycle.clone()]);
    cycle.set_children(&children);
    cycle
}

#[wasmgc]
pub fn gc_make_array() -> GcArray<i32> {
    GcArray::from_slice([1, 2, 3])
}
