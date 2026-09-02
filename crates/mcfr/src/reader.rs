use std::{collections::BTreeMap, fs, path::Path};

use crate::{
    DurableContext, Error, Hashes, Result, TickSlice, TransitionEvents, WorldSnapshot, canonical,
    model::IdentityAllocator, parquet_storage::StorageReader,
};

pub struct McfrReader {
    file_size_bytes: u64,
    game_build: String,
    context: DurableContext,
    tick_count: u32,
    terminal_tick: u32,
    hashes: Hashes,
    storage: StorageReader,
}

impl McfrReader {
    /// Opens an MCFR and validates its ZIP64/Parquet structure and metadata.
    /// Persisted hashes are trusted; timeline content is not rehashed.
    ///
    /// # Errors
    ///
    /// Returns an error for I/O failures, unsupported formats, malformed metadata, or invalid
    /// Parquet tracks.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let file_size_bytes = fs::metadata(path)?.len();
        let storage = StorageReader::open(path)?;
        let game_build = storage.metadata().game_build.clone();
        let context = storage.metadata().context.clone();
        let tick_count = storage.metadata().tick_count;
        let terminal_tick = storage.metadata().terminal_tick;
        let hashes = storage.metadata().hashes.clone();
        let reader = Self {
            file_size_bytes,
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

    #[must_use]
    pub const fn file_size_bytes(&self) -> u64 {
        self.file_size_bytes
    }

    #[must_use]
    pub const fn member_sizes_bytes(&self) -> &BTreeMap<String, u64> {
        self.storage.member_sizes()
    }

    /// Returns one tick hash as canonical lowercase hexadecimal.
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

    /// Reads the state, events, and hash for one logical tick.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range or its data is malformed.
    pub fn tick(&self, tick: u32) -> Result<TickSlice> {
        Ok(TickSlice {
            tick,
            state: self.state(tick)?,
            events: self.events(tick)?,
            tick_hash: self.tick_hash(tick)?,
        })
    }

    /// Returns the first unequal tick hash. A prefix-only length difference
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
