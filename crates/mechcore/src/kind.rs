//! What a file is, read from what it holds, and which verbs take it.
//!
//! A verb over files has one meaning for every kind it accepts, so the kind
//! decides only which code carries that meaning out. It is read from the
//! content, never from a flag or an extension: a YAML document names itself
//! in `kind`, an MCFR is a ZIP, and a GRBR opens with the serialization header
//! .NET's `BinaryFormatter` writes.

use std::path::Path;

use crate::cli::Failure;

/// A kind of file the command line reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Layout,
    Fight,
    Match,
    State,
    Action,
    Mcfr,
    Grbr,
}

/// How a conversion reaches the kind it writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Conversion {
    /// The same content in another form: lossless, or refused.
    Rewrite,
    /// Facts the input does not hold, derived by running the fight.
    Computation,
}

impl Conversion {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Rewrite => "rewrite",
            Self::Computation => "computation",
        }
    }
}

/// `BinaryFormatter`'s serialization header: record type 0, root id 1,
/// header id -1, format version 1.0.
const GRBR_HEADER: [u8; 17] = [
    0x00, 0x01, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00,
];

/// A ZIP's local file header, which opens every MCFR.
const MCFR_HEADER: [u8; 4] = *b"PK\x03\x04";

impl Kind {
    pub(crate) const ALL: [Self; 7] = [
        Self::Layout,
        Self::Fight,
        Self::Match,
        Self::State,
        Self::Action,
        Self::Mcfr,
        Self::Grbr,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Layout => "layout",
            Self::Fight => "fight",
            Self::Match => "match",
            Self::State => "state",
            Self::Action => "action",
            Self::Mcfr => "mcfr",
            Self::Grbr => "grbr",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// The kind a file's bytes are.
    ///
    /// # Errors
    ///
    /// Returns an error when the bytes are neither binary kind and no YAML
    /// document naming a kind this build reads.
    pub(crate) fn of(bytes: &[u8]) -> Result<Self, String> {
        if bytes.starts_with(&MCFR_HEADER) {
            return Ok(Self::Mcfr);
        }
        if bytes.starts_with(&GRBR_HEADER) {
            return Ok(Self::Grbr);
        }
        let first = serde_yaml::Deserializer::from_slice(bytes)
            .next()
            .and_then(|document| {
                <serde_yaml::Value as serde::Deserialize>::deserialize(document).ok()
            })
            .ok_or("neither a recording, a replay nor a YAML document")?;
        let named = first
            .get("kind")
            .and_then(serde_yaml::Value::as_str)
            .ok_or("the document does not name its kind: it starts with `kind: <kind>`")?;
        Self::parse(named)
            .filter(|kind| !matches!(kind, Self::Mcfr | Self::Grbr))
            .ok_or_else(|| format!("no document is of kind {named:?}"))
    }

    /// The kind of the file at `path`, and its bytes.
    ///
    /// A recording is opened by its reader rather than held in memory, so an
    /// MCFR answers no bytes; every other kind answers the whole file.
    ///
    /// # Errors
    ///
    /// Returns a failure when the file cannot be read and a refusal when it is
    /// no kind this build reads.
    pub(crate) fn read(path: &Path) -> Result<(Self, Vec<u8>), Failure> {
        use std::io::Read;
        if path.is_dir() {
            return Err(Failure::usage(format!(
                "{} is a directory; name the files themselves",
                path.display()
            )));
        }
        let unreadable = |error: std::io::Error| {
            Failure::failed(format!("cannot read {}: {error}", path.display()))
        };
        let mut file = std::fs::File::open(path).map_err(unreadable)?;
        let mut bytes = Vec::new();
        file.by_ref()
            .take(MCFR_HEADER.len() as u64)
            .read_to_end(&mut bytes)
            .map_err(unreadable)?;
        if bytes.starts_with(&MCFR_HEADER) {
            return Ok((Self::Mcfr, Vec::new()));
        }
        file.read_to_end(&mut bytes).map_err(unreadable)?;
        let kind = Self::of(&bytes)
            .map_err(|error| Failure::refused(format!("{}: {error}", path.display())))?;
        Ok((kind, bytes))
    }

    /// The verbs a file of this kind takes, `convert` with the kinds it
    /// reaches.
    pub(crate) const fn verbs(self) -> &'static [&'static str] {
        match self {
            Self::Layout | Self::Fight => &["verify", "convert", "diff", "play", "format"],
            Self::Match => &["verify", "convert"],
            Self::State | Self::Action => &[],
            Self::Mcfr => &["verify", "convert", "diff", "show", "query", "play"],
            Self::Grbr => &["convert"],
        }
    }

    /// The kinds this one converts to, and how.
    pub(crate) const fn conversions(self) -> &'static [(Self, Conversion)] {
        match self {
            Self::Layout => &[
                (Self::Grbr, Conversion::Rewrite),
                (Self::Mcfr, Conversion::Computation),
                (Self::Fight, Conversion::Computation),
            ],
            Self::Match => &[
                (Self::Grbr, Conversion::Rewrite),
                (Self::Layout, Conversion::Rewrite),
            ],
            // A replay's round is fought by the game alone; the simulator does
            // not open a replay.
            Self::Grbr => &[
                (Self::Match, Conversion::Rewrite),
                (Self::Mcfr, Conversion::Computation),
                (Self::Fight, Conversion::Computation),
            ],
            // What a recording holds of its fight, written onto the layout it
            // embeds: read, not computed.
            Self::Mcfr => &[(Self::Fight, Conversion::Rewrite)],
            // A fight is fought again from its projection and its own seed.
            Self::Fight => &[(Self::Mcfr, Conversion::Computation)],
            Self::State | Self::Action => &[],
        }
    }

    /// How this kind converts to `to`, if it does.
    pub(crate) fn conversion(self, to: Self) -> Option<Conversion> {
        self.conversions()
            .iter()
            .find(|(kind, _)| *kind == to)
            .map(|(_, how)| *how)
    }

    /// Refuses a verb this kind does not take, naming the kind.
    ///
    /// # Errors
    ///
    /// Returns a refusal naming the kind and the verbs it does take.
    pub(crate) fn require(self, verb: &str) -> Result<(), Failure> {
        if self.verbs().contains(&verb) {
            return Ok(());
        }
        Err(Failure::refused(format!(
            "{verb} does not take a {} file; {}",
            self.name(),
            self.takes()
        )))
    }

    /// What this kind takes, as a sentence's second half.
    pub(crate) fn takes(self) -> String {
        match self.verbs() {
            [] => format!("no verb takes a {} file", self.name()),
            verbs => format!("a {} file takes {}", self.name(), verbs.join(", ")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Conversion, Kind};

    /// The file verbs the command line holds.
    const VERBS: &[&str] = &[
        "verify", "convert", "diff", "show", "query", "play", "format",
    ];

    #[test]
    fn a_kind_is_read_from_the_content() {
        assert_eq!(Kind::of(b"kind: layout\nround: 1\n").unwrap(), Kind::Layout);
        assert_eq!(Kind::of(b"kind: fight\nround: 1\n").unwrap(), Kind::Fight);
        assert_eq!(
            Kind::of(b"kind: match\ngame_build: x\n---\nkind: action\n").unwrap(),
            Kind::Match
        );
        assert_eq!(Kind::of(b"PK\x03\x04rest").unwrap(), Kind::Mcfr);
        let mut grbr = super::GRBR_HEADER.to_vec();
        grbr.extend_from_slice(b"\x0c\x02");
        assert_eq!(Kind::of(&grbr).unwrap(), Kind::Grbr);
        assert!(Kind::of(b"round: 1\n").unwrap_err().contains("kind"));
        assert!(Kind::of(b"kind: mcfr\n").is_err());
        assert!(Kind::of(b"kind: novel\n").unwrap_err().contains("novel"));
    }

    /// A kind converts only through `convert`, and every verb it names is one
    /// the command line holds.
    #[test]
    fn the_verb_table_is_closed() {
        for kind in Kind::ALL {
            for verb in kind.verbs() {
                assert!(VERBS.contains(verb), "{verb}");
            }
            assert_eq!(
                kind.verbs().contains(&"convert"),
                !kind.conversions().is_empty(),
                "{kind:?}"
            );
            assert!(kind.require("schema").is_err());
        }
        assert_eq!(
            Kind::Layout.conversion(Kind::Mcfr),
            Some(Conversion::Computation)
        );
        assert_eq!(Kind::Mcfr.conversion(Kind::Layout), None);
        assert_eq!(
            Kind::Mcfr.conversion(Kind::Fight),
            Some(Conversion::Rewrite)
        );
        assert_eq!(
            Kind::Layout.conversion(Kind::Fight),
            Some(Conversion::Computation)
        );
        assert_eq!(
            Kind::Fight.conversion(Kind::Mcfr),
            Some(Conversion::Computation)
        );
        assert_eq!(
            Kind::Grbr.conversion(Kind::Mcfr),
            Some(Conversion::Computation)
        );
        assert!(Kind::Fight.require("verify").is_ok());
        assert!(
            Kind::State
                .require("verify")
                .unwrap_err()
                .reason()
                .contains("state")
        );
    }
}
