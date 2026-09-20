//! The file a match in progress keeps beside its document.
//!
//! `docs/spec/mechcore/turn.md` is the contract: what `<match>.turn` holds,
//! how two processes share it, and what is lost when it is. It is coordination
//! and not record — the rounds that have been played are the battle document's
//! — so everything here belongs to the round in progress.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use mechcore_document::battle::Action;
use serde::{Deserialize, Serialize};

use crate::cli::Failure;

pub(crate) const SCHEMA: &str = "mechcore.match-turn.v1";

/// Which side of a match, which is what an operation names itself by.
///
/// A side is given rather than claimed, so this says who is calling; nothing
/// here stops a process naming the side it was not given, and
/// `docs/spec/mechcore/cli.md` says why that is not a permission system.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Side {
    Blue,
    Red,
}

impl Side {
    pub(crate) const BOTH: [Self; 2] = [Self::Blue, Self::Red];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Red => "red",
        }
    }

    /// Whether this is the side the board's rules call red, which decides
    /// where what a decision hands out lands.
    pub(crate) const fn red(self) -> bool {
        matches!(self, Self::Red)
    }

    pub(crate) const fn other(self) -> Self {
        match self {
            Self::Blue => Self::Red,
            Self::Red => Self::Blue,
        }
    }

    /// The seat the map deals this side, which its reactor core is read by.
    pub(crate) const fn seat(self) -> usize {
        match self {
            Self::Blue => 0,
            Self::Red => 1,
        }
    }

    pub(crate) fn parse(name: &str) -> Result<Self, Failure> {
        match name {
            "blue" => Ok(Self::Blue),
            "red" => Ok(Self::Red),
            other => Err(Failure::usage(format!("side {other:?} is not blue or red"))),
        }
    }
}

/// The round in progress, as a turn file states it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Turn {
    pub(crate) schema: String,
    pub(crate) round: i32,
    /// When the round opened, which the deployment clock is measured from.
    pub(crate) opened: String,
    /// Whether this round's file was rebuilt rather than carried, so a side
    /// waiting on it learns why the wait grew.
    pub(crate) rebuilt: bool,
    pub(crate) sides: Sides,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sides {
    pub(crate) blue: SideTurn,
    pub(crate) red: SideTurn,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SideTurn {
    /// Whether this side has been handed to a player; a third caller is
    /// refused.
    pub(crate) given: bool,
    /// Whether this side has written this round's decisions to the document.
    pub(crate) committed: bool,
    /// What this side has decided this round and not committed, in order.
    pub(crate) decisions: Vec<Action>,
}

impl Turn {
    /// A round that has just opened, which is every side's clock restarted and
    /// nothing decided.
    pub(crate) fn opening(round: i32, given: [bool; 2], rebuilt: bool) -> Self {
        let side = |given: bool| SideTurn {
            given,
            committed: false,
            decisions: Vec::new(),
        };
        Self {
            schema: SCHEMA.to_owned(),
            round,
            opened: crate::instant::now(),
            rebuilt,
            sides: Sides {
                blue: side(given[0]),
                red: side(given[1]),
            },
        }
    }

    pub(crate) const fn side(&self, side: Side) -> &SideTurn {
        match side {
            Side::Blue => &self.sides.blue,
            Side::Red => &self.sides.red,
        }
    }

    pub(crate) const fn side_mut(&mut self, side: Side) -> &mut SideTurn {
        match side {
            Side::Blue => &mut self.sides.blue,
            Side::Red => &mut self.sides.red,
        }
    }

    pub(crate) fn given(&self) -> [bool; 2] {
        [self.sides.blue.given, self.sides.red.given]
    }

    /// How long this round has been open, in seconds.
    ///
    /// # Errors
    ///
    /// Returns an error when `opened` is not an instant this platform writes.
    pub(crate) fn age(&self) -> Result<f64, Failure> {
        crate::instant::elapsed(&self.opened).map_err(|error| {
            Failure::failed(format!("turn file states no readable clock: {error}"))
        })
    }
}

/// A turn file held open with its lock taken.
///
/// The lock is the file's own, so it lives exactly as long as this does: an
/// operation takes it, reads, decides, writes, and drops it. A process that
/// dies holding one leaves nothing behind, because the operating system
/// releases a lock with the file that carried it.
pub(crate) struct Held {
    file: File,
    path: PathBuf,
}

impl Held {
    /// Opens the turn file beside a match and takes its lock, waiting for
    /// whoever holds it.
    ///
    /// The file is created when it is not there, so the caller that deals a
    /// match and the caller that joins one take the same lock.
    ///
    /// # Errors
    ///
    /// Returns a failure when the file cannot be opened or the lock cannot be
    /// taken.
    pub(crate) fn take(path: &Path) -> Result<Self, Failure> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|error| Failure::failed(format!("cannot open {}: {error}", path.display())))?;
        lock(&file)
            .map_err(|error| Failure::failed(format!("cannot lock {}: {error}", path.display())))?;
        Ok(Self {
            file,
            path: path.to_owned(),
        })
    }

    /// What the file holds, or nothing when it is empty, which is the file a
    /// caller creates by taking the lock on a match that has none.
    ///
    /// A half-written file is unreadable rather than guessed at, and a caller
    /// that finds one rebuilds the round from the document.
    ///
    /// # Errors
    ///
    /// Returns a failure when the file cannot be read.
    pub(crate) fn read(&mut self) -> Result<Option<Turn>, Failure> {
        let mut text = String::new();
        self.file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.read_to_string(&mut text))
            .map_err(|error| {
                Failure::failed(format!("cannot read {}: {error}", self.path.display()))
            })?;
        if text.trim().is_empty() {
            return Ok(None);
        }
        let turn: Turn = match serde_json::from_str(&text) {
            Ok(turn) => turn,
            Err(_) => return Ok(None),
        };
        if turn.schema != SCHEMA {
            return Ok(None);
        }
        Ok(Some(turn))
    }

    /// Writes the round in progress, in the normal form the contract states.
    ///
    /// The file is written in place rather than replaced, because the lock
    /// belongs to the file the other process opened.
    ///
    /// # Errors
    ///
    /// Returns a failure when the file cannot be written.
    pub(crate) fn write(&mut self, turn: &Turn) -> Result<(), Failure> {
        let mut text = serde_json::to_string_pretty(turn)
            .map_err(|error| Failure::failed(format!("cannot write the round: {error}")))?;
        text.push('\n');
        self.file
            .seek(SeekFrom::Start(0))
            .and_then(|_| self.file.set_len(0))
            .and_then(|()| self.file.write_all(text.as_bytes()))
            .and_then(|()| self.file.flush())
            .map_err(|error| {
                Failure::failed(format!("cannot write {}: {error}", self.path.display()))
            })
    }

    /// Takes the file away, which is what the end of a match does with it.
    ///
    /// # Errors
    ///
    /// Returns a failure when the file cannot be removed.
    pub(crate) fn remove(&mut self) -> Result<(), Failure> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Failure::failed(format!(
                "cannot remove {}: {error}",
                self.path.display()
            ))),
        }
    }
}

/// Takes the file's advisory lock, waiting for whoever holds it.
fn lock(file: &File) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    // SAFETY: the descriptor is the open file's own and outlives the call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
        return Ok(());
    }
    Err(std::io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::{Held, SCHEMA, Side, Turn};

    fn scratch(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("mechcore-turn-{name}"));
        let _ = std::fs::remove_file(&path);
        path
    }

    /// The file a caller writes is the file the other caller reads, and the
    /// contract's keys are in the contract's order.
    #[test]
    fn a_round_reads_back_as_itself() {
        let path = scratch("round");
        let mut held = Held::take(&path).unwrap();
        assert!(held.read().unwrap().is_none(), "a new file holds nothing");
        let mut turn = Turn::opening(3, [true, false], false);
        turn.side_mut(Side::Blue).committed = true;
        held.write(&turn).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.ends_with("}\n"), "{written}");
        assert!(written.contains("\n  \"round\": 3,"), "{written}");
        let keys: Vec<&str> = written
            .lines()
            .filter_map(|line| line.strip_prefix("  \""))
            .filter_map(|line| line.split('"').next())
            .collect();
        assert_eq!(keys, ["schema", "round", "opened", "rebuilt", "sides"]);

        let read = held.read().unwrap().unwrap();
        assert_eq!(read.schema, SCHEMA);
        assert_eq!(read.round, 3);
        assert_eq!(read.given(), [true, false]);
        assert!(read.side(Side::Blue).committed);
        assert!(!read.side(Side::Red).committed);
        assert!(read.age().unwrap() < 5.0);
        std::fs::remove_file(&path).unwrap();
    }

    /// A file this platform did not write is a file to rebuild from the
    /// document, not one to guess at.
    #[test]
    fn a_half_written_or_foreign_file_holds_nothing() {
        let path = scratch("foreign");
        for text in [
            "{\"schema\": \"mechcore.match-turn.v1\", \"round\": 3, \"op",
            "{\"schema\": \"something.else.v1\", \"round\": 3, \"opened\": \"\", \
             \"rebuilt\": false, \"sides\": {}}",
            "not json at all",
        ] {
            std::fs::write(&path, text).unwrap();
            let mut held = Held::take(&path).unwrap();
            assert!(held.read().unwrap().is_none(), "{text}");
        }
        std::fs::remove_file(&path).unwrap();
    }

    /// Writing a shorter round over a longer one leaves none of the longer
    /// one behind, which an in-place write has to be told to do.
    #[test]
    fn a_write_replaces_what_the_file_held() {
        let path = scratch("shorter");
        let mut held = Held::take(&path).unwrap();
        let mut long = Turn::opening(1, [true, true], false);
        long.side_mut(Side::Red).decisions =
            serde_json::from_str(r#"[{"type": "unlock_unit", "name": "fang"}]"#).unwrap();
        held.write(&long).unwrap();
        let short = Turn::opening(2, [true, true], true);
        held.write(&short).unwrap();
        let read = held.read().unwrap().unwrap();
        assert_eq!(read.round, 2);
        assert!(read.rebuilt);
        assert!(read.side(Side::Red).decisions.is_empty());
        std::fs::remove_file(&path).unwrap();
    }
}
