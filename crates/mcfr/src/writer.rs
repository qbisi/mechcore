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
    game_build: String,
    context_bytes: Vec<u8>,
    initial_state_bytes: Option<Vec<u8>>,
    tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
    poisoned: bool,
}

impl McfrWriter {
    /// Starts an empty MCFR container. Call [`Self::set_initial_state`] once before
    /// appending any transition tick.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid context, an existing target, or a Parquet initialization
    /// failure.
    pub fn create(
        path: impl AsRef<Path>,
        game_build: &str,
        context: &DurableContext,
    ) -> Result<Self> {
        let target = path.as_ref().to_path_buf();
        if target.exists() {
            return Err(Error::invalid(format!(
                "refusing to overwrite existing MCFR {}",
                target.display()
            )));
        }
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        if game_build.trim().is_empty() {
            return Err(Error::invalid("game_build must not be empty"));
        }
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
            game_build: game_build.to_owned(),
            context_bytes,
            initial_state_bytes: None,
            tick_hashes: Vec::new(),
            poisoned: false,
        })
    }

    /// Sets the authoritative pre-advance state `S(0)`.
    ///
    /// # Errors
    ///
    /// Returns an error if an initial state was already written or the state is invalid.
    pub fn set_initial_state(&mut self, mut state: WorldSnapshot) -> Result<()> {
        if self.initial_state_bytes.is_some() || !self.tick_hashes.is_empty() {
            return Err(Error::invalid("S(0) has already been written"));
        }
        state.canonicalize();
        state.object_keys()?;
        IdentityAllocator::from_initial(&state)?;
        let state_bytes = canonical::encode(&state)?;
        self.poisoned = true;
        self.storage
            .as_mut()
            .ok_or_else(|| Error::invalid("writer storage is unavailable"))?
            .append_initial_state(&state);
        self.initial_state_bytes = Some(state_bytes);
        self.poisoned = false;
        Ok(())
    }

    /// Appends one transition `T(t)`, where the first appended transition is tick 1.
    ///
    /// # Errors
    ///
    /// Returns an error if `S(0)` is missing, the state or events are invalid, the tick count
    /// overflows, or the backing storage cannot append the transition.
    pub fn append_tick(
        &mut self,
        mut state: WorldSnapshot,
        events: &TransitionEvents,
    ) -> Result<String> {
        if self.initial_state_bytes.is_none() {
            return Err(Error::invalid("S(0) must be written before T(1)"));
        }
        let tick = u32::try_from(self.tick_hashes.len())
            .map_err(|_| Error::invalid("tick count overflow"))?
            .checked_add(1)
            .ok_or_else(|| Error::invalid("tick count overflow"))?;
        state.canonicalize();
        state.object_keys()?;
        let state_bytes = canonical::encode(&state)?;
        let event_bytes = canonical::encode(events)?;
        let hash = canonical::tick_hash(tick, &state_bytes, &event_bytes);
        self.poisoned = true;
        self.storage
            .as_mut()
            .ok_or_else(|| Error::invalid("writer storage is unavailable"))?
            .append_tick(tick, &state, events, hash)?;
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
        if self.initial_state_bytes.is_none() {
            return Err(Error::invalid("an MCFR must contain S(0)"));
        }
        if self.tick_hashes.is_empty() {
            return Err(Error::invalid("an MCFR must contain at least T(1)"));
        }
        let mut scenario_hasher = CanonicalHasher::new("scenario-0.1.0");
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
            .finish(&self.game_build, &self.context_bytes, &hashes)?;
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
