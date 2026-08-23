mod canonical;
mod error;
mod instrumentation;
mod model;
mod reader;
mod writer;

pub use error::{Error, Result};
pub use instrumentation::{
    InstrumentationEntry, InstrumentationReader, InstrumentationRecord, InstrumentationSink,
    InstrumentationWriter, NoInstrumentation,
};
pub use model::*;
pub use reader::McfrReader;
pub use writer::McfrWriter;
