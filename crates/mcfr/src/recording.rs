//! What a reader of one fight reads, wherever the fight is held.
//!
//! A fight the game played is read off an MCFR on disk; a fight the simulator
//! just ran is still in memory, and writing it out only to read it back would
//! make every reader pay for a file nobody keeps. Both answer [`Recording`],
//! so what reads a fight is written once against it.

use crate::{Error, Hashes, McfrReader, Producer, Result, TransitionEvents, WorldSnapshot};

/// One fight's timeline: the layout it started from, and each tick's state
/// and the events that led to it, from `S(1)` to the terminal tick.
pub trait Recording {
    /// What produced the fight: the game or the simulator.
    fn producer(&self) -> Producer;

    /// The canonical layout the fight started from, under its resolved seed.
    fn layout_yaml(&self) -> &str;

    fn hashes(&self) -> &Hashes;

    /// The last tick, which is the tick count: a recording holds every tick
    /// from 1 to it.
    fn terminal_tick(&self) -> u32;

    /// The state at one tick.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range or a stored value is
    /// malformed.
    fn state(&self, tick: u32) -> Result<WorldSnapshot>;

    /// The events that led to one tick's state.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range or a stored value is
    /// malformed.
    fn events(&self, tick: u32) -> Result<TransitionEvents>;
}

// Each method is the reader's own, which a path through the type names ahead
// of the trait's.
impl Recording for McfrReader {
    fn producer(&self) -> Producer {
        McfrReader::producer(self)
    }

    fn layout_yaml(&self) -> &str {
        McfrReader::layout_yaml(self)
    }

    fn hashes(&self) -> &Hashes {
        McfrReader::hashes(self)
    }

    fn terminal_tick(&self) -> u32 {
        McfrReader::terminal_tick(self)
    }

    fn state(&self, tick: u32) -> Result<WorldSnapshot> {
        McfrReader::state(self, tick)
    }

    fn events(&self, tick: u32) -> Result<TransitionEvents> {
        McfrReader::events(self, tick)
    }
}

/// A timeline an [`crate::McfrWriter`] kept in memory instead of writing it:
/// what the MCFR would hold, as the writer had it before storing it.
#[derive(Debug)]
pub struct MemoryRecording {
    pub(crate) producer: Producer,
    pub(crate) layout_yaml: String,
    pub(crate) hashes: Hashes,
    /// Tick `t`'s state and events at index `t - 1`.
    pub(crate) ticks: Vec<(WorldSnapshot, TransitionEvents)>,
}

impl MemoryRecording {
    fn tick(&self, tick: u32) -> Result<&(WorldSnapshot, TransitionEvents)> {
        usize::try_from(tick)
            .ok()
            .and_then(|tick| tick.checked_sub(1))
            .and_then(|index| self.ticks.get(index))
            .ok_or_else(|| {
                Error::invalid(format!("tick {tick} is outside 1..={}", self.ticks.len()))
            })
    }
}

impl Recording for MemoryRecording {
    fn producer(&self) -> Producer {
        self.producer
    }

    fn layout_yaml(&self) -> &str {
        &self.layout_yaml
    }

    fn hashes(&self) -> &Hashes {
        &self.hashes
    }

    fn terminal_tick(&self) -> u32 {
        u32::try_from(self.ticks.len()).expect("the writer counts ticks in u32")
    }

    fn state(&self, tick: u32) -> Result<WorldSnapshot> {
        Ok(self.tick(tick)?.0.clone())
    }

    fn events(&self, tick: u32) -> Result<TransitionEvents> {
        Ok(self.tick(tick)?.1.clone())
    }
}
