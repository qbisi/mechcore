use std::{
    fs,
    path::{Path, PathBuf},
};

use rust_hdf5::{H5Dataset, H5File};
use tempfile::TempPath;

use crate::{
    DurableContext, Error, Hashes, MCFR_CONTAINER_VERSION, MCFR_FORMAT, Result, TransitionEvents,
    WorldSnapshot,
    canonical::{self, CanonicalHasher},
    model::IdentityAllocator,
};

const BYTE_CHUNK: usize = 256 * 1024;
const OFFSET_CHUNK: usize = 4096;

pub struct McfrWriter {
    target: PathBuf,
    temporary: TempPath,
    file: Option<H5File>,
    state_data: H5Dataset,
    state_offsets: H5Dataset,
    event_data: H5Dataset,
    event_offsets: H5Dataset,
    context_bytes: Vec<u8>,
    initial_state_bytes: Vec<u8>,
    state_hasher: CanonicalHasher,
    event_hasher: CanonicalHasher,
    state_end: u64,
    event_end: u64,
    transition_count: u64,
    poisoned: bool,
}

impl McfrWriter {
    /// Starts an MCFR at `path` with durable context and the initial snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the input is invalid, the target already exists, or the temporary
    /// HDF5 container cannot be created and initialized.
    pub fn create(
        path: impl AsRef<Path>,
        context: &DurableContext,
        mut initial_state: WorldSnapshot,
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
        context.validate()?;
        initial_state.canonicalize();
        IdentityAllocator::from_initial(&initial_state)?;
        let context_bytes = canonical::encode(&context)?;
        let initial_bytes = canonical::encode(&initial_state)?;

        let temporary = tempfile::Builder::new()
            .prefix(".mcfr-")
            .suffix(".h5.part")
            .tempfile_in(parent)?
            .into_temp_path();
        let file = H5File::create(&temporary)?;
        file.set_attr_string("format", MCFR_FORMAT)?;
        file.set_attr_string("container_version", &MCFR_CONTAINER_VERSION.to_string())?;
        file.set_attr_string("schema_version", &context.schema_version.to_string())?;
        let context_group = file.create_group("context")?;
        let states_group = file.create_group("states")?;
        let events_group = file.create_group("events")?;
        context_group
            .new_dataset::<u8>()
            .shape([context_bytes.len()])
            .create("data")?
            .write_raw(&context_bytes)?;
        let state_data = byte_dataset(&states_group, "data")?;
        let state_offsets = offset_dataset(&states_group, "offsets")?;
        let event_data = byte_dataset(&events_group, "data")?;
        let event_offsets = offset_dataset(&events_group, "offsets")?;
        state_offsets.append(&[0_u64])?;
        event_offsets.append(&[0_u64])?;
        state_data.append(&initial_bytes)?;
        let state_end = u64::try_from(initial_bytes.len())
            .map_err(|_| Error::invalid("initial state is too large"))?;
        state_offsets.append(&[state_end])?;

        let mut state_hasher = CanonicalHasher::new("state-v1");
        state_hasher.update(&initial_bytes);

        Ok(Self {
            target,
            temporary,
            file: Some(file),
            state_data,
            state_offsets,
            event_data,
            event_offsets,
            context_bytes,
            initial_state_bytes: initial_bytes,
            state_hasher,
            event_hasher: CanonicalHasher::new("event-v1"),
            state_end,
            event_end: 0,
            transition_count: 0,
            poisoned: false,
        })
    }

    /// Appends one `E(t), S(t+1)` transition.
    ///
    /// # Errors
    ///
    /// Returns an error if canonical encoding or an HDF5 append fails.
    pub fn push_transition(
        &mut self,
        events: &TransitionEvents,
        mut next_state: WorldSnapshot,
    ) -> Result<()> {
        next_state.canonicalize();
        let event_bytes = canonical::encode(&events)?;
        let state_bytes = canonical::encode(&next_state)?;
        self.poisoned = true;
        self.event_data.append(&event_bytes)?;
        self.event_end = checked_end(self.event_end, event_bytes.len(), "event track")?;
        self.event_offsets.append(&[self.event_end])?;
        self.state_data.append(&state_bytes)?;
        self.state_end = checked_end(self.state_end, state_bytes.len(), "state track")?;
        self.state_offsets.append(&[self.state_end])?;
        self.event_hasher.update(&event_bytes);
        self.state_hasher.update(&state_bytes);
        self.transition_count = self
            .transition_count
            .checked_add(1)
            .ok_or_else(|| Error::invalid("transition count overflow"))?;
        self.poisoned = false;
        Ok(())
    }

    /// Finalizes hashes and atomically publishes the completed container.
    ///
    /// # Errors
    ///
    /// Returns an error after a partial append failure, if final HDF5 writes fail, or if the target
    /// path appears before publication.
    pub fn finish(mut self) -> Result<Hashes> {
        if self.poisoned {
            return Err(Error::invalid(
                "cannot finish an MCFR after a partial write failure",
            ));
        }
        let mut scenario_hasher = CanonicalHasher::new("scenario-v1");
        scenario_hasher.update(&self.context_bytes);
        scenario_hasher.update(&self.initial_state_bytes);
        let scenario = scenario_hasher.finalize();
        let state = std::mem::replace(
            &mut self.state_hasher,
            CanonicalHasher::new("consumed-state-hasher"),
        )
        .finalize();
        let event = std::mem::replace(
            &mut self.event_hasher,
            CanonicalHasher::new("consumed-event-hasher"),
        )
        .finalize();
        let result = canonical::result_hash(&scenario, &state, &event);
        let hashes = Hashes::from_raw(scenario, state, event, result);
        let file = self
            .file
            .as_ref()
            .ok_or_else(|| Error::invalid("writer file is unavailable"))?;
        file.set_attr_string("state_count", &(self.transition_count + 1).to_string())?;
        file.set_attr_string("transition_count", &self.transition_count.to_string())?;
        file.set_attr_string("terminal_step", &self.transition_count.to_string())?;
        file.set_attr_string("scenario_hash", &hashes.scenario_hash)?;
        file.set_attr_string("state_hash", &hashes.state_hash)?;
        file.set_attr_string("event_hash", &hashes.event_hash)?;
        file.set_attr_string("result_hash", &hashes.result_hash)?;
        let file = self
            .file
            .take()
            .ok_or_else(|| Error::invalid("writer file is unavailable"))?;
        file.close()?;
        self.temporary
            .persist_noclobber(&self.target)
            .map_err(|error| Error::Io(error.error))?;
        Ok(hashes)
    }
}

fn byte_dataset(group: &rust_hdf5::H5Group, name: &str) -> Result<H5Dataset> {
    Ok(group
        .new_dataset::<u8>()
        .shape([0])
        .chunk(&[BYTE_CHUNK])
        .max_shape(&[None])
        .deflate(6)
        .create(name)?)
}

fn offset_dataset(group: &rust_hdf5::H5Group, name: &str) -> Result<H5Dataset> {
    Ok(group
        .new_dataset::<u64>()
        .shape([0])
        .chunk(&[OFFSET_CHUNK])
        .max_shape(&[None])
        .create(name)?)
}

fn checked_end(current: u64, added: usize, label: &str) -> Result<u64> {
    current
        .checked_add(
            u64::try_from(added).map_err(|_| Error::invalid(format!("{label} is too large")))?,
        )
        .ok_or_else(|| Error::invalid(format!("{label} offset overflow")))
}
