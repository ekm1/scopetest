mod import_extractor;

pub use import_extractor::{ImportInfo, ImportType, parse_file};

use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ParseError {
    #[error("Failed to read file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Failed to parse file: {0}")]
    SyntaxError(String),
}

pub const SUPPORTED_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];

pub fn is_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            let ext_with_dot = format!(".{}", ext);
            SUPPORTED_EXTENSIONS.contains(&ext_with_dot.as_str())
        })
        .unwrap_or(false)
}
