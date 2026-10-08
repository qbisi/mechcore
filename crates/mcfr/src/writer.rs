use std::{
    fs,
    path::{Path, PathBuf},
};

use tempfile::TempPath;

use crate::{
    DurableContext, Error, Hashes, InstrumentRow, McfrReader, MemoryRecording, Producer, Result,
    TickHashes, TransitionEvents, WorldSnapshot, canonical,
    model::IdentityAllocator,
    parquet_storage::{self, StorageWriter},
};

pub struct McfrWriter {
    target: Option<PathBuf>,
    temporary: Option<TempPath>,
    storage: Option<StorageWriter>,
    producer: Option<Producer>,
    game_build: Option<String>,
    layout_yaml: Option<String>,
    context_bytes: Vec<u8>,
    identity_initialized: bool,
    /// Every tick appended, for a writer that keeps the timeline in memory.
    memory: Option<Vec<(WorldSnapshot, TransitionEvents)>>,
    tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
    /// The canonical bytes of the tick last hashed, kept to reuse.
    state_bytes: Vec<u8>,
    event_bytes: Vec<u8>,
    poisoned: bool,
}

impl McfrWriter {
    /// Starts an empty MCFR container. The first appended state is `S(1)`.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid context, an existing target, or a Parquet initialization
    /// failure.
    pub fn create(
        path: impl AsRef<Path>,
        producer: Producer,
        game_build: &str,
        context: &DurableContext,
        layout_yaml: &str,
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
        let layout_yaml = embedded_layout(game_build, context, layout_yaml)?;
        let context_bytes = parquet_storage::encode_durable_context(context)?;
        let temporary = tempfile::Builder::new()
            .prefix(".mcfr-")
            .suffix(".zip.part")
            .tempfile_in(parent)?
            .into_temp_path();
        let storage = StorageWriter::create(parent)?;
        Ok(Self {
            target: Some(target),
            temporary: Some(temporary),
            storage: Some(storage),
            producer: Some(producer),
            game_build: Some(game_build.to_owned()),
            layout_yaml: Some(layout_yaml),
            context_bytes,
            identity_initialized: false,
            memory: None,
            tick_hashes: Vec::new(),
            state_bytes: Vec::new(),
            event_bytes: Vec::new(),
            poisoned: false,
        })
    }

    /// Starts a canonical hash-only timeline without creating MCFR storage.
    ///
    /// # Errors
    ///
    /// Returns an error when the durable context is invalid or cannot be encoded.
    pub fn hash_only(context: &DurableContext) -> Result<Self> {
        context.validate()?;
        Ok(Self {
            target: None,
            temporary: None,
            storage: None,
            producer: None,
            game_build: None,
            layout_yaml: None,
            context_bytes: parquet_storage::encode_durable_context(context)?,
            identity_initialized: false,
            memory: None,
            tick_hashes: Vec::new(),
            state_bytes: Vec::new(),
            event_bytes: Vec::new(),
            poisoned: false,
        })
    }

    /// Starts a timeline kept in memory: what [`Self::create`] would write,
    /// checked the same way, handed back by [`Self::finish_in_memory`] for a
    /// reader that has no use for the file.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid context or layout.
    pub fn in_memory(
        producer: Producer,
        game_build: &str,
        context: &DurableContext,
        layout_yaml: &str,
    ) -> Result<Self> {
        let layout_yaml = embedded_layout(game_build, context, layout_yaml)?;
        Ok(Self {
            target: None,
            temporary: None,
            storage: None,
            producer: Some(producer),
            game_build: Some(game_build.to_owned()),
            layout_yaml: Some(layout_yaml),
            context_bytes: parquet_storage::encode_durable_context(context)?,
            identity_initialized: false,
            memory: Some(Vec::new()),
            tick_hashes: Vec::new(),
            state_bytes: Vec::new(),
            event_bytes: Vec::new(),
            poisoned: false,
        })
    }

    /// Appends one logical tick, where the first state and event batch are `S(1)` and `E(1)`.
    ///
    /// # Errors
    ///
    /// Returns an error if the state or events are invalid, the tick count overflows, or the
    /// backing storage cannot append the tick.
    pub fn append_tick(
        &mut self,
        mut state: WorldSnapshot,
        events: &TransitionEvents,
    ) -> Result<TickHashes> {
        let tick = u32::try_from(self.tick_hashes.len())
            .map_err(|_| Error::invalid("tick count overflow"))?
            .checked_add(1)
            .ok_or_else(|| Error::invalid("tick count overflow"))?;
        state.canonicalize();
        state.object_keys()?;
        if !self.identity_initialized {
            IdentityAllocator::from_initial(&state)?;
            self.identity_initialized = true;
        }
        canonical::encode_into(&mut self.state_bytes, &state)?;
        canonical::encode_into(&mut self.event_bytes, events)?;
        let tick_hash = canonical::tick_hash(tick, &self.state_bytes, &self.event_bytes);
        self.poisoned = true;
        if let Some(storage) = &mut self.storage {
            storage.append_tick(tick, &state, events, tick_hash)?;
        }
        if let Some(memory) = &mut self.memory {
            memory.push((state, events.clone()));
        }
        self.tick_hashes.push(tick_hash);
        self.poisoned = false;
        Ok(TickHashes {
            tick_hash: canonical::hex(&tick_hash),
        })
    }

    /// Adds rows of an instrument channel to the tick last appended.
    ///
    /// A channel is published once it has been appended to, rows or not, and
    /// the hash does not read it. A hash-only writer keeps nothing.
    ///
    /// # Errors
    ///
    /// Returns an error before the first tick, for a channel whose rows cannot be
    /// stored, or if the backing storage cannot append them.
    pub fn append_instrument<R: InstrumentRow>(&mut self, rows: &[R]) -> Result<()> {
        let tick = u32::try_from(self.tick_hashes.len())
            .map_err(|_| Error::invalid("tick count overflow"))?;
        if tick == 0 {
            return Err(Error::invalid(
                "an instrument row belongs to a tick, and none has been appended",
            ));
        }
        let Some(storage) = &mut self.storage else {
            return Ok(());
        };
        self.poisoned = true;
        storage.append_instrument(tick, rows)?;
        self.poisoned = false;
        Ok(())
    }

    /// Finalizes the timeline hash and hands back the timeline a writer made
    /// by [`Self::in_memory`] kept.
    ///
    /// # Errors
    ///
    /// Returns an error for a writer that keeps no timeline, when no tick was
    /// written, and after a partial append failure.
    pub fn finish_in_memory(mut self) -> Result<MemoryRecording> {
        let ticks = self
            .memory
            .take()
            .ok_or_else(|| Error::invalid("this writer does not keep its timeline in memory"))?;
        let producer = self
            .producer
            .ok_or_else(|| Error::invalid("writer producer is unavailable"))?;
        let layout_yaml = self
            .layout_yaml
            .take()
            .ok_or_else(|| Error::invalid("writer layout is unavailable"))?;
        let hashes = self.finish()?;
        Ok(MemoryRecording {
            producer,
            layout_yaml,
            hashes,
            ticks,
        })
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
            return Err(Error::invalid("an MCFR must contain at least tick 1"));
        }
        let hashes = Hashes::from_raw(canonical::result_hash(&self.tick_hashes));
        let Some(storage) = self.storage.take() else {
            return Ok(hashes);
        };
        let producer = self
            .producer
            .ok_or_else(|| Error::invalid("writer producer is unavailable"))?;
        let game_build = self
            .game_build
            .as_deref()
            .ok_or_else(|| Error::invalid("writer game build is unavailable"))?;
        let layout_yaml = self
            .layout_yaml
            .as_deref()
            .ok_or_else(|| Error::invalid("writer layout is unavailable"))?;
        let temporary = self
            .temporary
            .as_ref()
            .ok_or_else(|| Error::invalid("writer temporary output is unavailable"))?;
        let directory = storage.finish(
            producer,
            game_build,
            &self.context_bytes,
            layout_yaml,
            &hashes,
        )?;
        parquet_storage::package_members(directory.path(), temporary)?;
        let verified = McfrReader::open(temporary)?;
        if verified.hashes() != &hashes {
            return Err(Error::invalid(
                "published MCFR hashes differ after structural verification",
            ));
        }
        drop(verified);
        self.temporary
            .take()
            .ok_or_else(|| Error::invalid("writer temporary output is unavailable"))?
            .persist_noclobber(
                self.target
                    .as_ref()
                    .ok_or_else(|| Error::invalid("writer target is unavailable"))?,
            )
            .map_err(|error| Error::Io(error.error))?;
        Ok(hashes)
    }
}

/// The layout a recording embeds, in canonical form, once it is checked
/// against the build and the durable context: its seed is the match seed and
/// its round the combat round.
fn embedded_layout(
    game_build: &str,
    context: &DurableContext,
    layout_yaml: &str,
) -> Result<String> {
    if game_build.trim().is_empty() {
        return Err(Error::invalid("game_build must not be empty"));
    }
    context.validate()?;
    let layout =
        mechcore_document::parse_embedded_yaml(layout_yaml.as_bytes()).map_err(Error::invalid)?;
    match layout.seed {
        None => {
            return Err(Error::invalid(
                "layout has no seed; a recording embeds the resolved match seed",
            ));
        }
        Some(seed) if seed != context.match_seed => {
            return Err(Error::invalid(format!(
                "layout seed {seed} differs from durable context match_seed {}",
                context.match_seed
            )));
        }
        Some(_) => {}
    }
    if u32::try_from(layout.round).ok() != Some(context.combat_round) {
        return Err(Error::invalid(format!(
            "layout round {} differs from durable context combat_round {}",
            layout.round, context.combat_round
        )));
    }
    mechcore_document::canonical_embedded_yaml(layout).map_err(Error::invalid)
}
