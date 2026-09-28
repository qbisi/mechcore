//! Shared MCFR data model, canonicalization, Parquet writer, reader, validation,
//! and semantic hashing.
//!
//! Recording producers such as the injected game adapter and the deterministic
//! simulation call [`McfrWriter`] directly. Consumers call [`McfrReader`]
//! directly, or read a fight through [`Recording`], which a timeline kept in
//! memory answers too; MCP orchestration is not part of the file-format
//! boundary.

mod canonical;
mod error;
mod event_table;
mod instrument;
mod model;
mod parquet_storage;
mod reader;
mod recording;
mod writer;

pub use error::{Error, Result};
pub use instrument::{
    CheckedSkill, GroupSlot, INSTRUMENT_TICK_COLUMN, InstrumentRow, RvoExit, RvoNeighbour,
    RvoNeighbourKind, RvoSolve, RvoVec, RvoVo, SkillAttackableCheck, TARGET_CANDIDATES,
    TargetCandidate, TargetRefs, TargetSearch, TargetSearchPath, valid_channel_name,
};
pub use model::*;
pub use reader::McfrReader;
pub use recording::{MemoryRecording, Recording};
pub use writer::McfrWriter;
