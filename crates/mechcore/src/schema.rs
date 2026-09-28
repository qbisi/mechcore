//! `schema`: answers the JSON Schema of a document kind.

use crate::cli::{Args, Failure, Outcome, Verdict};

/// Answers the JSON Schema of each kind named.
///
/// A document names its own kind, and these are the kinds it can name. The
/// schema is the shape a reader validates against and a writer generates from;
/// what the fields mean is the document's own spec, which `man` answers with.
///
/// # Errors
///
/// Returns a usage failure for no kind, or for a kind no document names.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let kinds = arguments.operands()?;
    if kinds.is_empty() {
        return Err(Failure::usage(
            "expected <kind>...: layout, match, state or action",
        ));
    }
    for kind in kinds {
        let schema = match kind.as_str() {
            "layout" => schemars::schema_for!(mechcore_document::Layout),
            "match" => schemars::schema_for!(mechcore_document::r#match::schema::Match),
            "state" => schemars::schema_for!(mechcore_document::r#match::schema::State),
            "action" => schemars::schema_for!(mechcore_document::r#match::schema::Actions),
            other => {
                return Err(Failure::usage(format!(
                    "no document is of kind {other:?}; a document names itself \
                     layout, match, state or action"
                )));
            }
        };
        println!(
            "{}",
            serde_json::to_string(&schema)
                .map_err(|error| Failure::failed(format!("cannot write the schema: {error}")))?
        );
    }
    Ok(Verdict::Yes)
}
