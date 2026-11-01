use wasmgc::{Gc, wasmgc};

#[wasmgc]
struct Scalars {
    signed_byte: i8,
    byte: u8,
    signed_short: i16,
    short: u16,
    integer: i32,
    unsigned: u32,
    wide: i64,
    unsigned_wide: u64,
    float: f32,
    double: f64,
    boolean: bool,
}

#[wasmgc]
pub fn test_struct_scalars() {
    let value = Gc::new(Scalars {
        signed_byte: i8::MIN,
        byte: u8::MAX,
        signed_short: i16::MIN,
        short: u16::MAX,
        integer: i32::MIN,
        unsigned: u32::MAX,
        wide: i64::MIN,
        unsigned_wide: u64::MAX,
        float: -1.5,
        double: 2.25,
        boolean: true,
    });

    assert_eq!(value.signed_byte(), i8::MIN);
    assert_eq!(value.byte(), u8::MAX);
    assert_eq!(value.signed_short(), i16::MIN);
    assert_eq!(value.short(), u16::MAX);
    assert_eq!(value.integer(), i32::MIN);
    assert_eq!(value.unsigned(), u32::MAX);
    assert_eq!(value.wide(), i64::MIN);
    assert_eq!(value.unsigned_wide(), u64::MAX);
    assert_eq!(value.float(), -1.5);
    assert_eq!(value.double(), 2.25);
    assert!(value.boolean());

    value.set_signed_byte(i8::MAX);
    value.set_byte(0);
    value.set_signed_short(i16::MAX);
    value.set_short(0);
    value.set_integer(i32::MAX);
    value.set_unsigned(0);
    value.set_wide(i64::MAX);
    value.set_unsigned_wide(0);
    value.set_float(f32::INFINITY);
    value.set_double(-0.0);
    value.set_boolean(false);

    assert_eq!(value.signed_byte(), i8::MAX);
    assert_eq!(value.byte(), 0);
    assert_eq!(value.signed_short(), i16::MAX);
    assert_eq!(value.short(), 0);
    assert_eq!(value.integer(), i32::MAX);
    assert_eq!(value.unsigned(), 0);
    assert_eq!(value.wide(), i64::MAX);
    assert_eq!(value.unsigned_wide(), 0);
    assert_eq!(value.float(), f32::INFINITY);
    assert_eq!(value.double().to_bits(), (-0.0f64).to_bits());
    assert!(!value.boolean());
}
