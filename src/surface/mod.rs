//! The serde binding.
//!
//! This layer owns the *conventions* — which YSON shape a Rust type takes: a
//! field renamed `@name` is an attribute, a field renamed `$value` is the body,
//! and an enum is a one-key map. The bytes themselves belong to
//! [`crate::core`].
//!
//! Behind the `serde` feature, which is on by default.

pub(crate) mod access;
/// Tools for working with YSON attributes and metadata.
pub mod attributes;
/// Deserialization logic and types.
pub mod de;
/// Serialization logic and types.
pub mod ser;
mod value;
