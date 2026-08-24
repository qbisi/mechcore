//! Shared MCFR data model, canonicalization, HDF5 writer, reader, validation,
//! and semantic hashing.
//!
//! Recording producers such as the injected game adapter and the deterministic
//! simulation call [`McfrWriter`] directly. Consumers call [`McfrReader`]
//! directly; MCP orchestration is not part of the file-format boundary.

mod canonical;
mod error;
mod instrumentation;
mod model;
mod reader;
mod storage;
mod writer;

pub use error::{Error, Result};
pub use instrumentation::{
    InstrumentationEntry, InstrumentationReader, InstrumentationRecord, InstrumentationSink,
    InstrumentationWriter, NoInstrumentation,
};
pub use model::*;
pub use reader::McfrReader;
pub use writer::McfrWriter;
