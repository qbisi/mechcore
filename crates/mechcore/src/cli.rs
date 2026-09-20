//! What every command shares: how a result leaves, and how a failure is named.
//!
//! `docs/spec/mechcore/cli.md` is the contract. A result is one JSON object on
//! standard output; a failure is one error object on standard error and an
//! exit code that says who has to fix it.

use std::{path::PathBuf, process::ExitCode};

use serde::Serialize;

/// Why an operation did not happen.
///
/// The kind is what a caller acts on: a command it must rewrite, a request the
/// rules refuse, an environment it may wait for, or a failure that is neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Usage,
    Refused,
    Unavailable,
    Failed,
}

impl Kind {
    const fn code(self) -> u8 {
        match self {
            Self::Usage => 2,
            Self::Refused => 3,
            Self::Unavailable => 4,
            Self::Failed => 5,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Refused => "refused",
            Self::Unavailable => "unavailable",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug)]
pub(crate) struct Failure {
    kind: Kind,
    reason: String,
    /// The operation this failed in, when it is narrower than the namespace
    /// the command line dispatched.
    operation: Option<String>,
}

impl Failure {
    pub(crate) fn usage(reason: impl Into<String>) -> Self {
        Self::new(Kind::Usage, reason)
    }

    pub(crate) fn refused(reason: impl Into<String>) -> Self {
        Self::new(Kind::Refused, reason)
    }

    pub(crate) fn unavailable(reason: impl Into<String>) -> Self {
        Self::new(Kind::Unavailable, reason)
    }

    pub(crate) fn failed(reason: impl Into<String>) -> Self {
        Self::new(Kind::Failed, reason)
    }

    fn new(kind: Kind, reason: impl Into<String>) -> Self {
        Self {
            kind,
            reason: reason.into(),
            operation: None,
        }
    }

    /// Names the operation this failed in, which a namespace knows and the
    /// dispatcher does not.
    ///
    /// The first name given stands: a verb names itself, and a namespace does
    /// not rename what a verb already named.
    pub(crate) fn at(mut self, operation: impl Into<String>) -> Self {
        self.operation.get_or_insert_with(|| operation.into());
        self
    }

    /// Why the operation did not happen, for a caller that carries the reason
    /// into a failure of its own.
    pub(crate) fn reason(&self) -> &str {
        &self.reason
    }

    /// Writes the failure as the contract's error object.
    ///
    /// A prompt writes one and reads the next line; a command writes one and
    /// exits, and both say the same thing in the same place.
    pub(crate) fn write(&self, operation: &str) {
        let error = serde_json::json!({
            "schema": "mechcore.error.v1",
            "kind": self.kind.name(),
            "operation": self.operation.as_deref().unwrap_or(operation),
            "reason": self.reason,
        });
        eprintln!("{error}");
    }

    /// Writes the failure and answers the exit code its kind decides.
    fn report(&self, operation: &str) -> ExitCode {
        self.write(operation);
        ExitCode::from(self.kind.code())
    }
}

/// What an operation answered, when the answer itself may be no.
///
/// A no is an answer rather than a failure: a document that does not verify
/// and two recordings that differ are both reported, and the exit code carries
/// the verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Yes,
    No,
}

impl From<bool> for Verdict {
    fn from(yes: bool) -> Self {
        if yes { Self::Yes } else { Self::No }
    }
}

pub(crate) type Outcome = Result<Verdict, Failure>;

/// Turns an operation's outcome into the process's exit code.
pub(crate) fn exit(operation: &str, outcome: Outcome) -> ExitCode {
    match outcome {
        Ok(Verdict::Yes) => ExitCode::SUCCESS,
        Ok(Verdict::No) => ExitCode::FAILURE,
        Err(failure) => failure.report(operation),
    }
}

/// How a result is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Format {
    #[default]
    Json,
    Yaml,
    Text,
}

impl Format {
    fn parse(value: &str) -> Result<Self, Failure> {
        match value {
            "json" => Ok(Self::Json),
            "yaml" => Ok(Self::Yaml),
            "text" => Ok(Self::Text),
            other => Err(Failure::usage(format!(
                "format {other:?} is not json, yaml or text"
            ))),
        }
    }
}

/// Writes one result, in the format the caller asked for.
///
/// `text` belongs to the operation that has a rendering for it; an operation
/// that has none refuses it here rather than inventing one.
pub(crate) fn emit<T: Serialize>(value: &T, format: Format) -> Result<(), Failure> {
    let written = match format {
        Format::Json => serde_json::to_string_pretty(value)
            .map_err(|error| Failure::failed(format!("cannot write the result: {error}")))?,
        Format::Yaml => serde_yaml::to_string(value)
            .map_err(|error| Failure::failed(format!("cannot write the result: {error}")))?,
        Format::Text => {
            return Err(Failure::usage("this operation has no text rendering"));
        }
    };
    println!("{}", written.trim_end());
    Ok(())
}

/// A command line, read as the argument object the contract defines.
///
/// Options are taken out wherever they stand, so `--force` before an operand
/// and after it are the same command, and what is left over are the operands
/// in order.
pub(crate) struct Args {
    items: Vec<String>,
}

impl Args {
    pub(crate) fn new(items: impl IntoIterator<Item = String>) -> Self {
        Self {
            items: items.into_iter().collect(),
        }
    }

    /// The next operand, which is never an option.
    pub(crate) fn operand(&mut self, what: &str) -> Result<String, Failure> {
        match self.items.first() {
            Some(first) if first.starts_with('-') => {
                Err(Failure::usage(format!("expected {what}, found {first:?}")))
            }
            Some(_) => Ok(self.items.remove(0)),
            None => Err(Failure::usage(format!("expected {what}"))),
        }
    }

    pub(crate) fn path(&mut self, what: &str) -> Result<PathBuf, Failure> {
        self.operand(what).map(PathBuf::from)
    }

    /// Every remaining operand, which leaves the options to be read first.
    pub(crate) fn operands(&mut self) -> Result<Vec<String>, Failure> {
        if let Some(option) = self.items.iter().find(|item| item.starts_with('-')) {
            return Err(Failure::usage(format!("unknown option {option:?}")));
        }
        Ok(std::mem::take(&mut self.items))
    }

    /// Whether a boolean option was given, taking it out of the line.
    pub(crate) fn flag(&mut self, name: &str) -> Result<bool, Failure> {
        let mut found = false;
        while let Some(at) = self.items.iter().position(|item| item == name) {
            if found {
                return Err(Failure::usage(format!("option {name} is duplicated")));
            }
            self.items.remove(at);
            found = true;
        }
        Ok(found)
    }

    /// An option's value, taking both out of the line.
    pub(crate) fn value(&mut self, name: &str) -> Result<Option<String>, Failure> {
        let Some(at) = self.items.iter().position(|item| item == name) else {
            return Ok(None);
        };
        self.items.remove(at);
        if self.items.len() <= at {
            return Err(Failure::usage(format!("option {name} requires a value")));
        }
        let value = self.items.remove(at);
        if self.items.iter().any(|item| item == name) {
            return Err(Failure::usage(format!("option {name} is duplicated")));
        }
        Ok(Some(value))
    }

    /// An option whose value may be left out, such as `--wait [<seconds>]`.
    ///
    /// Answers nothing when the option is absent, `Some(None)` when it stands
    /// alone, and the number when one follows it. Only a number is taken as
    /// the value, because options are read before operands and `--wait` before
    /// a path would otherwise swallow the path.
    // The three cases are the three a caller means: no option, the option
    // alone, and the option with a number. An enum of three would be read back
    // out into these same three arms.
    #[allow(clippy::option_option)]
    pub(crate) fn optional_number(&mut self, name: &str) -> Result<Option<Option<f64>>, Failure> {
        let Some(at) = self.items.iter().position(|item| item == name) else {
            return Ok(None);
        };
        self.items.remove(at);
        if self.items.iter().any(|item| item == name) {
            return Err(Failure::usage(format!("option {name} is duplicated")));
        }
        let Some(value) = self.items.get(at).and_then(|item| item.parse::<f64>().ok()) else {
            return Ok(Some(None));
        };
        self.items.remove(at);
        Ok(Some(Some(value)))
    }

    /// An option's value, read as whatever it names.
    pub(crate) fn parsed<T: std::str::FromStr>(
        &mut self,
        name: &str,
        what: &str,
    ) -> Result<Option<T>, Failure> {
        self.value(name)?
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| Failure::usage(format!("{name} {value:?} is not {what}")))
            })
            .transpose()
    }

    pub(crate) fn format(&mut self) -> Result<Format, Failure> {
        match self.value("--format")? {
            Some(value) => Format::parse(&value),
            None => Ok(Format::default()),
        }
    }

    /// Whether anything is left to read.
    pub(crate) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The line as it stands, for a command that parses its own arguments.
    pub(crate) fn into_strings(self) -> impl Iterator<Item = String> {
        self.items.into_iter()
    }

    /// Refuses whatever is left, so a mistyped option is never ignored.
    pub(crate) fn finish(self) -> Result<(), Failure> {
        match self.items.first() {
            Some(item) if item.starts_with('-') => {
                Err(Failure::usage(format!("unknown option {item:?}")))
            }
            Some(item) => Err(Failure::usage(format!("unexpected argument {item:?}"))),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Args, Format};

    fn args(items: &[&str]) -> Args {
        Args::new(items.iter().map(|item| (*item).to_owned()))
    }

    /// An option is read wherever it stands, and the operands keep their order.
    #[test]
    fn options_are_taken_out_wherever_they_stand() {
        let mut arguments = args(&["--seed", "7", "left.mcfr", "--force", "right.mcfr"]);
        assert_eq!(
            arguments.parsed::<i32>("--seed", "an integer").unwrap(),
            Some(7)
        );
        assert!(arguments.flag("--force").unwrap());
        assert!(!arguments.flag("--write").unwrap());
        assert_eq!(
            arguments.path("left").unwrap().display().to_string(),
            "left.mcfr"
        );
        assert_eq!(arguments.operands().unwrap(), vec!["right.mcfr".to_owned()]);
    }

    #[test]
    fn a_duplicated_option_a_missing_value_and_a_stray_argument_are_usage() {
        assert!(args(&["--force", "--force"]).flag("--force").is_err());
        assert!(args(&["--seed"]).value("--seed").is_err());
        assert!(
            args(&["--seed", "1", "--seed", "2"])
                .value("--seed")
                .is_err()
        );
        assert!(
            args(&["--seed", "x"])
                .parsed::<i32>("--seed", "an integer")
                .is_err()
        );
        assert!(args(&["--typo"]).finish().is_err());
        assert!(args(&["extra"]).finish().is_err());
        assert!(args(&["--typo"]).operand("a path").is_err());
    }

    #[test]
    fn a_format_is_one_of_three() {
        assert_eq!(args(&[]).format().unwrap(), Format::Json);
        assert_eq!(args(&["--format", "yaml"]).format().unwrap(), Format::Yaml);
        assert!(args(&["--format", "toml"]).format().is_err());
    }
}
