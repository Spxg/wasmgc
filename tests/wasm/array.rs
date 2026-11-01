use wasmgc::{GcArray, wasmgc};

#[wasmgc]
pub fn test_array_construction_and_fill() {
    let empty = GcArray::<i32>::empty();
    assert!(empty.is_empty());
    assert_eq!(empty.len(), 0);
    assert_eq!(empty.get(0), None);

    let zeroes = GcArray::<i32>::new_default(3);
    assert_eq!(zeroes, [0, 0, 0]);

    let values = GcArray::array_new(3, 7i32);
    assert_eq!(values.len(), 3);
    assert!(!values.is_empty());
    assert_eq!(values, [7, 7, 7]);
    assert_eq!(values.get(3), None);
    assert_eq!(values.get(usize::MAX), None);

    values.set(1, -9);
    assert_eq!(values, [7, -9, 7]);
    values.fill(4);
    assert_eq!(values, [4, 4, 4]);

    empty.fill(1);
    assert!(empty.is_empty());
}

#[wasmgc]
pub fn test_array_packed_integers() {
    let signed_bytes = GcArray::from_slice([i8::MIN, -1, 0, i8::MAX]);
    assert_eq!(signed_bytes.get(0), Some(i8::MIN));
    assert_eq!(signed_bytes.get(1), Some(-1));
    signed_bytes.write(1, &[-2, 3]);
    let mut bytes = [0i8; 4];
    signed_bytes.copy_to_slice(&mut bytes);
    assert_eq!(bytes, [i8::MIN, -2, 3, i8::MAX]);

    let unsigned_bytes = GcArray::from_slice([0u8, 127, 128, u8::MAX]);
    assert_eq!(unsigned_bytes.get(3), Some(u8::MAX));
    unsigned_bytes.set(0, 200);
    assert_eq!(unsigned_bytes.to_vec(), [200, 127, 128, u8::MAX]);

    let signed_words = GcArray::from_slice([i16::MIN, -1, 0, i16::MAX]);
    assert_eq!(signed_words.get(0), Some(i16::MIN));
    signed_words.write(1, &[-300, 400]);
    let mut words = [0i16; 4];
    signed_words.copy_to_slice(&mut words);
    assert_eq!(words, [i16::MIN, -300, 400, i16::MAX]);

    let unsigned_words = GcArray::from_slice([0u16, 32767, 32768, u16::MAX]);
    assert_eq!(unsigned_words.get(3), Some(u16::MAX));
    unsigned_words.set(0, 50000);
    assert_eq!(unsigned_words.to_vec(), [50000, 32767, 32768, u16::MAX]);
}

#[wasmgc]
pub fn test_array_integer_slices() {
    let signed = GcArray::from_slice([i64::MIN, -1, 0, i64::MAX]);
    assert_eq!(signed.get(3), Some(i64::MAX));
    signed.write(1, &[1i64 << 40, -(1i64 << 40)]);
    let mut output = [0i64; 2];
    signed.read(1, &mut output);
    assert_eq!(output, [1i64 << 40, -(1i64 << 40)]);
    assert_eq!(
        signed.to_vec(),
        [i64::MIN, 1i64 << 40, -(1i64 << 40), i64::MAX]
    );

    let unsigned = GcArray::from_slice([0u64, 1u64 << 63, u64::MAX]);
    unsigned.copy_from_slice(&[u64::MAX, 1u64 << 40, 0]);
    let mut output = [0u64; 3];
    unsigned.copy_to_slice(&mut output);
    assert_eq!(output, [u64::MAX, 1u64 << 40, 0]);

    let words = GcArray::from_slice([0u32, 1u32 << 31, u32::MAX]);
    assert_eq!(words.to_vec(), [0, 1u32 << 31, u32::MAX]);
}

#[wasmgc]
pub fn test_array_float_slices() {
    let singles = GcArray::from_slice([-0.0f32, 1.5, f32::INFINITY]);
    assert_eq!(singles.get(0).unwrap().to_bits(), (-0.0f32).to_bits());
    singles.write(1, &[-2.5, 3.25]);
    let mut output = [0.0f32; 3];
    singles.copy_to_slice(&mut output);
    assert_eq!(output, [-0.0, -2.5, 3.25]);

    let doubles = GcArray::from_slice([f64::NEG_INFINITY, 1.0 / 3.0, -0.0]);
    doubles.set(0, 1.0e100);
    let mut output = [0.0f64; 2];
    doubles.read(1, &mut output);
    assert_eq!(output[0], 1.0 / 3.0);
    assert_eq!(output[1].to_bits(), (-0.0f64).to_bits());
    assert_eq!(doubles.get(0), Some(1.0e100));
}

#[wasmgc]
pub fn test_array_bool_slices() {
    let values = GcArray::<bool>::new_default(4);
    assert_eq!(values, [false; 4]);

    values.copy_from_slice(&[true, false, true, false]);
    values.write(1, &[true, false]);
    let mut output = [false; 4];
    values.copy_to_slice(&mut output);
    assert_eq!(output, [true, true, false, false]);
    assert_eq!(values.get(0), Some(true));
    assert_eq!(values.get(3), Some(false));

    values.fill(true);
    assert_eq!(values.to_vec(), [true; 4]);
}

#[wasmgc]
pub fn test_array_copy_overlap() {
    let values = GcArray::from_slice([1i32, 2, 3, 4, 5]);
    values.copy_within(0..4, 1);
    assert_eq!(values, [1, 1, 2, 3, 4]);
    values.copy_within(1..5, 0);
    assert_eq!(values, [1, 2, 3, 4, 4]);

    let destination = GcArray::<i32>::new_default(5);
    destination.copy(1, &values, 2, 3);
    assert_eq!(destination, [0, 3, 4, 4, 0]);
    destination.copy_from(0, &values, 1..=2);
    assert_eq!(destination, [2, 3, 4, 4, 0]);

    destination.copy(destination.len(), &values, values.len(), 0);
    destination.copy_within(destination.len().., destination.len());
    values.read(values.len(), &mut []);
    values.write(values.len(), &[]);
    assert_eq!(destination, [2, 3, 4, 4, 0]);
}

#[wasmgc]
pub fn test_array_iterator() {
    let values = GcArray::from_slice([1i32, 2, 3, 4, 5, 6]);
    let mut iter = values.iter();
    assert_eq!(iter.len(), 6);
    assert_eq!(iter.size_hint(), (6, Some(6)));
    assert_eq!(iter.next(), Some(1));
    assert_eq!(iter.next_back(), Some(6));
    assert_eq!(iter.nth(1), Some(3));
    assert_eq!(iter.len(), 2);

    values.set(3, 40);
    assert_eq!(iter.next(), Some(40));
    assert_eq!(iter.nth_back(0), Some(5));
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);

    let mut iter = values.iter();
    assert_eq!(iter.nth(usize::MAX), None);
    assert_eq!(iter.len(), 0);
    let mut iter = values.iter();
    assert_eq!(iter.nth_back(usize::MAX), None);
    assert_eq!(iter.next(), None);
}
