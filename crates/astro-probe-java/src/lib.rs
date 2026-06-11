#![allow(
    clippy::collapsible_match,
    clippy::manual_range_patterns,
    clippy::manual_is_multiple_of,
    clippy::unnecessary_get_then_check,
    clippy::too_many_arguments,
    clippy::if_same_then_else,
    clippy::needless_range_loop,
    clippy::manual_strip,
    clippy::while_let_on_iterator,
    clippy::collapsible_str_replace,
    clippy::redundant_pattern_matching,
    dead_code,
    unused_variables,
    unused_imports
)]

pub mod di;
pub mod jar;
pub mod parser;

#[derive(Debug, thiserror::Error)]
pub enum JavaError {
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Zip error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("Other error: {0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, JavaError>;
