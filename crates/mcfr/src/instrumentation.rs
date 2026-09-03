use std::{
    fs,
    path::{Path, PathBuf},
};

use rust_hdf5::{H5Dataset, H5File, H5Group};
use serde::Serialize;
use tempfile::TempPath;

use crate::{Error, INSTRUMENTATION_CONTAINER_VERSION, INSTRUMENTATION_FORMAT, Result, canonical};

const BYTE_CHUNK: usize = 256 * 1024;
const RECORD_CHUNK: usize = 4096;

#[derive(Debug, Clone, Copy)]
pub struct InstrumentationRecord<'a> {
    pub step: u64,
    pub channel: &'a str,
    pub content_type: &'a str,
    pub payload: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentationEntry {
    pub step: u64,
    pub channel: String,
    pub content_type: String,
    pub payload: Vec<u8>,
}

pub trait InstrumentationSink {
    /// Records one temporary research observation.
    ///
    /// # Errors
    ///
    /// Returns an implementation-defined error if the record is rejected or cannot be persisted.
    fn record(&mut self, record: InstrumentationRecord<'_>) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct NoInstrumentation;

impl InstrumentationSink for NoInstrumentation {
    fn record(&mut self, _record: InstrumentationRecord<'_>) -> Result<()> {
        Ok(())
    }
}

pub struct InstrumentationWriter {
    target: PathBuf,
    temporary: TempPath,
    file: Option<H5File>,
    records: H5Group,
    steps: H5Dataset,
    payload_data: H5Dataset,
    payload_offsets: H5Dataset,
    payload_end: u64,
    record_count: u64,
    poisoned: bool,
}

impl InstrumentationWriter {
    /// Creates an instrumentation sidecar bound to a formal MCFR physics result hash.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid metadata, an existing target, or HDF5/I/O failure.
    pub fn create(
        path: impl AsRef<Path>,
        physics_result_hash: &str,
        profile: &str,
        producer: &str,
    ) -> Result<Self> {
        canonical::parse_hex(physics_result_hash, "physics_result_hash")?;
        require_name(profile, "instrumentation profile")?;
        require_name(producer, "instrumentation producer")?;
        let target = path.as_ref().to_path_buf();
        if target.exists() {
            return Err(Error::invalid(format!(
                "refusing to overwrite existing instrumentation sidecar {}",
                target.display()
            )));
        }
        let parent = target.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;
        let temporary = tempfile::Builder::new()
            .prefix(".mcfr-i-")
            .suffix(".h5.part")
            .tempfile_in(parent)?
            .into_temp_path();
        let file = H5File::create(&temporary)?;
        file.set_attr_string("format", INSTRUMENTATION_FORMAT)?;
        file.set_attr_string(
            "container_version",
            &INSTRUMENTATION_CONTAINER_VERSION.to_string(),
        )?;
        file.set_attr_string("physics_result_hash", physics_result_hash)?;
        file.set_attr_string("profile", profile)?;
        file.set_attr_string("producer", producer)?;
        let records = file.create_group("records")?;
        let steps = records
            .new_dataset::<u64>()
            .shape([0])
            .chunk(&[RECORD_CHUNK])
            .max_shape(&[None])
            .create("steps")?;
        records.create_appendable_vlen_dataset("channels", RECORD_CHUNK, None)?;
        records.create_appendable_vlen_dataset("content_types", RECORD_CHUNK, None)?;
        let payload_data = records
            .new_dataset::<u8>()
            .shape([0])
            .chunk(&[BYTE_CHUNK])
            .max_shape(&[None])
            .deflate(6)
            .create("payload_data")?;
        let payload_offsets = records
            .new_dataset::<u64>()
            .shape([0])
            .chunk(&[RECORD_CHUNK])
            .max_shape(&[None])
            .create("payload_offsets")?;
        payload_offsets.append(&[0_u64])?;
        Ok(Self {
            target,
            temporary,
            file: Some(file),
            records,
            steps,
            payload_data,
            payload_offsets,
            payload_end: 0,
            record_count: 0,
            poisoned: false,
        })
    }

    /// Canonically serializes and records a JSON observation.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization fails or the underlying sink cannot append the record.
    pub fn record_json<T: Serialize>(&mut self, step: u64, channel: &str, value: &T) -> Result<()> {
        let payload = canonical::encode(value)?;
        self.record(InstrumentationRecord {
            step,
            channel,
            content_type: "application/json",
            payload: &payload,
        })
    }

    /// Atomically publishes the completed instrumentation sidecar.
    ///
    /// # Errors
    ///
    /// Returns an error after a partial append failure, if final HDF5 writes fail, or if the target
    /// appears before publication.
    pub fn finish(mut self) -> Result<()> {
        if self.poisoned {
            return Err(Error::invalid(
                "cannot finish instrumentation after a partial write failure",
            ));
        }
        let file = self
            .file
            .as_ref()
            .ok_or_else(|| Error::invalid("instrumentation file is unavailable"))?;
        file.set_attr_string("record_count", &self.record_count.to_string())?;
        let file = self
            .file
            .take()
            .ok_or_else(|| Error::invalid("instrumentation file is unavailable"))?;
        file.close()?;
        self.temporary
            .persist_noclobber(&self.target)
            .map_err(|error| Error::Io(error.error))?;
        Ok(())
    }
}

impl InstrumentationSink for InstrumentationWriter {
    fn record(&mut self, record: InstrumentationRecord<'_>) -> Result<()> {
        require_name(record.channel, "instrumentation channel")?;
        require_name(record.content_type, "instrumentation content type")?;
        self.poisoned = true;
        self.steps.append(&[record.step])?;
        self.records
            .append_vlen_strings("channels", &[record.channel])?;
        self.records
            .append_vlen_strings("content_types", &[record.content_type])?;
        self.payload_data.append(record.payload)?;
        self.payload_end = self
            .payload_end
            .checked_add(
                u64::try_from(record.payload.len())
                    .map_err(|_| Error::invalid("instrumentation payload is too large"))?,
            )
            .ok_or_else(|| Error::invalid("instrumentation payload offset overflow"))?;
        self.payload_offsets.append(&[self.payload_end])?;
        self.record_count = self
            .record_count
            .checked_add(1)
            .ok_or_else(|| Error::invalid("instrumentation record count overflow"))?;
        self.poisoned = false;
        Ok(())
    }
}

pub struct InstrumentationReader {
    _file: H5File,
    physics_result_hash: String,
    profile: String,
    producer: String,
    steps: Vec<u64>,
    channels: Vec<String>,
    content_types: Vec<String>,
    payload_offsets: Vec<u64>,
    payload_data: H5Dataset,
}

impl InstrumentationReader {
    /// Opens and validates an instrumentation sidecar.
    ///
    /// # Errors
    ///
    /// Returns an error for I/O or HDF5 failures, invalid metadata, or inconsistent track lengths.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = H5File::open(path)?;
        expect_attr(&file, "format", INSTRUMENTATION_FORMAT)?;
        expect_attr(
            &file,
            "container_version",
            &INSTRUMENTATION_CONTAINER_VERSION.to_string(),
        )?;
        let physics_result_hash = file.attr_string("physics_result_hash")?;
        canonical::parse_hex(&physics_result_hash, "physics_result_hash")?;
        let profile = file.attr_string("profile")?;
        let producer = file.attr_string("producer")?;
        require_name(&profile, "instrumentation profile")?;
        require_name(&producer, "instrumentation producer")?;
        let count: usize = file
            .attr_string("record_count")?
            .parse()
            .map_err(|_| Error::invalid("instrumentation record_count is invalid"))?;
        let steps = file.dataset("records/steps")?.read_raw::<u64>()?;
        let channels = file.dataset("records/channels")?.read_vlen_strings()?;
        let content_types = file.dataset("records/content_types")?.read_vlen_strings()?;
        let payload_offsets = file.dataset("records/payload_offsets")?.read_raw::<u64>()?;
        let payload_data = file.dataset("records/payload_data")?;
        if steps.len() != count || channels.len() != count || content_types.len() != count {
            return Err(Error::invalid(
                "instrumentation record datasets have inconsistent lengths",
            ));
        }
        if payload_offsets.len() != count + 1
            || payload_offsets.first() != Some(&0)
            || payload_offsets.windows(2).any(|pair| pair[0] > pair[1])
            || payload_offsets.last().copied()
                != Some(
                    u64::try_from(payload_data.total_elements()).map_err(|_| {
                        Error::invalid("instrumentation payload dataset is too large")
                    })?,
                )
        {
            return Err(Error::invalid(
                "instrumentation payload offsets are invalid",
            ));
        }
        Ok(Self {
            _file: file,
            physics_result_hash,
            profile,
            producer,
            steps,
            channels,
            content_types,
            payload_offsets,
            payload_data,
        })
    }

    #[must_use]
    pub fn physics_result_hash(&self) -> &str {
        &self.physics_result_hash
    }

    #[must_use]
    pub fn profile(&self) -> &str {
        &self.profile
    }

    #[must_use]
    pub fn producer(&self) -> &str {
        &self.producer
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Reads one instrumentation entry by record index.
    ///
    /// # Errors
    ///
    /// Returns an error when the index is out of range or the payload slice cannot be read.
    pub fn entry(&self, index: usize) -> Result<InstrumentationEntry> {
        let (&start, &end) = self
            .payload_offsets
            .get(index)
            .zip(self.payload_offsets.get(index + 1))
            .ok_or_else(|| {
                Error::invalid(format!("instrumentation index {index} is out of range"))
            })?;
        let length = usize::try_from(end - start)
            .map_err(|_| Error::invalid("instrumentation payload length overflow"))?;
        let start = usize::try_from(start)
            .map_err(|_| Error::invalid("instrumentation payload offset overflow"))?;
        Ok(InstrumentationEntry {
            step: self.steps[index],
            channel: self.channels[index].clone(),
            content_type: self.content_types[index].clone(),
            payload: self.payload_data.read_slice::<u8>(&[start], &[length])?,
        })
    }
}

fn require_name(value: &str, label: &str) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') {
        return Err(Error::invalid(format!("{label} is empty or contains NUL")));
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
