use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct TeamResult {
    pub team: &'static str,
    pub unit: String,
    pub alive: bool,
    pub remaining_life: i64,
    pub max_life: i64,
}

#[derive(Debug, Clone, Serialize)]
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
}

#[derive(Debug, Clone, Serialize)]
pub struct SimulationProfile {
    pub generation_duration_milliseconds: f64,
    pub simulation_to_real_time_rate: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub member_sizes_bytes: Option<BTreeMap<String, u64>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SimulationComparison {
    pub schema: &'static str,
    pub game_build: String,
    pub seed: i32,
    pub equal: bool,
    pub content_equal: bool,
    pub recording: TimelineSummary,
    pub simulation: TimelineSummary,
    pub first_divergence: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divergent_tick: Option<DivergentTick>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimelineSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physics_result_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_result_hash: Option<String>,
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
}

pub(crate) fn run(
    layout: &CompiledLayout,
    config: &SimulationConfig,
    seed: i32,
    seed_source: &'static str,
    output: Option<&Path>,
    replay_layout: &str,
) -> Result<SimulationResult> {
    let generation_started = Instant::now();
    let execution = execute(layout, config, seed, output, None, Some(replay_layout))?;
    let Execution {
        simulation,
        writer,
        steps,
        end_reason,
        first_divergence,
        ..
    } = execution;
    debug_assert!(first_divergence.is_none());
    let hashes = writer.finish()?;
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
        schema: "mechcore.simulation-result.v3",
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
            generation_duration_milliseconds: generation_duration.as_secs_f64() * 1_000.0,
            simulation_to_real_time_rate,
            file_size_bytes,
            member_sizes_bytes,
        },
    })
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
    let execution = execute(layout, config, seed, None, Some(recording), None)?;
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
        && hashes.physics_result_hash != recording.hashes().physics_result_hash
    {
        return Err(Error::new(
            "physics result hashes differ although every compared physics tick hash matches",
        ));
    }
    let content_equal = simulation_hashes
        .as_ref()
        .is_some_and(|hashes| hashes.content_result_hash == recording.hashes().content_result_hash);
    Ok(SimulationComparison {
        schema: "mechcore.sim-compare-result.v2",
        game_build: config.game_build.clone(),
        seed,
        equal: first_divergence.is_none(),
        content_equal,
        recording: TimelineSummary {
            physics_result_hash: Some(recording.hashes().physics_result_hash.clone()),
            content_result_hash: Some(recording.hashes().content_result_hash.clone()),
            tick_count: recording.tick_count(),
            complete: true,
        },
        simulation: TimelineSummary {
            physics_result_hash: simulation_hashes
                .as_ref()
                .map(|hashes| hashes.physics_result_hash.clone()),
            content_result_hash: simulation_hashes.map(|hashes| hashes.content_result_hash),
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
    output: Option<&Path>,
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
    let mut simulation = Simulation::new_unprepared(layout, &config.units, &config.towers, seed)?;
    let mut writer = match output {
        Some(path) => {
            let mut replay_layout = mechcore_document::parse_yaml(
                replay_layout
                    .ok_or_else(|| Error::new("output MCFR requires a replay layout"))?
                    .as_bytes(),
            )
            .map_err(Error::new)?;
            replay_layout.seed = Some(seed);
            let replay_layout =
                mechcore_document::canonical_yaml(replay_layout).map_err(Error::new)?;
            McfrWriter::create(path, &config.game_build, &context, &replay_layout)?
        }
        None => McfrWriter::hash_only(&context)?,
    };
    simulation.initialize_presearch_targets()?;
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
        let events = simulation.step(steps)?;
        steps += 1;
        let tick = u32::try_from(steps).map_err(|_| Error::new("tick index exceeds u32"))?;
        simulation.settle_intervals_if_finishing();
        let mut state = simulation.snapshot();
        state.canonicalize();
        let tick_hashes = writer.append_tick(state.clone(), &events)?;
        if let Some(recording) = recording {
            let expected_hash = if tick <= recording.tick_count() {
                Some(recording.physics_tick_hash(tick)?)
            } else {
                None
            };
            if expected_hash.as_deref() != Some(&tick_hashes.physics_tick_hash) {
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
                        physics_tick_hash: tick_hashes.physics_tick_hash,
                        content_tick_hash: tick_hashes.content_tick_hash,
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
    })
}
