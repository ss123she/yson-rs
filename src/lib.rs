#[cfg(all(not(debug_assertions), not(test)))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

pub mod access;
pub mod attributes;
pub mod de;
pub mod error;
pub mod lexer;
pub mod node;
pub mod parser;
pub mod ser;
pub mod varint;
