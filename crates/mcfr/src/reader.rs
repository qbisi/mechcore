use std::path::Path;

use crate::{
    DurableContext, Error, Hashes, MCFR_FORMAT, Result, TickSlice, TransitionEvents, WorldSnapshot,
    canonical::{self, CanonicalHasher},
    model::IdentityAllocator,
    parquet_storage::StorageReader,
};

pub struct McfrReader {
    context: DurableContext,
    tick_count: u64,
    terminal_tick: u64,
    hashes: Hashes,
    storage: StorageReader,
}

impl McfrReader {
    /// Opens an MCFR and validates its ZIP64/Parquet structure and metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for I/O failures, unsupported formats, malformed metadata, invalid
    /// Parquet tracks, or canonical hash mismatches.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let storage = StorageReader::open(path.as_ref())?;
        let context = storage.metadata().context.clone();
        let tick_count = storage.metadata().tick_count;
        let terminal_tick = storage.metadata().terminal_tick;
        let hashes = storage.metadata().hashes.clone();
        let reader = Self {
            context,
            tick_count,
            terminal_tick,
            hashes,
            storage,
        };
        if !reader.events(0)?.events.is_empty() {
            return Err(Error::invalid("tick zero must have an empty event batch"));
        }
        IdentityAllocator::from_initial(&reader.state(0)?)?;
        reader.validate_hashes()?;
        Ok(reader)
    }

    #[must_use]
    pub const fn context(&self) -> &DurableContext {
        &self.context
    }

    #[must_use]
    pub const fn hashes(&self) -> &Hashes {
        &self.hashes
    }

    #[must_use]
    pub const fn tick_count(&self) -> u64 {
        self.tick_count
    }

    #[must_use]
    pub const fn terminal_tick(&self) -> u64 {
        self.terminal_tick
    }

    /// Returns one tick hash as canonical lowercase hexadecimal.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range.
    pub fn tick_hash(&self, tick: u64) -> Result<String> {
        Ok(canonical::hex(&self.storage.tick_hash(tick)?))
    }

    /// Reads one authoritative snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range or a stored value is malformed.
    pub fn state(&self, tick: u64) -> Result<WorldSnapshot> {
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

    fn validate_hashes(&self) -> Result<()> {
        let context = canonical::encode(&self.context)?;
        let initial = canonical::encode(&self.state(0)?)?;
        let mut scenario = CanonicalHasher::new("scenario-0.0.1");
        scenario.update(MCFR_FORMAT.as_bytes());
        scenario.update(&context);
        scenario.update(&initial);
        let scenario = scenario.finalize();
        let stored_scenario = canonical::parse_hex(&self.hashes.scenario_hash, "scenario_hash")?;
        if scenario != stored_scenario {
            return Err(Error::invalid(
                "scenario_hash does not match decoded durable context and S(0)",
            ));
        }

        let mut tick_hashes = Vec::with_capacity(
            usize::try_from(self.tick_count)
                .map_err(|_| Error::invalid("tick count is too large"))?,
        );
        for tick in 0..self.tick_count {
            let state = canonical::encode(&self.state(tick)?)?;
            let events = canonical::encode(&self.events(tick)?)?;
            let actual = canonical::tick_hash(tick, &state, &events);
            if actual != self.storage.tick_hash(tick)? {
                return Err(Error::invalid(format!(
                    "tick_hash({tick}) does not match decoded S({tick}) and E({tick})"
                )));
            }
            tick_hashes.push(actual);
        }
        let result = canonical::result_hash(&scenario, &tick_hashes);
        let stored_result = canonical::parse_hex(&self.hashes.result_hash, "result_hash")?;
        if result != stored_result {
            return Err(Error::invalid(
                "result_hash does not match the decoded tick hash sequence",
            ));
        }
        Ok(())
    }

    /// Reads the native event batch associated with one tick. Events at tick
    /// `t > 0` occurred while advancing from `S(t-1)` to `S(t)`.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range or a stored value is malformed.
    pub fn events(&self, tick: u64) -> Result<TransitionEvents> {
        self.storage.events(tick)
    }

    /// Reads the state, events, and hash for one logical tick.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range or its data is malformed.
    pub fn tick(&self, tick: u64) -> Result<TickSlice> {
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
    /// Returns an error when the recordings have different durable contexts or an index cannot
    /// be represented.
    pub fn first_divergence(&self, other: &Self) -> Result<Option<u64>> {
        if self.hashes.scenario_hash != other.hashes.scenario_hash {
            return Err(Error::invalid(
                "cannot compare first divergence for different durable contexts",
            ));
        }
        for (index, (left, right)) in self
            .storage
            .tick_hashes()
            .iter()
            .zip(other.storage.tick_hashes())
            .enumerate()
        {
            if left != right {
                return Ok(Some(
                    u64::try_from(index).map_err(|_| Error::invalid("tick index overflow"))?,
                ));
            }
        }
        if self.tick_count == other.tick_count {
            Ok(None)
        } else {
            Ok(Some(self.tick_count.min(other.tick_count)))
        }
    }
}
