//! `format`: writes a document in its normal form.

use std::fs;

use crate::cli::{Args, Failure, Outcome, Verdict};
use crate::kind::Kind;

/// Writes the document in its normal form, on standard output or in place
/// with `--write`.
///
/// # Errors
///
/// Returns a usage failure for a command the contract does not define, and a
/// refusal for a kind `format` does not take or a document that does not read.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let write = arguments.flag("--write")?;
    let path = arguments.path("a document to format")?;
    arguments.finish()?;
    let (kind, bytes) = Kind::read(&path)?;
    kind.require("format")?;
    let canonical = match kind {
        Kind::Fight => mechcore_document::fight::parse_yaml(&bytes)
            .and_then(mechcore_document::fight::canonical_yaml),
        _ => mechcore_document::parse_yaml(&bytes).and_then(mechcore_document::canonical_yaml),
    }
    .map_err(Failure::refused)?;
    if write {
        fs::write(&path, canonical).map_err(|error| {
            Failure::failed(format!("cannot write {}: {error}", path.display()))
        })?;
    } else {
        print!("{canonical}");
    }
    Ok(Verdict::Yes)
}
