//! The request stream a player speaks: one JSON request per line in, one JSON
//! result per line out.
//!
//! `docs/spec/mechcore/cli.md` is the contract. A request is one of `match`'s
//! own operations on a match its caller already holds, so it names neither the
//! document nor the side: `shell --json` holds them because it was opened on
//! them, and an arena holds them because it dealt the match. A player written
//! against one runs under the other unchanged.

use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::{cli::Args, cli::Failure, turn::Side};

/// How long one slice of a wait lasts, so a caller that stops a wait learns it
/// within this.
const SLICE: f64 = 0.5;

/// One request, as a player writes it.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", deny_unknown_fields)]
enum Request {
    /// The caller's view of the match, waiting first when `wait` says so:
    /// `true` waits as long as it takes, and a number of seconds bounds it.
    #[serde(rename = "match.show")]
    Show {
        #[serde(default)]
        wait: Option<Wait>,
    },
    /// One decision, as the action spec writes it, kept unless `dry_run`.
    #[serde(rename = "match.act")]
    Act {
        decision: Value,
        #[serde(default)]
        dry_run: bool,
    },
    /// The side's decisions for this round, written.
    #[serde(rename = "match.commit")]
    Commit {},
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Wait {
    Forever(bool),
    Seconds(f64),
}

/// Answers one request line for the side a caller holds of a match.
///
/// The answer is what the operation answers on a command line, or its error
/// object: a request stream has one place to answer in, so a refusal is an
/// answer there rather than a line on standard error. `stopped` is asked
/// between the slices of a wait, and a wait it stops answers the view as it
/// stands, which is what a wait that reached its own bound answers.
pub(crate) fn answer(path: &Path, side: Side, line: &str, stopped: &dyn Fn() -> bool) -> Value {
    let request = match serde_json::from_str::<Request>(line) {
        Ok(request) => request,
        Err(error) => {
            return Failure::usage(format!(
                "{line:?} is not a request: one of match.show, match.act or match.commit, \
                 as {{\"op\": ...}}: {error}"
            ))
            .object("request");
        }
    };
    let (operation, outcome) = match request {
        Request::Show { wait } => ("match.show", show(path, side, wait.as_ref(), stopped)),
        Request::Act { decision, dry_run } => {
            let mut words = verb("act", path, side);
            if dry_run {
                words.push("--dry-run".into());
            }
            // A decision is read as YAML, which JSON is.
            words.push(decision.to_string());
            ("match.act", crate::r#match::answer(Args::new(words)))
        }
        Request::Commit {} => (
            "match.commit",
            crate::r#match::answer(Args::new(verb("commit", path, side))),
        ),
    };
    outcome.unwrap_or_else(|failure| failure.object(operation))
}

/// A wait, taken in slices so a caller can stop it between them.
fn show(
    path: &Path,
    side: Side,
    wait: Option<&Wait>,
    stopped: &dyn Fn() -> bool,
) -> Result<Value, Failure> {
    let bound = match wait {
        None | Some(Wait::Forever(false)) => {
            return crate::r#match::answer(Args::new(verb("show", path, side)));
        }
        Some(Wait::Forever(true)) => None,
        Some(Wait::Seconds(seconds)) if seconds.is_finite() && *seconds >= 0.0 => Some(*seconds),
        Some(Wait::Seconds(seconds)) => {
            return Err(Failure::usage(format!(
                "wait {seconds} is not a number of seconds"
            )));
        }
    };
    let since = std::time::Instant::now();
    loop {
        let left = bound.map(|bound| bound - since.elapsed().as_secs_f64());
        let slice = left.map_or(SLICE, |left| left.clamp(0.0, SLICE));
        let mut words = verb("show", path, side);
        words.extend(["--wait".into(), slice.to_string()]);
        let view = crate::r#match::answer(Args::new(words))?;
        let waited = view.get("phase").and_then(Value::as_str) == Some("over")
            || !committed(&view, side) && decides(&view);
        if waited || left.is_some_and(|left| left <= SLICE) || stopped() {
            return Ok(view);
        }
    }
}

/// Whether a view says this side has committed the round it stands in.
fn committed(view: &Value, side: Side) -> bool {
    view.pointer(&format!("/sides/{}/committed", side.name()))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Whether a view stands in a phase that takes decisions.
fn decides(view: &Value) -> bool {
    matches!(
        view.get("phase").and_then(Value::as_str),
        Some("opening" | "deploy")
    )
}

/// A verb of the `match` namespace, on the match and side the caller holds.
fn verb(name: &str, path: &Path, side: Side) -> Vec<String> {
    vec![
        name.into(),
        path.display().to_string(),
        "--side".into(),
        side.name().into(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn never() -> bool {
        false
    }

    /// A request that is not one of the three is answered with a usage error
    /// object, on the line where its result would have been.
    #[test]
    fn a_request_that_is_not_one_is_answered_with_an_error() {
        let path = &std::env::temp_dir().join("mechcore-no-such-directory/m.yaml");
        for line in [
            "show",
            r#"{"op": "match.new"}"#,
            r#"{"op": "match.commit", "side": "red"}"#,
            r#"{"op": "match.show", "wait": "soon"}"#,
        ] {
            let answer = answer(path, Side::Blue, line, &never);
            assert_eq!(answer["schema"], "mechcore.error", "{line}");
            assert_eq!(answer["kind"], "usage", "{line}");
        }
    }

    /// A request on a match that is not there is the operation's own refusal,
    /// named by the operation.
    #[test]
    fn a_request_names_the_operation_it_failed_in() {
        let path = &std::env::temp_dir().join("mechcore-no-such-directory/m.yaml");
        let answer = answer(path, Side::Red, r#"{"op": "match.commit"}"#, &never);
        assert_eq!(answer["schema"], "mechcore.error");
        assert_eq!(answer["operation"], "match.commit");
    }
}
