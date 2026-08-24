use std::{
    fs,
    path::{Path, PathBuf},
};

use rust_hdf5::H5File;
use tempfile::TempPath;

use crate::{
    DurableContext, Error, Hashes, MCFR_CONTAINER_VERSION, MCFR_FORMAT, Result, TransitionEvents,
    WorldSnapshot,
    canonical::{self, CanonicalHasher},
    model::IdentityAllocator,
    storage,
};

pub struct McfrWriter {
    target: PathBuf,
    temporary: TempPath,
    file: Option<H5File>,
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
    /// Returns an error for invalid context, an existing target, or an HDF5 initialization
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
            .suffix(".h5.part")
            .tempfile_in(parent)?
            .into_temp_path();
        let file = H5File::create(&temporary)?;
        file.set_attr_string("format", MCFR_FORMAT)?;
        file.set_attr_string("container_version", &MCFR_CONTAINER_VERSION.to_string())?;
        file.set_attr_string("schema_version", &context.schema_version.to_string())?;
        file.create_group("context")?
            .new_dataset::<u8>()
            .shape([context_bytes.len()])
            .create("data")?
            .write_raw(&context_bytes)?;
        storage::create(&file)?;
        Ok(Self {
            target,
            temporary,
            file: Some(file),
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
    /// HDF5 append failure.
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
        storage::append_tick(
            self.file
                .as_ref()
                .ok_or_else(|| Error::invalid("writer file is unavailable"))?,
            &state,
            events,
            &hash,
        )?;
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
        let mut scenario_hasher = CanonicalHasher::new("scenario-v2");
        scenario_hasher.update(&self.context_bytes);
        scenario_hasher.update(
            self.initial_state_bytes
                .as_deref()
                .ok_or_else(|| Error::invalid("an MCFR must contain tick zero"))?,
        );
        let scenario = scenario_hasher.finalize();
        let result = canonical::result_hash(&scenario, &self.tick_hashes);
        let hashes = Hashes::from_raw(scenario, result);
        let tick_count = u64::try_from(self.tick_hashes.len())
            .map_err(|_| Error::invalid("tick count overflow"))?;
        let file = self
            .file
            .as_ref()
            .ok_or_else(|| Error::invalid("writer file is unavailable"))?;
        file.set_attr_string("tick_count", &tick_count.to_string())?;
        file.set_attr_string("terminal_tick", &(tick_count - 1).to_string())?;
        file.set_attr_string("scenario_hash", &hashes.scenario_hash)?;
        file.set_attr_string("result_hash", &hashes.result_hash)?;
        self.file
            .take()
            .ok_or_else(|| Error::invalid("writer file is unavailable"))?
            .close()?;
        self.temporary
            .persist_noclobber(&self.target)
            .map_err(|error| Error::Io(error.error))?;
        Ok(hashes)
    }
}
