use core::{
    any::TypeId,
    fmt,
    iter::FusedIterator,
    marker::PhantomData,
    mem::MaybeUninit,
    ops::{Bound, RangeBounds},
};

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

use crate::{
    Gc, GcHeapObj, GcValue, abi,
    schema::{Operation, OperationKind, Schema, SchemaData, SchemaKind},
    value::GcAbi,
};

/// An owned root to a fixed-length, mutable WasmGC array.
///
/// This is an alias for [`Gc<Array<T>>`]. Elements implement [`GcValue`] and may
/// be scalars, GC references, nullable GC references, or [`crate::ExternRef`].
///
/// Cloning an array shares its object; writes through either root are visible
/// through both. Getters copy element values rather than returning Rust
/// references into the array. Reading a reference element creates an independent
/// root to the stored object. Array equality compares elements; [`Gc::ref_eq`]
/// compares the array objects themselves.
///
/// Copies between GC arrays use `array.copy`, including overlapping ranges.
/// Copies between GC storage and Rust slices transfer elements one at a time:
/// built-in scalars use a generated Wasm loop, while other types use their
/// [`GcValue`] conversions in batches. Neither path exposes GC storage as
/// linear memory. The `alloc` feature adds `to_vec` and conversion from `Vec<T>`.
///
/// Array lengths and indices are 32-bit on both wasm32 and wasm64. Constructors
/// reject lengths above `u32::MAX`; an engine may fail smaller allocations too.
/// Roots have the same ownership and instance restrictions as [`Gc`].
///
/// # Examples
///
/// Compile for a Wasm target and postprocess the resulting module with `wasmgc`
/// before executing this example.
///
/// ```no_run
/// use wasmgc::{Gc, GcArray};
///
/// let values = GcArray::from_slice([1i32, 2, 3]);
/// let alias = values.clone();
/// alias.set(1, 7);
/// assert_eq!(values.get(1), Some(7));
/// assert!(Gc::ref_eq(&values, &alias));
///
/// let copy = GcArray::<i32>::new_default(values.len());
/// copy.copy_from(0, &values, ..);
/// copy.set(1, 9);
/// assert_eq!(values.get(1), Some(7));
/// assert!(!Gc::ref_eq(&values, &copy));
/// ```
pub type GcArray<T> = Gc<Array<T>>;

/// The heap-type description for an array of [`GcValue`] elements.
///
/// Use [`GcArray<T>`] to construct and own an array object. `Array<T>` supplies
/// its element schema to [`GcHeapObj`]; it does not contain the elements in Rust
/// memory. See [`GcArray`] for an example.
pub struct Array<T: GcValue> {
    element: PhantomData<fn() -> T>,
}

// SAFETY: The promoted static schema contains one mutable element using
// T's declared storage kind. T's GcValue contract validates that storage and
// any referenced schema, and all ArrayOps<T> use this same schema.
unsafe impl<T: GcValue> GcHeapObj for Array<T> {
    const KIND: SchemaKind = SchemaKind::Array;

    const SCHEMA: *const Schema =
        (&SchemaData::array(T::KIND) as *const SchemaData<1>).cast::<Schema>();
}

const COPY_BATCH_SIZE: usize = 64;

// A read batch owns every initialized ABI result until take transfers it to T.
// The counters track root ownership as well as initialized numeric slots.
struct AbiValues<T: GcValue> {
    values: [MaybeUninit<T::Abi>; COPY_BATCH_SIZE],
    initialized: usize,
    consumed: usize,
}

impl<T: GcValue> AbiValues<T> {
    fn take(&mut self) -> T {
        let index = self.consumed;
        self.consumed += 1;
        // SAFETY: Callers take at most initialized entries, each written by the
        // read shim as a valid owned ABI result. Advancing consumed first prevents
        // Drop from decoding that same result again if conversion panics.
        unsafe { T::from_abi(self.values[index].assume_init_read()) }
    }
}

impl<T: GcValue> Drop for AbiValues<T> {
    fn drop(&mut self) {
        // An output destructor or conversion may panic partway through a batch.
        // Release the remaining owned results as well as the ones already returned.
        while self.consumed < self.initialized {
            drop(self.take());
        }
    }
}

fn wasm_len(value: usize) -> u32 {
    u32::try_from(value).expect("wasmgc: GC array length or offset exceeds u32::MAX")
}

fn check_range(offset: usize, count: usize, length: usize) {
    assert!(
        offset <= length && count <= length - offset,
        "wasmgc: buffer range out of bounds"
    );
}

fn range_bounds(range: impl RangeBounds<usize>, length: usize) -> (usize, usize) {
    let start = match range.start_bound() {
        Bound::Included(start) => *start,
        Bound::Excluded(start) => start.checked_add(1).expect("wasmgc: range start overflow"),
        Bound::Unbounded => 0,
    };
    let end = match range.end_bound() {
        Bound::Included(end) => end.checked_add(1).expect("wasmgc: range end overflow"),
        Bound::Excluded(end) => *end,
        Bound::Unbounded => length,
    };
    assert!(
        start <= end && end <= length,
        "wasmgc: buffer range out of bounds"
    );
    (start, end)
}

impl<T: GcValue> Gc<Array<T>> {
    /// Creates an array of `length` elements initialized to `value`, using `array.new`.
    ///
    /// The construction value is consumed after its representation has been stored.
    /// For a reference value, every element refers to the same object; construction
    /// does not clone that object.
    ///
    /// # Panics
    ///
    /// Panics if `length` exceeds `u32::MAX`.
    pub fn array_new(length: usize, value: T) -> Self {
        let value_abi = value.to_abi();
        // SAFETY: value retains the encoded input through allocation and storage.
        // The shim returns one owned, nonzero root with the Array<T> schema.
        unsafe { Self::from_raw(ArrayOps::<T>::array_new(wasm_len(length), value_abi)) }
    }

    /// Creates an array initialized with Wasm storage defaults, using `array.new_default`.
    ///
    /// Numeric elements are zero, booleans are false, and nullable references are
    /// null. This uses the default for [`GcValue::KIND`], without calling Rust's
    /// [`Default`] implementation. For [`crate::ExternRef`], reading a null
    /// element still produces a nonzero owned root.
    ///
    /// The element storage must be defaultable. Non-null GC-reference elements
    /// are rejected at compile time, even for a zero length. Use [`Self::array_new`]
    /// to supply an initial value, or [`Self::empty`] to construct an empty array
    /// of any supported element type.
    ///
    /// # Panics
    ///
    /// Panics if `length` exceeds `u32::MAX`.
    pub fn new_default(length: usize) -> Self {
        const {
            assert!(
                T::KIND.is_defaultable(),
                "wasmgc: new_default requires defaultable Wasm storage; use array_new for non-null references"
            );
        }
        // SAFETY: The const assertion permits only defaultable storage. The
        // generated allocation transfers one nonzero root for this array schema.
        unsafe { Self::from_raw(ArrayOps::<T>::new_default(wasm_len(length))) }
    }

    /// Returns the number of elements, using `array.len`.
    ///
    /// The length is fixed when the array is constructed.
    pub fn len(&self) -> usize {
        // SAFETY: self retains a live Array<T> root and the matching shim reads
        // its length without consuming that root.
        unsafe { ArrayOps::<T>::len(self.as_raw()) as usize }
    }

    /// Returns the element at `index`, or `None` if it is out of bounds.
    ///
    /// This uses `array.get`, or `array.get_u` for packed integer storage.
    /// Reference elements return new owned roots to the stored objects.
    /// See [`GcArray`] for an example.
    pub fn get(&self, index: usize) -> Option<T> {
        if index >= self.len() {
            return None;
        }
        // SAFETY: The index is below this fixed array's u32 length. The matching
        // getter returns a valid scalar or a newly owned root of T's declared kind.
        Some(unsafe { T::from_abi(ArrayOps::<T>::get(self.as_raw(), index as u32)) })
    }

    /// Replaces the element at `index`, using `array.set`.
    ///
    /// Reference inputs borrow a root until the reference is stored. The array
    /// then retains the object independently. See [`GcArray`] for an example.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of bounds.
    pub fn set(&self, index: usize, value: T::SetterValue<'_>) {
        assert!(index < self.len(), "wasmgc: array index out of bounds");
        let value_abi = T::setter_abi(&value);
        // SAFETY: The checked index fits the array and u32. self and the complete
        // setter value retain all required roots until the matching write returns.
        unsafe { ArrayOps::<T>::set(self.as_raw(), index as u32, value_abi) }
    }

    /// Replaces every element with `value`, using `array.fill`.
    ///
    /// For a reference value, every element retains the same object. The input
    /// root is borrowed only for the duration of the operation.
    pub fn fill(&self, value: T::SetterValue<'_>) {
        let value_abi = T::setter_abi(&value);
        // SAFETY: self roots the destination and value retains the encoded input.
        // The generated shim fills exactly the destination's current length.
        unsafe { ArrayOps::<T>::fill(self.as_raw(), value_abi) }
    }

    /// Copies `length` elements from `source` into this array, using `array.copy`.
    ///
    /// Elements starting at `source_offset` are copied to `destination`. The
    /// arrays may be the same object, and overlapping ranges are supported.
    /// Reference elements retain the same objects without cloning them.
    ///
    /// # Panics
    ///
    /// Panics if either range falls outside its array. A zero-length range may
    /// start at the array's end.
    pub fn copy(&self, destination: usize, source: &Self, source_offset: usize, length: usize) {
        check_range(source_offset, length, source.len());
        check_range(destination, length, self.len());
        // SAFETY: Both complete ranges were checked against their live arrays;
        // array.copy permits either root to refer to the same array.
        unsafe { self.copy_unchecked(destination, source, source_offset, length) }
    }
}

impl<T: GcValue> Gc<Array<T>> {
    /// Creates an empty array, using `array.new_fixed` with no elements.
    ///
    /// No initial value is required, including for non-null reference elements.
    /// [`Default`] also constructs an empty array.
    pub fn empty() -> Self {
        // SAFETY: A zero-element array requires no defaultable storage or input
        // values, and the shim transfers one owned root of Array<T>.
        unsafe { Self::from_raw(ArrayOps::<T>::new_fixed()) }
    }

    /// Creates an array filled with a single Rust [`Default`] value.
    ///
    /// Calls `T::default()` once and repeats its stored representation. For a
    /// reference type, all elements share that default object. Compare
    /// [`Self::new_default`], which uses Wasm storage defaults instead.
    ///
    /// # Panics
    ///
    /// Panics if `length` exceeds `u32::MAX`.
    pub fn defaulted(length: usize) -> Self
    where
        T: Default,
    {
        Self::array_new(length, T::default())
    }

    /// Returns whether the array has no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Copies a range of `source` to `destination` in this array.
    ///
    /// This is the Rust-range form of [`Self::copy`]. It supports aliasing and
    /// overlap and preserves reference identity.
    ///
    /// # Panics
    ///
    /// Panics if `range` is invalid or outside `source`, if its bounds overflow,
    /// or if the copied elements do not fit at `destination`.
    ///
    /// # Examples
    ///
    /// Compile for a Wasm target and postprocess before execution.
    ///
    /// ```no_run
    /// use wasmgc::GcArray;
    ///
    /// let source = GcArray::from_slice([1i32, 2, 3, 4]);
    /// let destination = GcArray::<i32>::new_default(3);
    /// destination.copy_from(1, &source, 1..3);
    /// assert_eq!(destination, [0, 2, 3]);
    /// ```
    pub fn copy_from(&self, destination: usize, source: &Self, range: impl RangeBounds<usize>) {
        let (start, end) = range_bounds(range, source.len());
        let length = end - start;
        check_range(destination, length, self.len());
        // SAFETY: range_bounds validated the complete source range and
        // check_range validated the destination, including zero-length copies.
        unsafe { self.copy_unchecked(destination, source, start, length) }
    }

    /// Copies a range to another position within this array.
    ///
    /// Overlapping ranges behave as if the source values were read before any
    /// destination value was replaced.
    ///
    /// # Panics
    ///
    /// Panics under the same conditions as [`Self::copy_from`].
    pub fn copy_within(&self, range: impl RangeBounds<usize>, destination: usize) {
        self.copy_from(destination, self, range);
    }

    /// Returns an iterator over copied element values.
    ///
    /// Values are read as the iterator advances, so writes through other aliases
    /// are observed by subsequent reads. Each reference element creates an
    /// independently owned root. See [`GcArrayIter`] for an example.
    pub fn iter(&self) -> GcArrayIter<'_, T> {
        GcArrayIter::new(self, self.len())
    }

    /// Copies a Rust slice, array, or vector into a new GC array.
    ///
    /// The source elements remain alive until all values have been stored.
    /// Reference elements keep the same objects without requiring [`Clone`].
    /// An owned source container is dropped after copying.
    ///
    /// This copies between linear memory and GC storage, element by element;
    /// it does not transfer a Rust allocation into the GC heap.
    /// See [`GcArray`] for an example.
    ///
    /// # Panics
    ///
    /// Panics if the source length exceeds `u32::MAX`.
    pub fn from_slice(values: impl AsRef<[T]>) -> Self {
        let values = values.as_ref();
        let length = wasm_len(values.len());
        if let Some(ops) = scalar_ops::<T>() {
            // SAFETY: scalar_ops matches T by TypeId to a built-in scalar with the
            // shim's exact memory layout. The source slice remains readable through
            // the copy, which returns one owned root of Array<T>.
            return unsafe { Self::from_raw((ops.from_slice)(values.as_ptr().cast(), length)) };
        }

        let Some((first, rest)) = values.split_first() else {
            return Self::empty();
        };
        // SAFETY: The nonempty source retains first through the allocation.
        // The matching shim returns one owned array root; wrapping it here also
        // releases that root if a later custom conversion panics.
        let array = unsafe { Self::from_raw(ArrayOps::<T>::array_new(length, first.to_abi())) };
        // SAFETY: array has values.len() elements, so rest exactly fits after
        // the first element. The source retains every borrowed input root.
        unsafe { array.write_unchecked(1, rest) };
        array
    }

    /// Copies elements starting at `offset` into `output`.
    ///
    /// Each previous output element is dropped when replaced. Reference reads
    /// produce owned roots independent of the array's root.
    ///
    /// # Panics
    ///
    /// Panics if the output range falls outside the array.
    pub fn read(&self, offset: usize, output: &mut [T]) {
        check_range(offset, output.len(), self.len());
        // SAFETY: check_range validated the entire source range. output is
        // an exclusive slice of initialized T values to replace.
        unsafe { self.read_unchecked(offset, output) }
    }

    /// Copies `input` into this array starting at `offset`.
    ///
    /// The source retains its values until all writes finish. Reference writes
    /// store the same objects rather than cloning them.
    ///
    /// # Panics
    ///
    /// Panics if the input range falls outside the array.
    pub fn write(&self, offset: usize, input: &[T]) {
        check_range(offset, input.len(), self.len());
        // SAFETY: check_range validated the complete destination range, and
        // the input slice keeps its values and any borrowed roots alive.
        unsafe { self.write_unchecked(offset, input) }
    }

    /// Copies all elements into `output`, dropping its previous values.
    ///
    /// Reference elements receive independently owned roots.
    ///
    /// # Panics
    ///
    /// Panics if `output.len()` differs from this array's length.
    pub fn copy_to_slice(&self, output: &mut [T]) {
        assert_eq!(output.len(), self.len(), "wasmgc: slice lengths differ");
        // SAFETY: Equal lengths make the complete source range valid, and
        // output exclusively borrows initialized destination values.
        unsafe { self.read_unchecked(0, output) }
    }

    /// Replaces all elements with values copied from `input`.
    ///
    /// Reference elements retain the source objects without cloning them.
    ///
    /// # Panics
    ///
    /// Panics if `input.len()` differs from this array's length.
    pub fn copy_from_slice(&self, input: &[T]) {
        assert_eq!(input.len(), self.len(), "wasmgc: slice lengths differ");
        // SAFETY: Equal lengths make the complete destination range valid;
        // input keeps the source values alive until the copy completes.
        unsafe { self.write_unchecked(0, input) }
    }
}

impl<T: GcValue> Gc<Array<T>> {
    /// Copies an already-validated pair of GC ranges.
    ///
    /// # Safety
    ///
    /// Both complete ranges must fit within their respective arrays. Because
    /// array lengths are u32, each offset and the count must also fit in u32.
    unsafe fn copy_unchecked(
        &self,
        destination: usize,
        source: &Self,
        source_offset: usize,
        length: usize,
    ) {
        // SAFETY: The caller guarantees both ranges fit and can be narrowed to
        // u32. Both roots remain borrowed, use the same element schema, and
        // array.copy explicitly permits overlap.
        unsafe {
            ArrayOps::<T>::copy(
                self.as_raw(),
                destination as u32,
                source.as_raw(),
                source_offset as u32,
                length as u32,
            )
        }
    }

    /// Reads an already-validated range into initialized Rust values.
    ///
    /// # Safety
    ///
    /// `offset..offset + output.len()` must fit in this array. This also ensures
    /// the offset and length fit the generated shim's u32 parameters.
    unsafe fn read_unchecked(&self, offset: usize, output: &mut [T]) {
        if let Some(ops) = scalar_ops::<T>() {
            // SAFETY: TypeId selection proves T has the concrete scalar shim's
            // layout. The caller validates the array range, and output provides an
            // exclusive writable slice for the entire copy. Bool reads normalize
            // the stored i32 to a valid Rust bool byte.
            unsafe {
                (ops.read)(
                    self.as_raw(),
                    offset as u32,
                    output.as_mut_ptr().cast(),
                    output.len() as u32,
                )
            }
            return;
        }

        for (index, output) in output.chunks_mut(COPY_BATCH_SIZE).enumerate() {
            let mut values = self.read_abi(offset + index * COPY_BATCH_SIZE, output.len());
            for value in output {
                *value = values.take();
            }
        }
    }

    /// Writes Rust values into an already-validated range.
    ///
    /// # Safety
    ///
    /// `offset..offset + input.len()` must fit in this array. The source slice
    /// must retain the resources required by every encoded element until its
    /// write finishes, as required by [`GcValue`].
    unsafe fn write_unchecked(&self, offset: usize, input: &[T]) {
        if let Some(ops) = scalar_ops::<T>() {
            // SAFETY: TypeId selection proves T matches the concrete scalar shim's
            // layout. The caller validates the array range, and input provides
            // readable scalar bytes for the duration of the copy.
            unsafe {
                (ops.write)(
                    self.as_raw(),
                    offset as u32,
                    input.as_ptr().cast(),
                    input.len() as u32,
                )
            }
            return;
        }

        let mut values = [MaybeUninit::<T::Abi>::uninit(); COPY_BATCH_SIZE];
        for (index, input) in input.chunks(COPY_BATCH_SIZE).enumerate() {
            for (value, input) in values.iter_mut().zip(input) {
                value.write(input.to_abi());
            }
            // SAFETY: Each element of this batch was initialized by to_abi, and
            // the input slice retains its borrowed roots. The caller's checked
            // range covers this batch; its count never exceeds the ABI buffer.
            unsafe {
                ArrayOps::<T>::write_abi(
                    self.as_raw(),
                    (offset + index * COPY_BATCH_SIZE) as u32,
                    values.as_ptr().cast(),
                    input.len() as u32,
                )
            }
        }
    }

    // Callers must supply a valid array range of at most COPY_BATCH_SIZE entries.
    // The returned guard owns the fresh roots produced by the read shim.
    fn read_abi(&self, offset: usize, length: usize) -> AbiValues<T> {
        let mut values = AbiValues {
            values: [MaybeUninit::<T::Abi>::uninit(); COPY_BATCH_SIZE],
            initialized: 0,
            consumed: 0,
        };
        // SAFETY: read_unchecked and to_vec bound their batches by both the
        // array's length and COPY_BATCH_SIZE. The buffer has enough writable ABI
        // slots, and the shim initializes each with a valid owned result. The
        // guard assumes ownership of all initialized results after the call.
        unsafe {
            ArrayOps::<T>::read_abi(
                self.as_raw(),
                offset as u32,
                values.values.as_mut_ptr().cast(),
                length as u32,
            )
        }
        values.initialized = length;
        values
    }
}

#[cfg(feature = "alloc")]
impl<T: GcValue> Gc<Array<T>> {
    /// Copies all elements into a new Rust vector.
    ///
    /// Reference elements receive independently owned roots to the stored objects.
    /// This method requires the `alloc` feature.
    ///
    /// # Examples
    ///
    /// Compile for a Wasm target and postprocess before execution.
    ///
    /// ```no_run
    /// use wasmgc::GcArray;
    ///
    /// let array = GcArray::from_slice([4u8, 5, 6]);
    /// assert_eq!(array.to_vec(), [4, 5, 6]);
    /// ```
    pub fn to_vec(&self) -> Vec<T> {
        if let Some(ops) = scalar_ops::<T>() {
            let length = self.len();
            let mut result = Vec::<T>::with_capacity(length);
            // SAFETY: The vector has capacity for the array's entire u32 length.
            // TypeId selection proves T's scalar layout, including normalization of
            // bools to 0/1. The shim initializes all elements before set_len exposes
            // them to safe Rust; built-in scalars need no previous values dropped.
            unsafe {
                (ops.read)(self.as_raw(), 0, result.as_mut_ptr().cast(), length as u32);
                result.set_len(length);
            }
            return result;
        }

        let length = self.len();
        let mut result = Vec::with_capacity(length);
        let mut offset = 0;
        while offset < length {
            let count = COPY_BATCH_SIZE.min(length - offset);
            let mut values = self.read_abi(offset, count);
            for _ in 0..count {
                result.push(values.take());
            }
            offset += count;
        }
        result
    }
}

impl<T: GcValue> Default for GcArray<T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T: GcValue> From<&[T]> for GcArray<T> {
    fn from(values: &[T]) -> Self {
        Self::from_slice(values)
    }
}

impl<T: GcValue, const N: usize> From<[T; N]> for GcArray<T> {
    fn from(values: [T; N]) -> Self {
        Self::from_slice(values)
    }
}

impl<T: GcValue, const N: usize> From<&[T; N]> for GcArray<T> {
    fn from(values: &[T; N]) -> Self {
        Self::from_slice(values)
    }
}

#[cfg(feature = "alloc")]
impl<T: GcValue> From<Vec<T>> for GcArray<T> {
    fn from(values: Vec<T>) -> Self {
        Self::from_slice(values)
    }
}

impl<T: GcValue + fmt::Debug> fmt::Debug for GcArray<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl<T: GcValue + PartialEq> PartialEq for GcArray<T> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl<T: GcValue + Eq> Eq for GcArray<T> {}

impl<T: GcValue + PartialEq> PartialEq<[T]> for GcArray<T> {
    fn eq(&self, other: &[T]) -> bool {
        self.len() == other.len() && self.iter().zip(other).all(|(left, right)| left.eq(right))
    }
}

impl<T: GcValue + PartialEq> PartialEq<&[T]> for GcArray<T> {
    fn eq(&self, other: &&[T]) -> bool {
        self == *other
    }
}

impl<T: GcValue + PartialEq, const N: usize> PartialEq<[T; N]> for GcArray<T> {
    fn eq(&self, other: &[T; N]) -> bool {
        self == other.as_slice()
    }
}

/// An iterator over values read from a borrowed [`GcArray`].
///
/// Created by [`GcArray::iter`] or iteration over `&GcArray<T>`. The iterator
/// borrows the root but returns owned values; reference elements receive their
/// own roots. It reads each element when visited, so it is not a snapshot of
/// the array's contents.
///
/// # Examples
///
/// Compile for a Wasm target and postprocess before execution.
///
/// ```no_run
/// use wasmgc::GcArray;
///
/// let array = GcArray::from_slice([1i32, 2, 3]);
/// let mut values = array.iter();
/// assert_eq!(values.next(), Some(1));
/// array.set(1, 8);
/// assert_eq!(values.next(), Some(8));
/// assert_eq!(values.next_back(), Some(3));
/// ```
pub struct GcArrayIter<'a, T: GcValue> {
    array: &'a GcArray<T>,
    front: usize,
    back: usize,
}

impl<'a, T: GcValue> GcArrayIter<'a, T> {
    fn new(array: &'a GcArray<T>, length: usize) -> Self {
        Self {
            array,
            front: 0,
            back: length,
        }
    }

    fn element(&self, index: usize) -> T {
        // SAFETY: next/next_back check the front/back bounds derived from this
        // fixed-length array before calling element. The borrowed root stays live,
        // and the matching getter returns a valid scalar or a new owned T root.
        unsafe { T::from_abi(ArrayOps::<T>::get(self.array.as_raw(), index as u32)) }
    }
}

impl<T: GcValue> Iterator for GcArrayIter<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        if self.front == self.back {
            return None;
        }
        let value = self.element(self.front);
        self.front += 1;
        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len(), Some(self.len()))
    }

    fn nth(&mut self, n: usize) -> Option<T> {
        self.front += n.min(self.len());
        self.next()
    }

    fn count(self) -> usize {
        self.len()
    }
}

impl<T: GcValue> DoubleEndedIterator for GcArrayIter<'_, T> {
    fn next_back(&mut self) -> Option<T> {
        if self.front == self.back {
            return None;
        }
        self.back -= 1;
        Some(self.element(self.back))
    }

    fn nth_back(&mut self, n: usize) -> Option<T> {
        self.back -= n.min(self.len());
        self.next_back()
    }
}

impl<T: GcValue> ExactSizeIterator for GcArrayIter<'_, T> {
    fn len(&self) -> usize {
        self.back - self.front
    }
}

impl<T: GcValue> FusedIterator for GcArrayIter<'_, T> {}

impl<'a, T: GcValue> IntoIterator for &'a GcArray<T> {
    type Item = T;

    type IntoIter = GcArrayIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

struct ArrayOps<T: GcValue>(PhantomData<fn() -> T>);

impl<T: GcValue> ArrayOps<T> {
    const SCHEMA: *const Schema = <Array<T> as GcHeapObj>::SCHEMA;
}

struct ScalarOps {
    from_slice: unsafe extern "C" fn(*const (), u32) -> u32,
    read: unsafe extern "C" fn(u32, u32, *mut (), u32),
    write: unsafe extern "C" fn(u32, u32, *const (), u32),
}

fn scalar_ops<T: GcValue>() -> Option<ScalarOps> {
    macro_rules! scalar {
        ($($ty:ty),* $(,)?) => {
            $(
                if TypeId::of::<T>() == TypeId::of::<$ty>() {
                    return Some(ScalarOps {
                        from_slice: ArrayOps::<$ty>::from_slice,
                        read: ArrayOps::<$ty>::read,
                        write: ArrayOps::<$ty>::write,
                    });
                }
            )*
        };
    }

    // Compare actual Rust types rather than storage kinds: a custom GcValue
    // can lower to a scalar while having a completely different memory layout.
    // Returning concrete shims also keeps invalid raw-copy shims out of debug builds.
    scalar!(i8, u8, i16, u16, i32, u32, i64, u64, f32, f64, bool);
    None
}

macro_rules! operation {
    ($name:ident, $descriptor:ident, $kind:ident, ($($param:ident: $ty:ty),*) -> $result:ty) => {
        const $descriptor: Operation = Operation::new(
            Self::SCHEMA,
            OperationKind::$kind,
            0,
            Self::$name as *const (),
        );

        /// Executes the described operation after Wasm postprocessing.
        ///
        /// # Safety
        ///
        /// Root arguments must be live slots for the schema and root table
        /// declared by the operation. Element arguments must satisfy T's ABI
        /// contract, and any borrowed roots must remain live until the call
        /// returns. Array ranges must be in bounds. Pointer arguments must
        /// cover the operation's full input or output buffer with the declared
        /// element representation; output buffers must be exclusively writable.
        /// The module must be postprocessed before executing this shim.
        #[inline(never)]
        unsafe extern "C" fn $name($($param: $ty),*) -> $result {
            let arguments = ($($param,)*);
            // SAFETY: The descriptor points to this exact shim and records its
            // operation. The local tuple retains every parameter in declaration
            // order, and the selected marker matches the numeric result ABI.
            unsafe {
                <$result as GcAbi>::marker(
                    (&Self::$descriptor as *const Operation).cast(),
                    (&raw const arguments).cast(),
                )
            }
        }
    };
    ($name:ident, $descriptor:ident, $kind:ident, ($($param:ident: $ty:ty),*)) => {
        const $descriptor: Operation = Operation::new(
            Self::SCHEMA,
            OperationKind::$kind,
            0,
            Self::$name as *const (),
        );

        /// Executes the described write operation after Wasm postprocessing.
        ///
        /// # Safety
        ///
        /// Root arguments must be live slots for the declared schema and table.
        /// Element arguments must satisfy T's ABI contract, and all borrowed
        /// roots must outlive the call. Every array range must be in bounds.
        /// Pointer arguments must cover the complete buffer with the operation's
        /// ABI or scalar representation, remaining readable or exclusively
        /// writable as appropriate. Postprocessing must replace this shim
        /// before it executes.
        #[inline(never)]
        unsafe extern "C" fn $name($($param: $ty),*) {
            let arguments = ($($param,)*);
            // SAFETY: The descriptor identifies this exact void-returning shim,
            // and the live tuple retains every declared parameter for discovery
            // before the postprocessor replaces the function body.
            unsafe {
                abi::marker_void(
                    (&Self::$descriptor as *const Operation).cast(),
                    (&raw const arguments).cast(),
                )
            }
        }
    };
}

impl<T: GcValue> ArrayOps<T> {
    operation!(array_new, ARRAY_NEW, ArrayNew, (length: u32, value: T::Abi) -> u32);
    operation!(new_fixed, NEW_FIXED, ArrayNewFixed, () -> u32);
    operation!(new_default, NEW_DEFAULT, ArrayNewDefault, (length: u32) -> u32);
    operation!(len, LEN, ArrayLen, (handle: u32) -> u32);
    operation!(get, GET, ArrayGet, (handle: u32, index: u32) -> T::Abi);
    operation!(set, SET, ArraySet, (handle: u32, index: u32, value: T::Abi));
    operation!(fill, FILL, ArrayFill, (handle: u32, value: T::Abi));
    operation!(
        copy, COPY, ArrayCopy,
        (destination: u32, destination_offset: u32, source: u32, source_offset: u32, length: u32)
    );
}

impl<T: GcValue> ArrayOps<T> {
    operation!(
        read_abi, READ_ABI, ArrayReadAbi,
        (handle: u32, offset: u32, pointer: *mut (), length: u32)
    );
    operation!(
        write_abi, WRITE_ABI, ArrayWriteAbi,
        (handle: u32, offset: u32, pointer: *const (), length: u32)
    );

    // Only scalar_ops may select these raw-memory shims, after proving a
    // concrete built-in Rust scalar type and its matching memory representation.
    operation!(from_slice, FROM_SLICE, ArrayFromSlice, (pointer: *const (), length: u32) -> u32);
    operation!(read, READ, ArrayRead, (handle: u32, offset: u32, pointer: *mut (), length: u32));
    operation!(write, WRITE, ArrayWrite, (handle: u32, offset: u32, pointer: *const (), length: u32));
}
