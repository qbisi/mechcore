use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct TeamResult {
    pub team: &'static str,
    pub unit: String,
    pub alive: bool,
    pub remaining_life: i64,
    pub max_life: i64,
}

#[derive(Debug, Serialize)]
pub struct SimulationResult {
    pub schema: &'static str,
    pub game_build: String,
    pub seed: i32,
    pub seed_source: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    pub end_reason: &'static str,
    pub steps: u64,
    pub simulated_duration_milliseconds: u64,
    pub winner: Option<&'static str>,
    pub draw: bool,
    pub teams: Vec<TeamResult>,
    pub hashes: Hashes,
    pub profiling: SimulationProfile,
    /// The timeline, when it was asked to be kept in memory.
    #[serde(skip)]
    pub recording: Option<MemoryRecording>,
}

/// What a fight cost to compute, for following the simulator's speed from one
/// change to the next.
#[derive(Debug, Clone, Serialize)]
pub struct SimulationProfile {
    pub generation_duration_milliseconds: f64,
    pub simulation_to_real_time_rate: f64,
    /// Where the generation went: the four phases add up to it.
    pub phases_milliseconds: Phases,
    /// Live units summed over every tick: the fight's size, which the step's
    /// cost grows with.
    pub unit_ticks: u64,
    /// The most units alive on one tick.
    pub peak_live_units: u64,
    /// The step phase over the ticks fought.
    pub step_milliseconds_per_tick: f64,
    /// The step phase over the unit-ticks fought.
    pub step_microseconds_per_unit_tick: f64,
    /// The tick whose step took longest.
    pub slowest_step: SlowestStep,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_sizes_bytes: Option<BTreeMap<String, u64>>,
}

/// Where a fight's generation went, in milliseconds.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Phases {
    /// Building the scene and its first targets, before the first tick.
    pub prepare: f64,
    /// Advancing the fight a tick: the rules themselves.
    pub step: f64,
    /// Reading each tick's state out of the fight and putting it in order.
    pub snapshot: f64,
    /// Hashing each tick and, for a kept recording, storing it.
    pub record: f64,
    /// Closing the recording, and reopening a written one to check it.
    pub finish: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct SlowestStep {
    pub tick: u32,
    pub milliseconds: f64,
    pub live_units: u64,
}

/// What [`execute`] measures as it fights.
#[derive(Default)]
pub(in crate::fight) struct Costs {
    prepare: Duration,
    step: Duration,
    snapshot: Duration,
    record: Duration,
    unit_ticks: u64,
    peak_live_units: u64,
    slowest_step: (Duration, u32, u64),
}

impl Costs {
    /// Counts one tick's step and the units alive after it.
    fn stepped(&mut self, tick: u32, took: Duration, live_units: usize) {
        let live_units = u64::try_from(live_units).unwrap_or(u64::MAX);
        self.step += took;
        self.unit_ticks += live_units;
        self.peak_live_units = self.peak_live_units.max(live_units);
        if took > self.slowest_step.0 {
            self.slowest_step = (took, tick, live_units);
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SimulationComparison {
    pub schema: &'static str,
    pub game_build: String,
    pub seed: i32,
    pub equal: bool,
    pub recording: TimelineSummary,
    pub simulation: TimelineSummary,
    pub first_divergence: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divergent_tick: Option<DivergentTick>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_hash: Option<String>,
    pub tick_count: u32,
    pub complete: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DivergentTick {
    pub recording: Option<TickSlice>,
    pub simulation: Option<TickSlice>,
}

pub(in crate::fight) struct Execution {
    pub(in crate::fight) simulation: Simulation,
    pub(in crate::fight) writer: McfrWriter,
    pub(in crate::fight) steps: u64,
    pub(in crate::fight) end_reason: &'static str,
    pub(in crate::fight) first_divergence: Option<u32>,
    pub(in crate::fight) divergent_tick: Option<DivergentTick>,
    pub(in crate::fight) costs: Costs,
}

pub(crate) fn run(
    layout: &CompiledLayout,
    config: &SimulationConfig,
    seed: i32,
    seed_source: &'static str,
    record: Record<'_>,
    replay_layout: &str,
) -> Result<SimulationResult> {
    let generation_started = Instant::now();
    let execution = execute(layout, config, seed, record, None, Some(replay_layout))?;
    let Execution {
        simulation,
        writer,
        steps,
        end_reason,
        first_divergence,
        costs,
        ..
    } = execution;
    debug_assert!(first_divergence.is_none());
    let finish_started = Instant::now();
    let (hashes, recording) = match record {
        Record::Memory => {
            let recording = writer.finish_in_memory()?;
            (recording.hashes().clone(), Some(recording))
        }
        Record::Hash | Record::File(_) => (writer.finish()?, None),
    };
    let output = match record {
        Record::File(path) => Some(path),
        Record::Hash | Record::Memory => None,
    };
    let (file_size_bytes, member_sizes_bytes) = if let Some(path) = output {
        let published = mechcore_mcfr::McfrReader::open(path)?;
        if published.hashes() != &hashes {
            return Err(Error::new("published MCFR hashes changed after reopening"));
        }
        (
            Some(published.file_size_bytes()),
            Some(published.member_sizes_bytes().clone()),
        )
    } else {
        (None, None)
    };
    let finish = finish_started.elapsed();
    let generation_duration = generation_started.elapsed();
    let simulated_duration_milliseconds = steps
        .saturating_mul(LOGIC_TICK_TIME_UNITS)
        .saturating_mul(1_000)
        / TIME_UNITS_PER_SECOND;
    #[allow(
        clippy::cast_precision_loss,
        reason = "a millisecond count stays far below f64's exact integer range"
    )]
    let simulation_to_real_time_rate =
        simulated_duration_milliseconds as f64 / generation_duration.as_secs_f64() / 1_000.0;
    let winner = simulation.winner().map(team_name);
    Ok(SimulationResult {
        schema: "mechcore.simulation-result.v5",
        game_build: config.game_build.clone(),
        seed,
        seed_source,
        output: output.map(|path| path.display().to_string()),
        end_reason,
        steps,
        simulated_duration_milliseconds,
        winner,
        draw: winner.is_none(),
        teams: simulation
            .actors
            .values()
            .map(|actor| TeamResult {
                team: team_name(actor.placement.team),
                unit: actor.placement.type_name.clone(),
                alive: actor.alive(),
                remaining_life: actor.life,
                max_life: actor.stats.max_life(),
            })
            .collect(),
        hashes,
        profiling: SimulationProfile {
            generation_duration_milliseconds: milliseconds(generation_duration),
            simulation_to_real_time_rate,
            phases_milliseconds: Phases {
                prepare: milliseconds(costs.prepare),
                step: milliseconds(costs.step),
                snapshot: milliseconds(costs.snapshot),
                record: milliseconds(costs.record),
                finish: milliseconds(finish),
            },
            unit_ticks: costs.unit_ticks,
            peak_live_units: costs.peak_live_units,
            step_milliseconds_per_tick: per(milliseconds(costs.step), steps),
            step_microseconds_per_unit_tick: per(
                milliseconds(costs.step) * 1_000.0,
                costs.unit_ticks,
            ),
            slowest_step: SlowestStep {
                tick: costs.slowest_step.1,
                milliseconds: milliseconds(costs.slowest_step.0),
                live_units: costs.slowest_step.2,
            },
            file_size_bytes,
            member_sizes_bytes,
        },
        recording,
    })
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

/// A total over a count, zero over none.
#[allow(
    clippy::cast_precision_loss,
    reason = "a tick or unit-tick count stays far below f64's exact integer range"
)]
fn per(total: f64, count: u64) -> f64 {
    if count == 0 {
        0.0
    } else {
        total / count as f64
    }
}

pub(crate) fn compare(
    layout: &CompiledLayout,
    config: &SimulationConfig,
    seed: i32,
    recording: &McfrReader,
) -> Result<SimulationComparison> {
    if recording.game_build() != config.game_build {
        return Err(Error::new(format!(
            "game_build mismatch: recording={}, simulation={}",
            recording.game_build(),
            config.game_build
        )));
    }
    let execution = execute(layout, config, seed, Record::Hash, Some(recording), None)?;
    let Execution {
        writer,
        steps,
        first_divergence,
        divergent_tick,
        ..
    } = execution;
    let simulation_hashes = if first_divergence.is_none() {
        Some(writer.finish()?)
    } else {
        None
    };
    if let Some(hashes) = &simulation_hashes
        && hashes.result_hash != recording.hashes().result_hash
    {
        return Err(Error::new(
            "result hashes differ although every compared tick hash matches",
        ));
    }
    Ok(SimulationComparison {
        schema: "mechcore.sim-compare-result.v3",
        game_build: config.game_build.clone(),
        seed,
        equal: first_divergence.is_none(),
        recording: TimelineSummary {
            result_hash: Some(recording.hashes().result_hash.clone()),
            tick_count: recording.tick_count(),
            complete: true,
        },
        simulation: TimelineSummary {
            result_hash: simulation_hashes.map(|hashes| hashes.result_hash),
            tick_count: u32::try_from(steps)
                .map_err(|_| Error::new("simulation tick count exceeds u32"))?,
            complete: first_divergence.is_none(),
        },
        first_divergence,
        divergent_tick,
    })
}

pub(in crate::fight) fn execute(
    layout: &CompiledLayout,
    config: &SimulationConfig,
    seed: i32,
    record: Record<'_>,
    recording: Option<&McfrReader>,
    replay_layout: Option<&str>,
) -> Result<Execution> {
    let divisor = gcd(LOGIC_TICK_TIME_UNITS, TIME_UNITS_PER_SECOND);
    let context = DurableContext {
        logic_step: Rational {
            numerator: u32::try_from(LOGIC_TICK_TIME_UNITS / divisor)
                .map_err(|_| Error::new("logic-step numerator exceeds u32"))?,
            denominator: u32::try_from(TIME_UNITS_PER_SECOND / divisor)
                .map_err(|_| Error::new("logic-step denominator exceeds u32"))?,
        },
        time_units_per_second: u32::try_from(TIME_UNITS_PER_SECOND)
            .map_err(|_| Error::new("time units per second exceeds u32"))?,
        combat_round: layout.round,
        match_seed: seed,
    };
    let mut costs = Costs::default();
    let prepare_started = Instant::now();
    let mut simulation =
        Simulation::new_unprepared(layout, &config.units, &config.towers, &config.maps, seed)?;
    let mut writer = writer(record, replay_layout, seed, config, &context)?;
    simulation.initialize_presearch_targets()?;
    costs.prepare = prepare_started.elapsed();
    let mut steps = 0;
    let mut first_divergence = None;
    let mut divergent_tick = None;
    let max_steps = FIGHT_TIME_SECONDS
        .saturating_mul(TIME_UNITS_PER_SECOND)
        .div_ceil(LOGIC_TICK_TIME_UNITS);
    let mut end_reason = loop {
        if steps >= max_steps {
            break "forced_time_limit";
        }
        let step_started = Instant::now();
        let events = simulation.step(steps)?;
        steps += 1;
        let tick = u32::try_from(steps).map_err(|_| Error::new("tick index exceeds u32"))?;
        simulation.close_tick(steps >= max_steps)?;
        let stepped = step_started.elapsed();
        let snapshot_started = Instant::now();
        let mut state = simulation.snapshot();
        state.canonicalize();
        costs.snapshot += snapshot_started.elapsed();
        costs.stepped(tick, stepped, state.live_units.len());
        let record_started = Instant::now();
        let tick_hashes = writer.append_tick(state.clone(), &events)?;
        costs.record += record_started.elapsed();
        if let Some(recording) = recording {
            let expected_hash = if tick <= recording.tick_count() {
                Some(recording.tick_hash(tick)?)
            } else {
                None
            };
            if expected_hash.as_deref() != Some(&tick_hashes.tick_hash) {
                first_divergence = Some(tick);
                divergent_tick = Some(DivergentTick {
                    recording: if expected_hash.is_some() {
                        Some(recording.tick(tick)?)
                    } else {
                        None
                    },
                    simulation: Some(TickSlice {
                        tick,
                        state,
                        events,
                        tick_hash: tick_hashes.tick_hash,
                    }),
                });
                break "first_divergence";
            }
        }
        if simulation.ready_to_finish() {
            break "natural_module_drain";
        }
    };
    if first_divergence.is_none()
        && let Some(recording) = recording
        && steps < u64::from(recording.tick_count())
    {
        let tick = u32::try_from(steps)
            .map_err(|_| Error::new("tick index exceeds u32"))?
            .checked_add(1)
            .ok_or_else(|| Error::new("tick index overflow"))?;
        first_divergence = Some(tick);
        divergent_tick = Some(DivergentTick {
            recording: Some(recording.tick(tick)?),
            simulation: None,
        });
        end_reason = "first_divergence";
    }
    Ok(Execution {
        simulation,
        writer,
        steps,
        end_reason,
        first_divergence,
        divergent_tick,
        costs,
    })
}

/// The writer a fight records into: a recording at a path or in memory,
/// which embeds the replay layout under the seed the fight ran with, or only
/// the hash.
fn writer(
    record: Record<'_>,
    replay_layout: Option<&str>,
    seed: i32,
    config: &SimulationConfig,
    context: &DurableContext,
) -> Result<McfrWriter> {
    let path = match record {
        Record::Hash => return McfrWriter::hash_only(context).map_err(Into::into),
        Record::File(path) => Some(path),
        Record::Memory => None,
    };
    let mut replay_layout = mechcore_document::parse_yaml(
        replay_layout
            .ok_or_else(|| Error::new("a recorded fight requires a replay layout"))?
            .as_bytes(),
    )
    .map_err(Error::new)?;
    replay_layout.seed = Some(seed);
    let replay_layout = mechcore_document::canonical_yaml(replay_layout).map_err(Error::new)?;
    match path {
        Some(path) => McfrWriter::create(
            path,
            Producer::Simulator,
            &config.game_build,
            context,
            &replay_layout,
        ),
        None => McfrWriter::in_memory(
            Producer::Simulator,
            &config.game_build,
            context,
            &replay_layout,
        ),
    }
    .map_err(Into::into)
}
