use std::{collections::BTreeSet, path::Path};

use rust_hdf5::{H5Dataset, H5File};

use crate::{
    DurableContext, Error, Hashes, MCFR_CONTAINER_VERSION, MCFR_FORMAT, Result, TransitionEvents,
    WorldSnapshot,
    canonical::{self, CanonicalHasher},
    model::validate_transition,
};

pub struct McfrReader {
    _file: H5File,
    context: DurableContext,
    header: Header,
    state_offsets: Vec<u64>,
    event_offsets: Vec<u64>,
    state_data: H5Dataset,
    event_data: H5Dataset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Header {
    state_count: u64,
    transition_count: u64,
    terminal_step: u64,
    hashes: Hashes,
}

impl McfrReader {
    /// Opens an MCFR and validates its container structure and canonical encodings.
    ///
    /// # Errors
    ///
    /// Returns an error for I/O or HDF5 failures, unsupported versions, malformed metadata, or
    /// invalid track layout.
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
        let state_count = parse_u64_attr(&file, "state_count")?;
        let transition_count = parse_u64_attr(&file, "transition_count")?;
        let terminal_step = parse_u64_attr(&file, "terminal_step")?;
        if state_count == 0 || state_count != transition_count + 1 {
            return Err(Error::invalid(
                "MCFR must contain exactly one more state than transitions",
            ));
        }
        if terminal_step != transition_count {
            return Err(Error::invalid(
                "terminal_step must identify the final state boundary",
            ));
        }
        let hashes = Hashes {
            scenario_hash: file.attr_string("scenario_hash")?,
            state_hash: file.attr_string("state_hash")?,
            event_hash: file.attr_string("event_hash")?,
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
        let state_data = file.dataset("states/data")?;
        let event_data = file.dataset("events/data")?;
        let state_offsets = file.dataset("states/offsets")?.read_raw::<u64>()?;
        let event_offsets = file.dataset("events/offsets")?.read_raw::<u64>()?;
        validate_offsets(
            &state_offsets,
            state_count + 1,
            state_data.total_elements(),
            "state",
        )?;
        validate_offsets(
            &event_offsets,
            transition_count + 1,
            event_data.total_elements(),
            "event",
        )?;
        Ok(Self {
            _file: file,
            context,
            header: Header {
                state_count,
                transition_count,
                terminal_step,
                hashes,
            },
            state_offsets,
            event_offsets,
            state_data,
            event_data,
        })
    }

    /// Opens an MCFR and verifies the complete S/E transition chain and all formal hashes.
    ///
    /// # Errors
    ///
    /// Returns any error from [`Self::open`] or [`Self::verify`].
    pub fn open_verified(path: impl AsRef<Path>) -> Result<Self> {
        let reader = Self::open(path)?;
        reader.verify()?;
        Ok(reader)
    }

    #[must_use]
    pub const fn context(&self) -> &DurableContext {
        &self.context
    }

    #[must_use]
    pub const fn hashes(&self) -> &Hashes {
        &self.header.hashes
    }

    #[must_use]
    pub const fn state_count(&self) -> u64 {
        self.header.state_count
    }

    #[must_use]
    pub const fn transition_count(&self) -> u64 {
        self.header.transition_count
    }

    #[must_use]
    pub const fn terminal_step(&self) -> u64 {
        self.header.terminal_step
    }

    /// Reads a canonical state snapshot by step boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when the index is out of range or the stored record is malformed.
    pub fn state(&self, step: u64) -> Result<WorldSnapshot> {
        let bytes = read_record(&self.state_data, &self.state_offsets, step, "state")?;
        let state: WorldSnapshot = canonical::decode(&bytes, "state")?;
        let mut normalized = state.clone();
        normalized.canonicalize();
        if normalized != state {
            return Err(Error::invalid(format!(
                "state {step} object collections are not in canonical order"
            )));
        }
        Ok(state)
    }

    /// Reads one canonical event batch by transition index.
    ///
    /// # Errors
    ///
    /// Returns an error when the index is out of range or the stored record is malformed.
    pub fn events(&self, transition: u64) -> Result<TransitionEvents> {
        let bytes = read_record(
            &self.event_data,
            &self.event_offsets,
            transition,
            "event transition",
        )?;
        canonical::decode(&bytes, "event transition")
    }

    /// Recomputes transition validity and the scenario, state, event, and result hashes.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed or inconsistent records and [`Error::HashMismatch`] when a
    /// stored formal hash differs from the recomputed value.
    pub fn verify(&self) -> Result<Hashes> {
        let context_bytes = canonical::encode(&self.context)?;
        let initial = self.state(0)?;
        initial.validate(&BTreeSet::new())?;
        let initial_bytes = canonical::encode(&initial)?;
        let mut scenario_hasher = CanonicalHasher::new("scenario-v1");
        scenario_hasher.update(&context_bytes);
        scenario_hasher.update(&initial_bytes);
        let mut state_hasher = CanonicalHasher::new("state-v1");
        state_hasher.update(&initial_bytes);
        let mut event_hasher = CanonicalHasher::new("event-v1");
        let mut current = initial;
        let mut known = current.object_keys()?;
        for transition in 0..self.header.transition_count {
            let events = self.events(transition)?;
            let next = self.state(transition + 1)?;
            known = validate_transition(&current, &events, &next, &known)?;
            event_hasher.update(&canonical::encode(&events)?);
            state_hasher.update(&canonical::encode(&next)?);
            current = next;
        }
        let scenario = scenario_hasher.finalize();
        let state = state_hasher.finalize();
        let event = event_hasher.finalize();
        let result = canonical::result_hash(&scenario, &state, &event);
        let actual = Hashes::from_raw(scenario, state, event, result);
        compare_hash(
            "scenario_hash",
            &self.header.hashes.scenario_hash,
            &actual.scenario_hash,
        )?;
        compare_hash(
            "state_hash",
            &self.header.hashes.state_hash,
            &actual.state_hash,
        )?;
        compare_hash(
            "event_hash",
            &self.header.hashes.event_hash,
            &actual.event_hash,
        )?;
        compare_hash(
            "result_hash",
            &self.header.hashes.result_hash,
            &actual.result_hash,
        )?;
        Ok(actual)
    }
}

fn read_record(dataset: &H5Dataset, offsets: &[u64], index: u64, label: &str) -> Result<Vec<u8>> {
    let index =
        usize::try_from(index).map_err(|_| Error::invalid(format!("{label} index overflow")))?;
    let end_index = index
        .checked_add(1)
        .ok_or_else(|| Error::invalid(format!("{label} index overflow")))?;
    let (&start, &end) = offsets
        .get(index)
        .zip(offsets.get(end_index))
        .ok_or_else(|| Error::invalid(format!("{label} index {index} is out of range")))?;
    let length = usize::try_from(end - start)
        .map_err(|_| Error::invalid(format!("{label} length overflow")))?;
    let start =
        usize::try_from(start).map_err(|_| Error::invalid(format!("{label} offset overflow")))?;
    Ok(dataset.read_slice::<u8>(&[start], &[length])?)
}

fn validate_offsets(offsets: &[u64], expected: u64, data_len: usize, label: &str) -> Result<()> {
    if offsets.len() != usize::try_from(expected).unwrap_or(usize::MAX)
        || offsets.first() != Some(&0)
    {
        return Err(Error::invalid(format!(
            "{label} offsets have an invalid shape"
        )));
    }
    if offsets.windows(2).any(|pair| pair[0] > pair[1]) {
        return Err(Error::invalid(format!("{label} offsets are not monotonic")));
    }
    let data_len = u64::try_from(data_len)
        .map_err(|_| Error::invalid(format!("{label} data is too large")))?;
    if offsets.last() != Some(&data_len) {
        return Err(Error::invalid(format!(
            "{label} offsets do not cover the dataset"
        )));
    }
    Ok(())
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

fn compare_hash(name: &'static str, expected: &str, actual: &str) -> Result<()> {
    if expected != actual {
        return Err(Error::HashMismatch {
            name,
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        });
    }
    Ok(())
}
