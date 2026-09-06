use std::{
    fs,
    path::{Path, PathBuf},
};

use tempfile::TempPath;

use crate::{
    DurableContext, Error, Hashes, McfrReader, Result, TickHashes, TransitionEvents, WorldSnapshot,
    canonical,
    model::IdentityAllocator,
    parquet_storage::{self, StorageWriter},
};

pub struct McfrWriter {
    target: Option<PathBuf>,
    temporary: Option<TempPath>,
    storage: Option<StorageWriter>,
    game_build: Option<String>,
    layout_yaml: Option<String>,
    context: DurableContext,
    context_bytes: Vec<u8>,
    identity_initialized: bool,
    physics_tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
    content_tick_hashes: Vec<[u8; canonical::HASH_BYTES]>,
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
        if game_build.trim().is_empty() {
            return Err(Error::invalid("game_build must not be empty"));
        }
        context.validate()?;
        let layout =
            mechcore_layout::parse_embedded_yaml(layout_yaml.as_bytes()).map_err(Error::invalid)?;
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
        let layout_yaml =
            mechcore_layout::canonical_embedded_yaml(layout).map_err(Error::invalid)?;
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
            game_build: Some(game_build.to_owned()),
            layout_yaml: Some(layout_yaml),
            context: context.clone(),
            context_bytes,
            identity_initialized: false,
            physics_tick_hashes: Vec::new(),
            content_tick_hashes: Vec::new(),
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
            game_build: None,
            layout_yaml: None,
            context: context.clone(),
            context_bytes: parquet_storage::encode_durable_context(context)?,
            identity_initialized: false,
            physics_tick_hashes: Vec::new(),
            content_tick_hashes: Vec::new(),
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
        let tick = u32::try_from(self.physics_tick_hashes.len())
            .map_err(|_| Error::invalid("tick count overflow"))?
            .checked_add(1)
            .ok_or_else(|| Error::invalid("tick count overflow"))?;
        state.canonicalize();
        state.object_keys()?;
        if !self.identity_initialized {
            IdentityAllocator::from_initial(&state)?;
            self.identity_initialized = true;
        }
        let state_bytes = canonical::encode(&state)?;
        let event_bytes = canonical::encode(events)?;
        let physics_hash = canonical::physics_tick_hash(&self.context, tick, &state, events);
        let content_hash = canonical::content_tick_hash(tick, &state_bytes, &event_bytes);
        self.poisoned = true;
        if let Some(storage) = &mut self.storage {
            storage.append_tick(tick, &state, events, physics_hash, content_hash)?;
        }
        self.physics_tick_hashes.push(physics_hash);
        self.content_tick_hashes.push(content_hash);
        self.poisoned = false;
        Ok(TickHashes {
            physics_tick_hash: canonical::hex(&physics_hash),
            content_tick_hash: canonical::hex(&content_hash),
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
        if self.physics_tick_hashes.is_empty() {
            return Err(Error::invalid("an MCFR must contain at least tick 1"));
        }
        debug_assert_eq!(
            self.physics_tick_hashes.len(),
            self.content_tick_hashes.len()
        );
        let hashes = Hashes::from_raw(
            canonical::physics_result_hash(&self.physics_tick_hashes),
            canonical::content_result_hash(&self.content_tick_hashes),
        );
        let Some(storage) = self.storage.take() else {
            return Ok(hashes);
        };
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
        let directory = storage.finish(game_build, &self.context_bytes, layout_yaml, &hashes)?;
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
