//! Reads a linked Wasm module, lowers its wasmgc shims, and writes the result.
//!
//! File handling and command-line errors stay here; binary transformation is
//! provided by `wasmgc::process` when the `cli` feature is enabled.

use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut input = None;
    let mut output = None;
    let mut args = env::args_os().skip(1);

    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => {
                println!(
                    "Usage: wasmgc <input.wasm> -o <output.wasm>\n\nEncodes WasmGC implementations directly; no external tools required."
                );

                return Ok(());
            }
            Some("-o" | "--output") => {
                output = Some(PathBuf::from(args.next().ok_or("missing output path")?))
            }
            Some(value) if value.starts_with('-') => {
                return Err(format!("unknown option {value}").into());
            }
            _ if input.is_none() => input = Some(PathBuf::from(arg)),
            _ => return Err("expected one input module".into()),
        }
    }

    let input = input.ok_or("missing input: wasmgc <input.wasm> -o <output.wasm>")?;
    let output = output.ok_or("missing -o <output.wasm>")?;

    let bytes = fs::read(input)?;
    let bytes = wasmgc::process(&bytes)?;

    fs::write(output, bytes)?;

    Ok(())
}
