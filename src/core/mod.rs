//! The format layer: reading and writing YSON, with no serde involved.
//!
//! Everything here builds with `default-features = false`. It speaks in bytes
//! rather than `str`, because YSON strings, map keys and attribute names are
//! arbitrary byte strings.
//!
//! Seven pieces, each with one job:
//!
//! | Module | Turns | Into |
//! |---|---|---|
//! | [`mod@format`] | — | which of the two encodings is in play |
//! | [`varint`] | bytes | integers, and back |
//! | [`token`] | — | the lexical units, and their payload-free [`TokenKind`] |
//! | [`reader`] | bytes | [`Token`]s, or a [`node::YsonValue`] tree |
//! | [`scan`] | bytes | a length, building no values at all |
//! | [`frames`] | a stream | one record's bytes at a time |
//! | [`writer`] | tokens and values | bytes |
//!
//! [`node`] holds the tree those pieces pass around, borrowed from the input.
//!
//! ```
//! use yson_rs::core::{Reader, Writer, YsonFormat};
//!
//! let value = Reader::new(b"<a=1>{b=2}", YsonFormat::Text).read_value().unwrap();
//!
//! let mut out = Vec::new();
//! Writer::new(&mut out, YsonFormat::Text).write_value(&value).unwrap();
//! assert_eq!(out, b"<a=1>{b=2}");
//! ```

/// Error types and handling.
pub mod error;
/// Which of the two YSON encodings is in play.
pub mod format;
pub mod frames;
pub mod node;
/// Turning bytes into tokens and values.
pub mod reader;
pub mod scan;
/// The lexical units a [`reader::Reader`] produces.
pub mod token;
/// Varint and zigzag helpers for the binary format.
pub mod varint;
/// Turning tokens and values back into bytes.
pub mod writer;

pub use error::YsonError;
pub use format::YsonFormat;
pub use frames::{FrameReader, Frames};
pub use node::{OwnedYsonValue, YsonKey, YsonMap, YsonNode, YsonValue};
pub use reader::Reader;
pub use scan::{Scan, scan_value};
pub use token::{Token, TokenKind};
pub use writer::Writer;

/// The nesting depth [`Reader::read_value`] and [`Writer::write_value`] allow
/// before they refuse the value.
pub const DEFAULT_MAX_DEPTH: usize = 128;
