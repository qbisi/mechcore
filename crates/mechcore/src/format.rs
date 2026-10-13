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
    let mut canonical = match kind {
        Kind::Fight => mechcore_document::fight::parse_yaml(&bytes).and_then(|mut fight| {
            let mut formatter = syntaqlite::Formatter::new();
            for (index, assertion) in fight.asserts.iter_mut().enumerate() {
                assertion.sql = formatter.format(&assertion.sql).map_err(|error| {
                    format!("asserts[{index}].sql is not readable SQLite SQL: {error}")
                })?;
            }
            mechcore_document::fight::canonical_yaml(fight)
        }),
        _ => mechcore_document::parse_yaml(&bytes).and_then(mechcore_document::canonical_yaml),
    }
    .map_err(Failure::refused)?;
    let header = std::str::from_utf8(comment_header(&bytes))
        .map_err(|error| Failure::refused(format!("invalid YAML comment header: {error}")))?;
    canonical.insert_str(0, header);
    if write {
        fs::write(&path, canonical).map_err(|error| {
            Failure::failed(format!("cannot write {}: {error}", path.display()))
        })?;
    } else {
        print!("{canonical}");
    }
    Ok(Verdict::Yes)
}

/// Preserve the initial comment block verbatim. Leading blank lines alone
/// are formatting, so they survive only when the block contains a comment.
fn comment_header(bytes: &[u8]) -> &[u8] {
    let mut end = 0;
    let mut has_comment = false;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        match line.iter().find(|byte| !byte.is_ascii_whitespace()) {
            Some(b'#') => has_comment = true,
            None => {}
            _ => break,
        }
        end += line.len();
    }
    if has_comment { &bytes[..end] } else { &[] }
}
