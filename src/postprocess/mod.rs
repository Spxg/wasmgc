mod data_reader;
mod encode;
mod resolver;
mod root;
mod schema;
mod transformer;
mod types;

use data_reader::DataReader;
use resolver::Resolver;
use schema::{FunctionExport, FunctionImport, Operation, Schema, Struct, Value};
use transformer::Transformer;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub fn process(module: &[u8]) -> Result<Vec<u8>> {
    Transformer::new(Resolver::parse(module)?)
        .transform()
        .finish(module)
}
