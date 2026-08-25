use std::path::Path;

use rust_hdf5::H5File;

use crate::{
    DurableContext, Error, Hashes, MCFR_CONTAINER_VERSION, MCFR_FORMAT, Result, TickSlice,
    TransitionEvents, WorldSnapshot, canonical, storage::StorageReader,
};

pub struct McfrReader {
    file: H5File,
    context: DurableContext,
    tick_count: u64,
    terminal_tick: u64,
    hashes: Hashes,
    storage: StorageReader,
}

impl McfrReader {
    /// Opens an MCFR and validates its HDF5 structure and metadata.
    ///
    /// # Errors
    ///
    /// Returns an error for I/O failures, unsupported versions, malformed metadata, or invalid
    /// column and offset shapes.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = H5File::open(path)?;
        expect_attr(&file, "format", MCFR_FORMAT)?;
        let container_version = parse_u32_attr(&file, "container_version")?;
        if container_version != MCFR_CONTAINER_VERSION {
            return Err(Error::invalid(format!(
                "unsupported MCFR container version {container_version}"
            )));
        }
        let schema_version = parse_u32_attr(&file, "schema_version")?;
        let tick_count = parse_u64_attr(&file, "tick_count")?;
        let terminal_tick = parse_u64_attr(&file, "terminal_tick")?;
        if tick_count == 0 || terminal_tick != tick_count - 1 {
            return Err(Error::invalid(
                "MCFR must contain tick zero and terminal_tick must be the final tick",
            ));
        }
        let hashes = Hashes {
            scenario_hash: file.attr_string("scenario_hash")?,
            result_hash: file.attr_string("result_hash")?,
        };
        hashes.validate_encoding()?;
        let context_data = file.dataset("context/data")?.read_raw::<u8>()?;
        let context: DurableContext = canonical::decode(&context_data, "durable context")?;
        context.validate()?;
        if context.schema_version != schema_version {
            return Err(Error::invalid(
                "schema_version attribute differs from durable context",
            ));
        }
        let storage = StorageReader::open(&file, tick_count)?;
        let reader = Self {
            file,
            context,
            tick_count,
            terminal_tick,
            hashes,
            storage,
        };
        if !reader.events(0)?.events.is_empty() {
            return Err(Error::invalid("tick zero must have an empty event batch"));
        }
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
        let state = self.storage.state(&self.file, tick)?;
        let mut normalized = state.clone();
        normalized.canonicalize();
        if normalized != state {
            return Err(Error::invalid(format!(
                "state {tick} object collections are not in canonical order"
            )));
        }
        Ok(state)
    }

    /// Reads the native event batch associated with one tick. Events at tick
    /// `t > 0` occurred while advancing from `S(t-1)` to `S(t)`.
    ///
    /// # Errors
    ///
    /// Returns an error when `tick` is out of range or a stored value is malformed.
    pub fn events(&self, tick: u64) -> Result<TransitionEvents> {
        self.storage.events(&self.file, tick)
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

fn expect_attr(file: &H5File, name: &str, expected: &str) -> Result<()> {
    let actual = file.attr_string(name)?;
    if actual != expected {
        return Err(Error::invalid(format!(
            "attribute {name} is {actual:?}, expected {expected:?}"
        )));
    }
    Ok(())
}

fn parse_u32_attr(file: &H5File, name: &str) -> Result<u32> {
    file.attr_string(name)?
        .parse()
        .map_err(|_| Error::invalid(format!("attribute {name} is not a u32")))
}

fn parse_u64_attr(file: &H5File, name: &str) -> Result<u64> {
    file.attr_string(name)?
        .parse()
        .map_err(|_| Error::invalid(format!("attribute {name} is not a u64")))
}
