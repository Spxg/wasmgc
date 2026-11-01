use wasmgc::{ExternRef, Gc, GcArray, wasmgc};

#[wasmgc]
struct HostValue {
    value: ExternRef,
}

#[wasmgc]
extern "wasmgc" {
    fn host_object() -> ExternRef;
    fn host_add(value: &ExternRef, delta: i32) -> i32;
    fn host_identity(value: &ExternRef) -> ExternRef;
}

#[wasmgc]
pub fn test_externref_null() {
    let value = ExternRef::null();

    assert!(value.is_null());
    assert_ne!(value.as_raw(), 0);

    let alias = value.clone();
    drop(value);
    assert!(alias.is_null());

    let handle = alias.into_raw();
    // SAFETY: into_raw transferred this live root, which is recovered exactly once.
    let value = unsafe { ExternRef::from_raw(handle) };
    assert!(value.is_null());
}

#[wasmgc]
pub fn test_externref_import() {
    // SAFETY: The runner supplies a counter object and matching host functions.
    unsafe {
        let value = host_object();
        let alias = host_identity(&value);

        assert!(!value.is_null());
        assert_eq!(host_add(&alias, 3), 3);
        assert_eq!(host_add(&value, 2), 5);

        drop(value);
        assert_eq!(host_add(&alias, 1), 6);
    }
}

#[wasmgc]
pub fn test_externref_field() {
    // SAFETY: Every host_add argument comes from the runner's host_object import.
    unsafe {
        let value = host_object();
        let holder = Gc::new(HostValue { value });
        let value = holder.value();
        assert_eq!(host_add(&value, 1), 1);

        let replacement = host_object();
        holder.set_value(&replacement);
        drop(replacement);

        let replacement = holder.value();
        drop(holder);
        assert_eq!(host_add(&replacement, 2), 2);
        assert_eq!(host_add(&value, 1), 2);
    }
}

#[wasmgc]
pub fn test_externref_array() {
    let values = GcArray::<ExternRef>::new_default(2);
    assert!(values.get(0).unwrap().is_null());
    assert!(values.get(1).unwrap().is_null());

    // SAFETY: Every host_add argument comes from the runner's host_object import.
    unsafe {
        let value = host_object();
        values.fill(&value);
        drop(value);

        let first = values.get(0).unwrap();
        let second = values.get(1).unwrap();
        drop(values);

        assert_eq!(host_add(&first, 1), 1);
        assert_eq!(host_add(&second, 2), 3);
    }
}

#[wasmgc]
pub fn identity_externref(value: ExternRef) -> ExternRef {
    value
}
