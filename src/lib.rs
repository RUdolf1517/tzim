//! Tzim internals.
//!
//! The command line binary is a thin shell around this crate. The library
//! surface exists so that the parsers can be exercised directly — by the fuzz
//! targets in `fuzz/` and by nothing else. It is **not** a stable API: anything
//! here may change between releases without notice.

#![forbid(unsafe_code)]

pub mod cli;
pub mod detect;
pub mod error;
pub mod formats;
pub mod pipeline;
pub mod pool;
pub mod progress;
pub mod replace;
pub mod report;
pub mod style;
pub mod walk;

#[cfg(feature = "fuzz-api")]
pub mod fuzz_api;
