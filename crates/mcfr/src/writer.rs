use std::{
    fs,
    path::{Path, PathBuf},
};

use tempfile::TempPath;

use crate::{
    DurableContext, Error, Hashes, MCFR_FORMAT, McfrReader, Result, TransitionEvents,
    WorldSnapshot,
    canonical::{self, CanonicalHasher},
    model::IdentityAllocator,
    parquet_storage::{self, StorageWriter},
};

pub struct McfrWriter {
    target: PathBuf,
    temporary: TempPath,
    storage: Option<StorageWriter>,
    context_bytes: Vec<u8>,
    initial_state_bytes: Option<Vec<u8>>,
    tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
    poisoned: bool,
}

impl McfrWriter {
    /// Starts an empty MCFR container. The first appended tick must be tick zero
    /// and therefore carry an empty event batch.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid context, an existing target, or a Parquet initialization
    /// failure.
    pub fn create(path: impl AsRef<Path>, context: &DurableContext) -> Result<Self> {
        let target = path.as_ref().to_path_buf();
        if target.exists() {
            return Err(Error::invalid(format!(
                "refusing to overwrite existing MCFR {}",
                target.display()
            )));
        }
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        context.validate()?;
        let context_bytes = canonical::encode(context)?;
        let temporary = tempfile::Builder::new()
            .prefix(".mcfr-")
            .suffix(".zip.part")
            .tempfile_in(parent)?
            .into_temp_path();
        let storage = StorageWriter::create(parent)?;
        Ok(Self {
            target,
            temporary,
            storage: Some(storage),
            context_bytes,
            initial_state_bytes: None,
            tick_hashes: Vec::new(),
            poisoned: false,
        })
    }

    /// Appends one end-of-logical-tick slice and returns its canonical hash.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid tick-zero snapshot, canonical encoding failure, or partial
    /// Parquet append failure.
    pub fn append_tick(
        &mut self,
        mut state: WorldSnapshot,
        events: &TransitionEvents,
    ) -> Result<String> {
        let tick = u64::try_from(self.tick_hashes.len())
            .map_err(|_| Error::invalid("tick count overflow"))?;
        if tick == 0 && !events.events.is_empty() {
            return Err(Error::invalid("tick zero must have an empty event batch"));
        }
        state.canonicalize();
        state.object_keys()?;
        if tick == 0 {
            IdentityAllocator::from_initial(&state)?;
        }
        let state_bytes = canonical::encode(&state)?;
        let event_bytes = canonical::encode(events)?;
        let hash = canonical::tick_hash(tick, &state_bytes, &event_bytes);
        self.poisoned = true;
        self.storage
            .as_mut()
            .ok_or_else(|| Error::invalid("writer storage is unavailable"))?
            .append_tick(tick, &state, events, hash)?;
        if tick == 0 {
            self.initial_state_bytes = Some(state_bytes);
        }
        self.tick_hashes.push(hash);
        self.poisoned = false;
        Ok(canonical::hex(&hash))
    }

    /// Finalizes the timeline hash and atomically publishes the container.
    ///
    /// # Errors
    ///
    /// Returns an error when no tick was written, after a partial append failure, or if metadata
    /// finalization and atomic publication fail.
    pub fn finish(mut self) -> Result<Hashes> {
        if self.poisoned {
            return Err(Error::invalid(
                "cannot finish an MCFR after a partial write failure",
            ));
        }
        if self.tick_hashes.is_empty() {
            return Err(Error::invalid("an MCFR must contain tick zero"));
        }
        let mut scenario_hasher = CanonicalHasher::new("scenario-0.0.1");
        scenario_hasher.update(MCFR_FORMAT.as_bytes());
        scenario_hasher.update(&self.context_bytes);
        scenario_hasher.update(
            self.initial_state_bytes
                .as_deref()
                .ok_or_else(|| Error::invalid("an MCFR must contain tick zero"))?,
        );
        let scenario = scenario_hasher.finalize();
        let result = canonical::result_hash(&scenario, &self.tick_hashes);
        let hashes = Hashes::from_raw(scenario, result);
        let directory = self
            .storage
            .take()
            .ok_or_else(|| Error::invalid("writer storage is unavailable"))?
            .finish(&self.context_bytes, &hashes)?;
        parquet_storage::package_members(directory.path(), &self.temporary)?;
        let verified = McfrReader::open(&self.temporary)?;
        if verified.hashes() != &hashes {
            return Err(Error::invalid(
                "published MCFR hashes differ after structural verification",
            ));
        }
        drop(verified);
        self.temporary
            .persist_noclobber(&self.target)
            .map_err(|error| Error::Io(error.error))?;
        Ok(hashes)
    }
}
