use std::{collections::BTreeMap, fs, path::Path};

use crate::{
    DurableContext, Error, Hashes, InstrumentRow, Producer, Result, TickSlice, TransitionEvents,
    WorldSnapshot, canonical, model::IdentityAllocator, parquet_storage::StorageReader,
};

pub struct McfrReader {
    file_size_bytes: u64,
    producer: Producer,
    game_build: String,
    context: DurableContext,
    tick_count: u32,
    terminal_tick: u32,
    hashes: Hashes,
    storage: StorageReader,
}

/// What [`McfrReader::published`] reads of a container.
#[derive(Debug, Clone)]
pub struct Published {
    pub hashes: Hashes,
    pub file_size_bytes: u64,
    pub member_sizes_bytes: BTreeMap<String, u64>,
}

impl McfrReader {
    /// Opens an MCFR and validates its ZIP64/Parquet structure and metadata.
    /// The result hash is checked against the stored tick hashes; the tick
    /// hashes are trusted, and timeline content is not rehashed until
    /// [`Self::tick`] reads it.
    ///
    /// # Errors
    ///
    /// Returns an error for I/O failures, unsupported formats, malformed metadata, or invalid
    /// Parquet tracks.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file_size_bytes = fs::metadata(path)?.len();
        let storage = StorageReader::open(path)?;
        let producer = storage.metadata().producer;
        let game_build = storage.metadata().game_build.clone();
        let context = storage.metadata().context.clone();
        let tick_count = storage.metadata().tick_count;
        let terminal_tick = storage.metadata().terminal_tick;
        let hashes = storage.metadata().hashes.clone();
        let reader = Self {
            file_size_bytes,
            producer,
            game_build,
            context,
            tick_count,
            terminal_tick,
            hashes,
            storage,
        };
        IdentityAllocator::from_initial(&reader.state(1)?)?;
        Ok(reader)
    }

    /// A published container's hashes and sizes, read from its
    /// `ticks.parquet` without its timeline: the result hash is held to the
    /// tick hash column, and neither is held to the states and events.
    ///
    /// # Errors
    ///
    /// Returns an error for I/O failures, an unsupported format, or a
    /// `ticks.parquet` whose metadata or tick hash column is malformed.
    pub fn published(path: impl AsRef<Path>) -> Result<Published> {
        let path = path.as_ref();
        let file_size_bytes = fs::metadata(path)?.len();
        let (hashes, member_sizes_bytes) = crate::parquet_storage::published(path)?;
        Ok(Published {
            hashes,
            file_size_bytes,
            member_sizes_bytes,
        })
    }

    /// What wrote the recording: the game or the simulator.
    #[must_use]
    pub const fn producer(&self) -> Producer {
        self.producer
    }

    #[must_use]
    pub fn game_build(&self) -> &str {
        &self.game_build
    }

    #[must_use]
    pub const fn context(&self) -> &DurableContext {
        &self.context
    }

    /// Returns the canonical replay layout embedded in the container.
    #[must_use]
    pub fn layout_yaml(&self) -> &str {
        self.storage.layout_yaml()
    }

    #[must_use]
    pub const fn hashes(&self) -> &Hashes {
        &self.hashes
    }

    #[must_use]
    pub const fn tick_count(&self) -> u32 {
        self.tick_count
    }

    #[must_use]
    pub const fn terminal_tick(&self) -> u32 {
        self.terminal_tick
    }

    /// The instrument channels the recording holds, in name order.
    pub fn instrument_channels(&self) -> impl Iterator<Item = &str> {
        self.storage.instrument_channels()
    }

    /// Every row of one instrument channel with the tick it was observed on,
    /// or `None` when the recording did not ask for the channel.
    ///
    /// # Errors
    ///
    /// Returns an error when the channel's member does not decode as its rows,
    /// or holds a row outside the recording's ticks.
    pub fn instrument<R: InstrumentRow>(&self) -> Result<Option<Vec<(u32, R)>>> {
        self.storage.instrument()
    }

    #[must_use]
    pub const fn file_size_bytes(&self) -> u64 {
        self.file_size_bytes
    }

    #[must_use]
    pub const fn member_sizes_bytes(&self) -> &BTreeMap<String, u64> {
        self.storage.member_sizes()
    }

    /// Returns one stored tick hash as canonical lowercase hexadecimal.
    ///
    /// The value is read from `ticks.parquet`, not recomputed; [`Self::tick`]
    /// is what checks it against the tick's state and events.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range.
    pub fn tick_hash(&self, tick: u32) -> Result<String> {
        Ok(canonical::hex(&self.storage.tick_hash(tick)?))
    }

    /// Reads one authoritative snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range or a stored value is malformed.
    pub fn state(&self, tick: u32) -> Result<WorldSnapshot> {
        let state = self.storage.state(tick)?;
        let mut normalized = state.clone();
        normalized.canonicalize();
        if normalized != state {
            return Err(Error::invalid(format!(
                "state {tick} object collections are not in canonical order"
            )));
        }
        state.object_keys()?;
        Ok(state)
    }

    /// Reads the native event batch associated with one tick. Events at tick
    /// `t > 1` occurred while advancing from `S(t-1)` to `S(t)`. `E(1)` is the
    /// first observed native update; `S(0)` is deliberately not persisted.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range or a stored value is malformed.
    pub fn events(&self, tick: u32) -> Result<TransitionEvents> {
        self.storage.events(tick)
    }

    /// Reads the state, events, and hash for one logical tick, and checks
    /// that the stored hash is the hash of that state and those events.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range, its data is malformed, or
    /// its stored hash does not match its content.
    pub fn tick(&self, tick: u32) -> Result<TickSlice> {
        let state = self.state(tick)?;
        let events = self.events(tick)?;
        let stored = self.storage.tick_hash(tick)?;
        let computed = canonical::tick_hash(
            tick,
            &canonical::encode(&state)?,
            &canonical::encode(&events)?,
        );
        if stored != computed {
            return Err(Error::invalid(format!(
                "tick {tick} stored tick_hash does not match its state and events"
            )));
        }
        Ok(TickSlice {
            tick,
            state,
            events,
            tick_hash: canonical::hex(&stored),
        })
    }

    /// Returns the first tick whose stored hashes differ. A prefix-only length difference
    /// diverges at the first missing tick.
    ///
    /// # Errors
    ///
    /// Returns an error when an index cannot be represented.
    pub fn first_divergence(&self, other: &Self) -> Result<Option<u32>> {
        for (index, (left, right)) in self
            .storage
            .tick_hashes()
            .iter()
            .zip(other.storage.tick_hashes())
            .enumerate()
        {
            if left != right {
                return Ok(Some(
                    u32::try_from(index + 1).map_err(|_| Error::invalid("tick index overflow"))?,
                ));
            }
        }
        if self.tick_count == other.tick_count {
            Ok(None)
        } else {
            Ok(Some(
                self.tick_count
                    .min(other.tick_count)
                    .checked_add(1)
                    .ok_or_else(|| Error::invalid("tick index overflow"))?,
            ))
        }
    }
}
