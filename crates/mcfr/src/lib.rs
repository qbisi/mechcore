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
#[cfg(test)]
mod hashed_content;
mod instrument;
mod model;
mod numbering;
mod parquet_storage;
mod reader;
mod recording;
mod writer;

pub use error::{Error, Result};
pub use instrument::{
    CheckedSkill, ExpRange, INSTRUMENT_TICK_COLUMN, InstrumentRow, PoseClip, ProjectileReach,
    RvoExit, RvoNeighbour, RvoNeighbourKind, RvoSolve, RvoVec, RvoVo, SkillAttackableCheck,
    TARGET_CANDIDATES, TargetCandidate, TargetRefs, TargetSearch, TargetSearchPath, UnitPose,
    valid_channel_name,
};
pub use model::*;
pub use numbering::UnitNumbering;
pub use parquet_storage::tables::{McfrTables, TableOrigin, column_tags};
pub use reader::{McfrReader, Published};
pub use recording::{MemoryRecording, Recording};
pub use writer::McfrWriter;
