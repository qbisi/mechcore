//! `show`: answers one view of what a file holds.
//!
//! A recording has three: `outcome`, what the fight decided; `stats`, a unit's
//! numbers and the corrections behind them; and `buildings`, what is standing.
//! Each is its own reader, because what a fight decided, what was written onto
//! its units and what stands on its board are three different questions.

use std::path::Path;

use serde_json::Value;

use crate::cli::{Args, Failure, Outcome};
use crate::kind::Kind;

/// The views a recording answers, in the order the manual lists them.
pub(crate) const VIEWS: &[&str] = &["outcome", "stats", "buildings"];

/// Reads `show <file> --view <view>` off a command line.
///
/// # Errors
///
/// Returns a usage failure for a command the contract does not define, and a
/// refusal for a kind `show` does not take.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let view = arguments
        .value("--view")?
        .ok_or_else(|| Failure::usage(format!("expected --view {}", VIEWS.join("|"))))?;
    let tick = arguments.parsed::<u32>("--tick", "a tick the recording holds")?;
    let input = arguments.path("a file to show")?;
    arguments.finish()?;
    let (verdict, shown) = show(&input, &view, tick)?;
    crate::cli::emit(&shown, format)?;
    Ok(verdict.into())
}

/// One view of one file, with its verdict.
///
/// Only `outcome` has a verdict of its own: it is no while anything the fight
/// decided is unresolved, because the fight was read and the answer is that it
/// does not settle a round.
///
/// # Errors
///
/// Returns a refusal for a kind `show` does not take, a usage failure for a
/// view the kind has not or an option the view does not take, and whatever
/// reading the view fails with.
pub(crate) fn show(input: &Path, view: &str, tick: Option<u32>) -> Result<(bool, Value), Failure> {
    let (kind, _) = Kind::read(input)?;
    kind.require("show")?;
    let written = |value: Result<Value, serde_json::Error>| {
        value.map_err(|error| Failure::failed(format!("cannot write the result: {error}")))
    };
    match view {
        "outcome" => {
            if tick.is_some() {
                return Err(Failure::usage(
                    "the outcome is the whole fight's; --tick belongs to stats and buildings",
                ));
            }
            let outcome = crate::outcome::read(input)?;
            let settled = outcome.unresolved.is_empty();
            Ok((settled, written(serde_json::to_value(&outcome))?))
        }
        "stats" => Ok((
            true,
            written(serde_json::to_value(crate::stats::read(input, tick)?))?,
        )),
        "buildings" => Ok((
            true,
            written(serde_json::to_value(crate::buildings::read(input, tick)?))?,
        )),
        other => Err(Failure::usage(format!(
            "a recording has no view {other:?}; it has {}",
            VIEWS.join(", ")
        ))),
    }
}
