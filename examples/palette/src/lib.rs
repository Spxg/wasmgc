//! Shared palette entries rendered by the terminal and browser examples.
//!
//! Canvas cells hold references to three color objects. Updating one color
//! changes every cell that references it without replacing the canvas.

use wasmgc::{Gc, GcArray, wasmgc};

/// An RGB palette entry shared by multiple canvas cells.
#[wasmgc]
pub struct Color {
    /// Red intensity, from 0 to 255.
    #[getter]
    #[setter]
    pub r: u8,
    /// Green intensity, from 0 to 255.
    #[getter]
    #[setter]
    pub g: u8,
    /// Blue intensity, from 0 to 255.
    #[getter]
    #[setter]
    pub b: u8,
}

/// Creates a row-major canvas whose cells share three repeating palette entries.
#[wasmgc]
pub fn create_canvas(width: u32, height: u32) -> GcArray<Gc<Color>> {
    let colors = [
        Gc::new(Color {
            r: 235,
            g: 80,
            b: 75,
        }),
        Gc::new(Color {
            r: 50,
            g: 130,
            b: 220,
        }),
        Gc::new(Color {
            r: 80,
            g: 190,
            b: 120,
        }),
    ];
    let cells: Vec<_> = (0..width * height)
        .map(|index| colors[((index % width + index / width) % 3) as usize].clone())
        .collect();

    GcArray::from_slice(cells)
}

/// Returns the existing palette entry referenced by a canvas cell.
///
/// # Panics
///
/// Panics if `index` is outside the canvas.
#[wasmgc]
pub fn color_at(canvas: &GcArray<Gc<Color>>, index: u32) -> Gc<Color> {
    canvas
        .get(index as usize)
        .expect("cell index out of bounds")
}
