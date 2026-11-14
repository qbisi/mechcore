//! `play` and `--to html`: serve or export interactive battle playback pages.
//!
//! Both commands share the same round preparation and simulation path:
//! - `mechcore convert <in> --to html <out.html>` simulates the selected fight
//!   or all playable rounds upfront and writes one self-contained offline HTML
//!   file via [`mechcore_player::page`] or [`mechcore_player::match_page`].
//! - `mechcore play <in>` computes or loads round 0 first, binds a local HTTP
//!   server on loopback with round 0 embedded, computes remaining rounds
//!   sequentially in a background thread (prioritizing any round requested over
//!   HTTP), and persists simulated timelines in a two-tier memory + disk cache.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::{BufRead, BufReader, Write as _},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Condvar, Mutex},
    time::Duration,
};

use mechcore_mcfr::{McfrReader, Recording, UnitPose};
use mechcore_simulation::{Record, SimulationResult};
use serde::{Deserialize, Serialize};

use crate::{
    cli::{Args, Failure, Format, Outcome, Verdict},
    convert::{Answer, Request},
    kind::Kind,
};

pub(crate) const SCHEMA: &str = "mechcore.play-result";
pub(crate) const HTML_SCHEMA: &str = "mechcore.convert-html-result";

/// What `play` reports when its local playback server is listening.
#[derive(Serialize)]
pub(crate) struct Played {
    schema: &'static str,
    input: String,
    kind: &'static str,
    producer: &'static str,
    url: String,
    rounds: usize,
    ticks: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed_source: Option<&'static str>,
    cached: bool,
    opened: bool,
}

#[derive(Serialize)]
struct HtmlReport {
    schema: &'static str,
    from: &'static str,
    to: &'static str,
    backend: &'static str,
    output: String,
    ticks: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    rounds: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed_source: Option<&'static str>,
}

/// Reads `play <file> [--seed <i32>] [--port <u16>] [--no-open] [--no-cache]`
/// off a command line, serves the live player over HTTP, and blocks until
/// interrupted.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let seed = arguments.parsed::<i32>("--seed", "a signed 32-bit integer")?;
    let port = arguments
        .parsed::<u16>("--port", "a TCP port (0..=65535)")?
        .unwrap_or(0);
    let no_open = arguments.flag("--no-open")?;
    let no_cache = arguments.flag("--no-cache")?;
    let input = arguments.path("a file to play")?;
    check_trailing_args(arguments, &input)?;

    let (kind, bytes) = Kind::read(&input)?;
    kind.require("play")?;
    let session = prepare_session(kind, &input, &bytes, seed, None)?;
    let store = if no_cache {
        StoreContext::Memory
    } else {
        StoreContext::open_disk()
    };

    let round0 = load_or_compute_round(&session.specs[0], &store)?;
    let (all_rosters, all_arsenals) = collect_panel_assets(&session.specs, &round0.header);
    let meta: Vec<mechcore_player::LiveRoundMeta> = session
        .specs
        .iter()
        .enumerate()
        .map(|(index, spec)| mechcore_player::LiveRoundMeta {
            index,
            round: spec.round,
        })
        .collect();
    let page_html = mechcore_player::live_page_json(
        &round0.timeline_json,
        &meta,
        &all_rosters,
        &all_arsenals,
        &session.title,
    )
    .map_err(|error| Failure::failed(format!("cannot render live page: {error}")))?;

    let mut slots = Vec::with_capacity(session.specs.len());
    slots.push(RoundSlot::Ready {
        round: round0.header.round,
        timeline_json: round0.timeline_json,
    });
    for _ in 1..session.specs.len() {
        slots.push(RoundSlot::Pending);
    }

    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|error| Failure::failed(format!("cannot bind 127.0.0.1:{port}: {error}")))?;
    let local_addr = listener
        .local_addr()
        .map_err(|error| Failure::failed(format!("cannot read local listener address: {error}")))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| Failure::failed(format!("cannot configure listener: {error}")))?;
    let url = format!("http://{local_addr}/");

    let mut opened = false;
    if !no_open {
        match open(&url) {
            Ok(()) => opened = true,
            Err(error) => eprintln!("mechcore: serving {url} but cannot open it: {error}"),
        }
    }

    let played = Played {
        schema: SCHEMA,
        input: input.display().to_string(),
        kind: kind.name(),
        producer: session.producer,
        url,
        rounds: session.specs.len(),
        ticks: round0.header.ticks,
        seed: round0.header.seed,
        seed_source: known_seed_source(round0.header.seed_source.as_deref()),
        cached: round0.from_cache,
        opened,
    };
    emit_played(&played, format)?;

    let engine = SharedEngine {
        specs: session.specs,
        store,
        state: Mutex::new(EngineState {
            slots,
            priority: None,
            shutdown: false,
        }),
        changed: Condvar::new(),
    };
    serve(&listener, &engine, &page_html);
    Ok(Verdict::Yes)
}

/// Converts any battle input (`layout`, `fight`, `match`, `mcfr`, or `grbr`)
/// into a standalone self-contained HTML file.
pub(crate) fn convert_html(request: &Request, from: Kind, bytes: &[u8]) -> Result<Answer, Failure> {
    let output = request.output.as_deref().ok_or_else(|| {
        Failure::usage("converting to html writes a file; name it after the input")
    })?;
    let session = prepare_session(from, &request.input, bytes, request.seed, request.round)?;

    let mut timelines = Vec::with_capacity(session.specs.len());
    let mut total_ticks = 0u32;
    let mut seed = None;
    let mut seed_source = None;
    let mut producer = session.producer;

    for spec in &session.specs {
        let computed = compute_round(spec)?;
        total_ticks = total_ticks.saturating_add(computed.timeline.ticks);
        producer = computed.producer;
        if seed.is_none() {
            seed = computed.seed;
            seed_source = computed.seed_source;
        }
        timelines.push(computed.timeline);
    }

    let html = if session.single_round || timelines.len() == 1 {
        mechcore_player::page(&timelines[0], &session.title)
    } else {
        mechcore_player::match_page(&timelines, &session.title)
    }
    .map_err(|error| Failure::failed(format!("cannot render the page: {error}")))?;

    std::fs::write(output, html)
        .map_err(|error| Failure::failed(format!("cannot write {}: {error}", output.display())))?;

    let rounds = if session.single_round {
        None
    } else {
        Some(timelines.len())
    };
    let report = HtmlReport {
        schema: HTML_SCHEMA,
        from: from.name(),
        to: Kind::Html.name(),
        backend: producer,
        output: output.display().to_string(),
        ticks: total_ticks,
        rounds,
        seed,
        seed_source,
    };
    let text = match rounds {
        Some(count) => format!(
            "{} from {} ({count} rounds, {total_ticks} ticks)",
            report.output, report.from
        ),
        None => format!(
            "{} from {} ({total_ticks} ticks)",
            report.output, report.from
        ),
    };
    Ok(Answer::Report {
        value: serde_json::to_value(&report)
            .map_err(|error| Failure::failed(format!("cannot serialize result: {error}")))?,
        text: Some(text),
    })
}

fn check_trailing_args(arguments: Args, input: &Path) -> Result<(), Failure> {
    if arguments.is_empty() {
        return Ok(());
    }
    let rest = arguments.rest();
    if let Some(option) = rest.iter().find(|item| item.starts_with('-')) {
        return Err(Failure::usage(format!("unknown option {option:?}")));
    }
    let extra = &rest[0];
    Err(Failure::usage(format!(
        "unexpected argument {extra:?}; play serves over HTTP now — write a standalone page with `mechcore convert {} --to html {extra}`",
        input.display()
    )))
}

fn emit_played(played: &Played, format: Format) -> Result<(), Failure> {
    match format {
        Format::Json => {
            let line = serde_json::to_string(played)
                .map_err(|error| Failure::failed(format!("cannot write the result: {error}")))?;
            println!("{line}");
        }
        other => crate::cli::emit(played, other)?,
    }
    std::io::stdout()
        .flush()
        .map_err(|error| Failure::failed(format!("cannot flush standard output: {error}")))
}

fn known_seed_source(source: Option<&str>) -> Option<&'static str> {
    match source {
        Some("external") => Some("external"),
        Some("layout") => Some("layout"),
        Some("generated") => Some("generated"),
        Some("match") => Some("match"),
        _ => None,
    }
}

struct PreparedSession {
    title: String,
    producer: &'static str,
    single_round: bool,
    specs: Vec<RoundSpec>,
}

struct RoundSpec {
    round: i32,
    key: String,
    source: RoundSource,
}

struct MatchRoundData {
    economy: &'static mechcore_document::economy::Economy,
    stated: Arc<mechcore_document::opening::Stated>,
    turn_index: usize,
    declined: Option<i32>,
    deployment_layout_yaml: Vec<u8>,
    sides: [mechcore_document::r#match::SideState; 2],
    rosters: [Vec<mechcore_player::RosterEntry>; 2],
    arsenals: [mechcore_player::Arsenal; 2],
}

enum RoundSource {
    Mcfr {
        input: PathBuf,
    },
    Layout {
        input: Option<PathBuf>,
        yaml: Vec<u8>,
        seed: Option<i32>,
        layout_core_one: bool,
    },
    MatchRound(Box<MatchRoundData>),
}

struct ComputedRound {
    producer: &'static str,
    seed: Option<i32>,
    seed_source: Option<&'static str>,
    timeline: mechcore_player::Timeline,
}

fn prepare_session(
    kind: Kind,
    input: &Path,
    bytes: &[u8],
    seed: Option<i32>,
    round_filter: Option<i32>,
) -> Result<PreparedSession, Failure> {
    if seed.is_some() && kind != Kind::Layout {
        return Err(Failure::usage(format!(
            "--seed belongs to a layout; a {} states its own",
            kind.name()
        )));
    }
    let title = input.file_stem().map_or_else(
        || {
            if kind == Kind::Match {
                "match".to_owned()
            } else {
                "fight".to_owned()
            }
        },
        |stem| stem.to_string_lossy().into_owned(),
    );
    let file_hash = blake3::hash(bytes).to_hex();

    match kind {
        Kind::Mcfr => {
            // Kind::read leaves recordings on disk rather than loading their
            // bytes. Hash the archive itself, including optional instruments.
            let file_hash = recording_hash(input)?.to_hex();
            let reader = McfrReader::open(input).map_err(|error| {
                Failure::refused(format!("cannot read {}: {error}", input.display()))
            })?;
            let round = crate::scene::layout(&reader).map_or(1, |layout| layout.round);
            let producer = if reader.producer().as_str() == "game" {
                "game"
            } else {
                "simulator"
            };
            Ok(PreparedSession {
                title,
                producer,
                single_round: true,
                specs: vec![RoundSpec {
                    round,
                    key: spec_key("mcfr", &file_hash, 0, round, None),
                    source: RoundSource::Mcfr {
                        input: input.to_path_buf(),
                    },
                }],
            })
        }
        Kind::Layout => {
            let layout = mechcore_document::parse_yaml(bytes).map_err(Failure::refused)?;
            Ok(PreparedSession {
                title,
                producer: "simulator",
                single_round: true,
                specs: vec![RoundSpec {
                    round: layout.round,
                    key: spec_key("layout", &file_hash, 0, layout.round, seed),
                    source: RoundSource::Layout {
                        input: Some(input.to_path_buf()),
                        yaml: bytes.to_vec(),
                        seed,
                        layout_core_one: true,
                    },
                }],
            })
        }
        Kind::Fight => {
            let fight = mechcore_document::fight::parse_yaml(bytes).map_err(Failure::refused)?;
            let round = fight.round;
            let layout_yaml =
                mechcore_document::canonical_yaml(mechcore_document::fight::project(&fight))
                    .map_err(Failure::failed)?;
            Ok(PreparedSession {
                title,
                producer: "simulator",
                single_round: true,
                specs: vec![RoundSpec {
                    round,
                    key: spec_key("fight", &file_hash, 0, round, None),
                    source: RoundSource::Layout {
                        input: None,
                        yaml: layout_yaml.into_bytes(),
                        seed: None,
                        layout_core_one: false,
                    },
                }],
            })
        }
        Kind::Match => {
            let stated = mechcore_document::opening::stated(bytes)
                .map_err(Failure::refused)?
                .ok_or_else(|| Failure::refused("the document holds no match"))?;
            prepare_match_session(title, &file_hash, stated, round_filter)
        }
        Kind::Grbr => prepare_grbr_session(title, &file_hash, bytes, round_filter),
        other => Err(Failure::refused(format!(
            "play does not take a {} file; {}",
            other.name(),
            other.takes()
        ))),
    }
}

fn recording_hash(input: &Path) -> Result<blake3::Hash, Failure> {
    let mut hasher = blake3::Hasher::new();
    std::fs::File::open(input)
        .and_then(|file| hasher.update_reader(file).map(|_| ()))
        .map_err(|error| Failure::failed(format!("cannot hash {}: {error}", input.display())))?;
    Ok(hasher.finalize())
}

fn prepare_grbr_session(
    title: String,
    file_hash: &str,
    bytes: &[u8],
    round_filter: Option<i32>,
) -> Result<PreparedSession, Failure> {
    let selected =
        mechcore_document::record::select_round(bytes, round_filter).map_err(Failure::refused)?;
    if selected.synthetic {
        let layout = mechcore_document::layout_replay::read(bytes, selected.round)
            .map_err(Failure::refused)?;
        let round = layout.round;
        let layout_yaml = mechcore_document::canonical_yaml(layout).map_err(Failure::failed)?;
        return Ok(PreparedSession {
            title,
            producer: "simulator",
            single_round: true,
            specs: vec![RoundSpec {
                round,
                key: spec_key("grbr-layout", file_hash, 0, round, None),
                source: RoundSource::Layout {
                    input: None,
                    yaml: layout_yaml.into_bytes(),
                    seed: None,
                    layout_core_one: false,
                },
            }],
        });
    }
    let economy = mechcore_document::economy::Economy::embedded().map_err(Failure::failed)?;
    let converted =
        mechcore_document::convert::document(economy, bytes).map_err(Failure::refused)?;
    prepare_match_session(title, file_hash, converted.stated, round_filter)
}

fn prepare_match_session(
    title: String,
    file_hash: &str,
    stated: mechcore_document::opening::Stated,
    round_filter: Option<i32>,
) -> Result<PreparedSession, Failure> {
    crate::r#match::warn_build(&stated.build);
    let economy = mechcore_document::economy::Economy::embedded().map_err(Failure::failed)?;
    let reinforcement = mechcore_document::opening::verify(economy, &stated)
        .and_then(|opening| mechcore_document::reinforcement::verify(economy, &stated, &opening))
        .map_err(Failure::refused)?;

    let stated = Arc::new(stated);
    let mut specs = Vec::new();
    let mut research_history = [BTreeMap::new(), BTreeMap::new()];
    let mut matched_unplayable_round = false;

    for (turn_index, turn) in stated.turns.iter().enumerate() {
        let is_trailing_opening = turn_index + 1 == stated.turns.len() && !stated.ends_on_actions;
        let conceded = turn
            .actions
            .blue
            .iter()
            .chain(&turn.actions.red)
            .any(|action| matches!(action, mechcore_document::r#match::Action::Concede));
        if is_trailing_opening || conceded {
            if round_filter == Some(turn.round) {
                matched_unplayable_round = true;
            }
            continue;
        }

        let declined = reinforcement.declined(turn);
        let deployment = crate::convert::project_match(
            economy,
            &turn.state,
            &turn.actions,
            turn.round,
            stated.map_id,
            stated.seed,
            declined,
        )
        .map_err(Failure::refused)?;
        let rosters = match_rosters(
            economy,
            &deployment,
            turn,
            &stated,
            declined,
            &mut research_history,
        )?;
        let sides = [deployment.state.blue.clone(), deployment.state.red.clone()];
        let arsenals = [
            mechcore_player::arsenal(&sides[0]),
            mechcore_player::arsenal(&sides[1]),
        ];
        let deployment_layout_yaml =
            mechcore_document::canonical_yaml(deployment.layout).map_err(Failure::failed)?;

        if round_filter.is_some_and(|target| target != turn.round) {
            continue;
        }
        specs.push(RoundSpec {
            round: turn.round,
            key: spec_key("match", file_hash, turn_index, turn.round, None),
            source: RoundSource::MatchRound(Box::new(MatchRoundData {
                economy,
                stated: Arc::clone(&stated),
                turn_index,
                declined,
                deployment_layout_yaml: deployment_layout_yaml.into_bytes(),
                sides,
                rosters,
                arsenals,
            })),
        });
    }

    if specs.is_empty() {
        if let Some(target) = round_filter {
            return Err(Failure::refused(if matched_unplayable_round {
                format!("round {target} holds no completed deployment to convert")
            } else {
                format!(
                    "this match holds no round {target}; it holds {} of them",
                    stated.turns.len()
                )
            }));
        }
        return Err(Failure::refused(
            "this match holds no completed deployment to play",
        ));
    }

    Ok(PreparedSession {
        title,
        producer: "simulator",
        single_round: round_filter.is_some(),
        specs,
    })
}

fn spec_key(
    domain: &str,
    file_hash: &str,
    turn_index: usize,
    round: i32,
    seed: Option<i32>,
) -> String {
    let seed_tag = seed.map_or_else(|| "none".to_owned(), |value| value.to_string());
    blake3::hash(format!("{domain}:{file_hash}:{turn_index}:{round}:{seed_tag}").as_bytes())
        .to_hex()
        .to_string()
}

fn collect_panel_assets(
    specs: &[RoundSpec],
    round0_header: &CachedHeader,
) -> (
    Vec<mechcore_player::RosterEntry>,
    Vec<mechcore_player::Arsenal>,
) {
    if specs.len() <= 1 {
        let rosters = round0_header.rosters.iter().flatten().cloned().collect();
        let arsenals = round0_header.arsenals.to_vec();
        return (rosters, arsenals);
    }
    let mut rosters = Vec::new();
    let mut arsenals = Vec::new();
    for spec in specs {
        if let RoundSource::MatchRound(data) = &spec.source {
            rosters.extend(data.rosters.iter().flatten().cloned());
            arsenals.extend(data.arsenals.iter().cloned());
        }
    }
    (rosters, arsenals)
}

fn compute_round(spec: &RoundSpec) -> Result<ComputedRound, Failure> {
    match &spec.source {
        RoundSource::Mcfr { input } => {
            let reader = McfrReader::open(input).map_err(|error| {
                Failure::refused(format!("cannot read {}: {error}", input.display()))
            })?;
            let poses = reader.instrument::<UnitPose>().map_err(|error| {
                Failure::refused(format!("cannot read {}'s poses: {error}", input.display()))
            })?;
            let timeline = player_timeline(&reader, poses.as_deref())
                .map_err(|error| Failure::failed(format!("cannot read the fight: {error}")))?;
            let producer = if reader.producer().as_str() == "game" {
                "game"
            } else {
                "simulator"
            };
            Ok(ComputedRound {
                producer,
                seed: None,
                seed_source: None,
                timeline,
            })
        }
        RoundSource::Layout {
            input,
            yaml,
            seed,
            layout_core_one,
        } => {
            let result = match input.as_deref() {
                Some(path) => mechcore_simulation::simulate_layout(path, Record::Memory, *seed),
                None => mechcore_simulation::simulate_document(yaml, Record::Memory, *seed),
            };
            let result = result.map_err(|error| Failure::unsimulated(&error))?;
            let recording = result
                .recording
                .ok_or_else(|| Failure::failed("the simulator kept no recording of the fight"))?;
            let mut timeline = player_timeline(&recording, None)
                .map_err(|error| Failure::failed(format!("cannot read the fight: {error}")))?;
            if *layout_core_one {
                for core in &mut timeline.cores {
                    core.begin = Some(1);
                    core.maximum = Some(1);
                }
            }
            Ok(ComputedRound {
                producer: "simulator",
                seed: Some(result.seed),
                seed_source: Some(result.seed_source),
                timeline,
            })
        }
        RoundSource::MatchRound(data) => {
            let timeline = compute_match_round(
                data.economy,
                &data.stated.turns[data.turn_index],
                &data.stated,
                data.declined,
                &data.deployment_layout_yaml,
                [&data.sides[0], &data.sides[1]],
                data.rosters.clone(),
                data.arsenals.clone(),
            )?;
            Ok(ComputedRound {
                producer: "simulator",
                seed: Some(data.stated.seed),
                seed_source: Some("match"),
                timeline,
            })
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compute_match_round(
    economy: &mechcore_document::economy::Economy,
    turn: &mechcore_document::r#match::Turn,
    stated: &mechcore_document::opening::Stated,
    declined: Option<i32>,
    deployment_layout_yaml: &[u8],
    sides: [&mechcore_document::r#match::SideState; 2],
    rosters: [Vec<mechcore_player::RosterEntry>; 2],
    arsenals: [mechcore_player::Arsenal; 2],
) -> Result<mechcore_player::Timeline, Failure> {
    let result: SimulationResult =
        mechcore_simulation::simulate_document(deployment_layout_yaml, Record::Memory, None)
            .map_err(|error| Failure::unsimulated(&error))?;
    let recording = result
        .recording
        .ok_or_else(|| Failure::failed("the simulator kept no recording of the fight"))?;
    let mut timeline = player_timeline(&recording, None)
        .map_err(|error| Failure::failed(format!("cannot read round {}: {error}", turn.round)))?;
    let formation_ids: BTreeMap<(u32, i32), u64> = crate::scene::layout(&recording)
        .and_then(|layout| {
            let opened = crate::scene::snapshot(&recording, 1)?;
            crate::scene::formations(&layout, &opened)
        })?
        .into_iter()
        .map(|(id, (side, index))| ((u32::from(side == crate::turn::Side::Red), index), id))
        .collect();
    timeline.lead_in = mechcore_player::lead_in(
        economy,
        [&turn.state.blue, &turn.state.red],
        &turn.actions,
        declined,
        &formation_ids,
        timeline.ticks_per_second,
    )
    .map_err(|error| {
        Failure::failed(format!(
            "cannot replay round {}'s deployment: {error}",
            turn.round
        ))
    })?;
    timeline.rosters = rosters;
    timeline.arsenals = arsenals;
    match_tower_cards(&mut timeline, sides)?;
    for (seat, (header, state)) in [
        (&stated.blue, &turn.state.blue),
        (&stated.red, &turn.state.red),
    ]
    .into_iter()
    .enumerate()
    {
        let core = &mut timeline.cores[seat];
        core.name = header.name.clone().unwrap_or_else(|| core.name.clone());
        core.begin = Some(state.reactor_core);
        core.supply = sides[seat].supply;
        core.maximum = header.opening.as_ref().and_then(|opening| {
            let base = mechcore_document::opening::reactor_core(stated.map_id, seat).ok()?;
            let team = economy.advance_team(opening.taken.team)?;
            let specialist = economy.advance_team(opening.taken.specialist)?;
            Some(base + team.reactor_core + specialist.reactor_core)
        });
    }
    Ok(timeline)
}

#[derive(Clone, Deserialize, Serialize)]
struct CachedHeader {
    schema: String,
    producer: String,
    round: i32,
    ticks: u32,
    seed: Option<i32>,
    seed_source: Option<String>,
    rosters: [Vec<mechcore_player::RosterEntry>; 2],
    arsenals: [mechcore_player::Arsenal; 2],
}

struct LoadedRound {
    header: CachedHeader,
    timeline_json: String,
    from_cache: bool,
}

enum StoreContext {
    Memory,
    Disk { dir: PathBuf, build: String },
}

impl StoreContext {
    fn open_disk() -> Self {
        let dir = cache_directory();
        match (std::fs::create_dir_all(&dir), build_fingerprint()) {
            (Ok(()), Ok(build)) => Self::Disk { dir, build },
            (Err(error), _) => {
                eprintln!(
                    "mechcore: play cache directory {} cannot be created ({error}); keeping cache in memory",
                    dir.display()
                );
                Self::Memory
            }
            (_, Err(error)) => {
                eprintln!(
                    "mechcore: play cache build fingerprint unavailable ({error}); keeping cache in memory"
                );
                Self::Memory
            }
        }
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        let Self::Disk { dir, build } = self else {
            return None;
        };
        Some(dir.join(format!("{}-{}.json", &key[..32], &build[..16])))
    }
}

fn cache_directory() -> PathBuf {
    match std::env::var_os("MECHCORE_PLAY_CACHE") {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => {
            let tmp = Path::new("/tmp");
            if cfg!(unix) && tmp.is_dir() {
                tmp.join("mechcore-play")
            } else {
                std::env::temp_dir().join("mechcore-play")
            }
        }
    }
}

fn build_fingerprint() -> Result<String, String> {
    let binary = std::env::current_exe()
        .and_then(std::fs::metadata)
        .map_err(|error| error.to_string())?;
    let modified = binary
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    let material = format!(
        "{}:{modified}:{}:{}",
        binary.len(),
        mechcore_player::SCHEMA,
        mechcore_document::game_build()
    );
    Ok(blake3::hash(material.as_bytes()).to_hex().to_string())
}

fn load_or_compute_round(spec: &RoundSpec, store: &StoreContext) -> Result<LoadedRound, Failure> {
    if let Some(path) = store.path(&spec.key)
        && let Some(cached) = read_cached_round(&path)
    {
        return Ok(cached);
    }

    let computed = compute_round(spec)?;
    let timeline_json = serde_json::to_string(&computed.timeline)
        .map_err(|error| Failure::failed(format!("cannot serialize round timeline: {error}")))?;
    let header = CachedHeader {
        schema: mechcore_player::SCHEMA.to_owned(),
        producer: computed.producer.to_owned(),
        round: computed.timeline.round,
        ticks: computed.timeline.ticks,
        seed: computed.seed,
        seed_source: computed.seed_source.map(str::to_owned),
        rosters: computed.timeline.rosters,
        arsenals: computed.timeline.arsenals,
    };

    if let Some(path) = store.path(&spec.key)
        && let Err(error) = write_cached_round(&path, &header, &timeline_json)
    {
        eprintln!(
            "mechcore: cannot write play cache {}: {error}",
            path.display()
        );
    }

    Ok(LoadedRound {
        header,
        timeline_json,
        from_cache: false,
    })
}

fn read_cached_round(path: &Path) -> Option<LoadedRound> {
    let content = std::fs::read_to_string(path).ok()?;
    let (header_line, body) = content.split_once('\n')?;
    let header: CachedHeader = serde_json::from_str(header_line).ok()?;
    if header.schema != mechcore_player::SCHEMA {
        return None;
    }
    let timeline_json = body.trim();
    if !(timeline_json.starts_with('{') && timeline_json.ends_with('}')) {
        return None;
    }
    Some(LoadedRound {
        header,
        timeline_json: timeline_json.to_owned(),
        from_cache: true,
    })
}

fn write_cached_round(
    path: &Path,
    header: &CachedHeader,
    timeline_json: &str,
) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| "cache path has no parent directory".to_owned())?;
    let header_json = serde_json::to_string(header).map_err(|error| error.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(dir).map_err(|error| error.to_string())?;
    temp.write_all(header_json.as_bytes())
        .map_err(|error| error.to_string())?;
    temp.write_all(b"\n").map_err(|error| error.to_string())?;
    temp.write_all(timeline_json.as_bytes())
        .map_err(|error| error.to_string())?;
    temp.flush().map_err(|error| error.to_string())?;
    temp.persist(path)
        .map_err(|error| error.error.to_string())?;
    Ok(())
}

enum RoundSlot {
    Pending,
    Running,
    Ready { round: i32, timeline_json: String },
    Failed { reason: String },
}

struct SharedEngine {
    specs: Vec<RoundSpec>,
    store: StoreContext,
    state: Mutex<EngineState>,
    changed: Condvar,
}

struct EngineState {
    slots: Vec<RoundSlot>,
    priority: Option<usize>,
    shutdown: bool,
}

fn serve(listener: &TcpListener, engine: &SharedEngine, page_html: &str) {
    let _signal_guard = signal::install();
    std::thread::scope(|scope| {
        if engine.specs.len() > 1 {
            scope.spawn(|| worker_loop(engine));
        }

        while !signal::stopped() {
            match listener.accept() {
                Ok((stream, _)) => {
                    scope.spawn(move || handle_connection(stream, engine, page_html));
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => {
                    eprintln!("mechcore: HTTP listener error: {error}");
                    break;
                }
            }
        }

        if let Ok(mut guard) = engine.state.lock() {
            guard.shutdown = true;
            engine.changed.notify_all();
        }
    });
}

fn worker_loop(engine: &SharedEngine) {
    loop {
        let next = {
            let mut state = match engine.state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if state.shutdown || signal::stopped() {
                return;
            }
            let picked = state
                .priority
                .take()
                .filter(|&index| matches!(state.slots.get(index), Some(RoundSlot::Pending)))
                .or_else(|| {
                    state
                        .slots
                        .iter()
                        .position(|slot| matches!(slot, RoundSlot::Pending))
                });
            let Some(index) = picked else {
                return;
            };
            state.slots[index] = RoundSlot::Running;
            engine.changed.notify_all();
            index
        };

        let result = load_or_compute_round(&engine.specs[next], &engine.store);
        let mut state = match engine.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        state.slots[next] = match result {
            Ok(loaded) => RoundSlot::Ready {
                round: loaded.header.round,
                timeline_json: loaded.timeline_json,
            },
            Err(error) => RoundSlot::Failed {
                reason: error.reason().to_owned(),
            },
        };
        engine.changed.notify_all();
    }
}

fn handle_connection(mut stream: TcpStream, engine: &SharedEngine, page_html: &str) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));

    let Some((method, target)) = read_request_line(&stream) else {
        return;
    };
    if method != "GET" {
        let _ = write_http_response(
            &mut stream,
            "405 Method Not Allowed",
            "application/json",
            br#"{"error":"method not allowed"}"#,
        );
        return;
    }

    let path = target.split('?').next().unwrap_or(target.as_str());
    match path {
        "/" | "/index.html" => {
            let _ = write_http_response(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                page_html.as_bytes(),
            );
        }
        "/api/status" => {
            let body = status_json(engine);
            let _ = write_http_response(&mut stream, "200 OK", "application/json", body.as_bytes());
        }
        _ => {
            if let Some(rest) = path.strip_prefix("/api/round/")
                && let Ok(index) = rest.parse::<usize>()
                && index < engine.specs.len()
            {
                serve_round(&mut stream, engine, index);
                return;
            }
            let _ = write_http_response(
                &mut stream,
                "404 Not Found",
                "application/json",
                br#"{"error":"not found"}"#,
            );
        }
    }
}

fn read_request_line(stream: &TcpStream) -> Option<(String, String)> {
    let mut reader = BufReader::new(stream);
    let mut first = String::new();
    if reader.read_line(&mut first).ok()? == 0 {
        return None;
    }
    let mut parts = first.split_whitespace();
    let method = parts.next()?.to_owned();
    let target = parts.next()?.to_owned();
    let mut header = String::new();
    while reader.read_line(&mut header).is_ok() {
        if header == "\r\n" || header == "\n" || header.is_empty() {
            break;
        }
        header.clear();
    }
    Some((method, target))
}

#[derive(Serialize)]
struct StatusRound {
    index: usize,
    round: i32,
    ready: bool,
    failed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn status_json(engine: &SharedEngine) -> String {
    let state = match engine.state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let rounds: Vec<StatusRound> = engine
        .specs
        .iter()
        .enumerate()
        .map(|(index, spec)| match &state.slots[index] {
            RoundSlot::Ready { round, .. } => StatusRound {
                index,
                round: *round,
                ready: true,
                failed: false,
                error: None,
            },
            RoundSlot::Failed { reason } => StatusRound {
                index,
                round: spec.round,
                ready: false,
                failed: true,
                error: Some(reason.clone()),
            },
            RoundSlot::Pending | RoundSlot::Running => StatusRound {
                index,
                round: spec.round,
                ready: false,
                failed: false,
                error: None,
            },
        })
        .collect();
    serde_json::json!({ "rounds": rounds }).to_string()
}

fn serve_round(stream: &mut TcpStream, engine: &SharedEngine, index: usize) {
    let mut state = match engine.state.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    loop {
        if state.shutdown || signal::stopped() {
            drop(state);
            let _ = write_http_response(
                stream,
                "503 Service Unavailable",
                "application/json",
                br#"{"error":"server shutting down"}"#,
            );
            return;
        }
        match &state.slots[index] {
            RoundSlot::Ready { timeline_json, .. } => {
                let body = timeline_json.clone();
                drop(state);
                let _ = write_http_response(stream, "200 OK", "application/json", body.as_bytes());
                return;
            }
            RoundSlot::Failed { reason } => {
                let body = serde_json::json!({ "error": reason }).to_string();
                drop(state);
                let _ = write_http_response(
                    stream,
                    "500 Internal Server Error",
                    "application/json",
                    body.as_bytes(),
                );
                return;
            }
            RoundSlot::Pending => {
                state.priority = Some(index);
            }
            RoundSlot::Running => {}
        }
        let (guard, _) = match engine
            .changed
            .wait_timeout(state, Duration::from_millis(50))
        {
            Ok(waited) => waited,
            Err(poisoned) => poisoned.into_inner(),
        };
        state = guard;
    }
}

fn write_http_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let mut header = String::new();
    let _ = write!(
        header,
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

#[cfg(unix)]
mod signal {
    use std::sync::atomic::{AtomicBool, Ordering};

    static STOP: AtomicBool = AtomicBool::new(false);

    extern "C" fn handle(_signal: libc::c_int) {
        STOP.store(true, Ordering::SeqCst);
    }

    pub(super) struct Guard {
        previous_int: libc::sigaction,
        previous_term: libc::sigaction,
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            unsafe {
                libc::sigaction(
                    libc::SIGINT,
                    &raw const self.previous_int,
                    std::ptr::null_mut(),
                );
                libc::sigaction(
                    libc::SIGTERM,
                    &raw const self.previous_term,
                    std::ptr::null_mut(),
                );
            }
        }
    }

    #[allow(
        clippy::fn_to_numeric_cast,
        reason = "POSIX sigaction registers a C handler address via sa_sigaction"
    )]
    pub(super) fn install() -> Guard {
        STOP.store(false, Ordering::SeqCst);
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = handle as *const () as usize;
        unsafe {
            libc::sigemptyset(&raw mut action.sa_mask);
        }
        let mut previous_int: libc::sigaction = unsafe { std::mem::zeroed() };
        let mut previous_term: libc::sigaction = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigaction(libc::SIGINT, &raw const action, &raw mut previous_int);
            libc::sigaction(libc::SIGTERM, &raw const action, &raw mut previous_term);
        }
        Guard {
            previous_int,
            previous_term,
        }
    }

    pub(super) fn stopped() -> bool {
        STOP.load(Ordering::SeqCst)
    }
}

#[cfg(not(unix))]
mod signal {
    pub(super) struct Guard;

    pub(super) fn install() -> Guard {
        Guard
    }

    pub(super) fn stopped() -> bool {
        false
    }
}

fn match_tower_cards(
    timeline: &mut mechcore_player::Timeline,
    sides: [&mechcore_document::r#match::SideState; 2],
) -> Result<(), Failure> {
    for (core, state) in timeline.cores.iter_mut().zip(sides) {
        core.tower_cards =
            mechcore_player::tower_cards(&state.energy_tower_skills, &state.blueprints)
                .map_err(|error| Failure::failed(error.to_string()))?;
    }
    Ok(())
}

fn match_rosters(
    economy: &mechcore_document::economy::Economy,
    deployment: &mechcore_document::project::Deployment,
    turn: &mechcore_document::r#match::Turn,
    stated: &mechcore_document::opening::Stated,
    declined: Option<i32>,
    research_history: &mut [BTreeMap<i32, i32>; 2],
) -> Result<[Vec<mechcore_player::RosterEntry>; 2], Failure> {
    let sides = [&deployment.state.blue, &deployment.state.red];
    let layouts = [&deployment.layout.blue, &deployment.layout.red];
    let loadouts = [&stated.blue.tech_loadout, &stated.red.tech_loadout];
    let mut rosters = std::array::from_fn(|seat| {
        mechcore_player::unit_roster(
            layouts[seat],
            &sides[seat].unlocked_units,
            Some(loadouts[seat]),
        )
    });
    for seat in 0..2 {
        let (opening, actions) = if seat == 0 {
            (&turn.state.blue, &turn.actions.blue)
        } else {
            (&turn.state.red, &turn.actions.red)
        };
        remember_research_prices(
            economy,
            opening,
            actions,
            seat == 1,
            declined,
            &mut research_history[seat],
        )
        .map_err(Failure::refused)?;
        for row in &mut rosters[seat] {
            row.investment_supply =
                unit_investment(sides[seat], &row.kind, &research_history[seat]);
            row.count_before = Some(
                opening
                    .units
                    .iter()
                    .filter(|unit| unit.unit.type_name == row.kind)
                    .count(),
            );
            for tech in &mut row.techs {
                tech.activated_this_round = tech.active && !opening.techs.contains(&tech.id);
                tech.purchase_supply = if tech.active {
                    research_history[seat].get(&tech.id).copied()
                } else {
                    mechcore_document::transition::technology_price(economy, sides[seat], tech.id)
                };
            }
        }
    }
    Ok(rosters)
}

fn unit_investment(
    state: &mechcore_document::r#match::SideState,
    kind: &str,
    research_history: &BTreeMap<i32, i32>,
) -> i64 {
    let formations: i64 = state
        .units
        .iter()
        .filter(|unit| unit.unit.type_name == kind)
        .filter_map(|unit| unit.value)
        .map(i64::from)
        .sum();
    let technologies: i64 = state
        .techs
        .iter()
        .filter(|id| {
            mechcore_document::names::technology_owner(**id)
                .and_then(mechcore_document::unit_type_from_id)
                .is_some_and(|(name, _)| name == kind)
        })
        .filter_map(|id| research_history.get(id))
        .copied()
        .map(i64::from)
        .sum();
    formations + technologies
}

/// Replay deployment decisions in their recorded order: later discounts and
/// research must not retroactively change the cost of an earlier purchase.
fn remember_research_prices(
    economy: &mechcore_document::economy::Economy,
    opening: &mechcore_document::r#match::SideState,
    actions: &[mechcore_document::r#match::Action],
    red: bool,
    declined: Option<i32>,
    history: &mut BTreeMap<i32, i32>,
) -> Result<(), String> {
    history.retain(|id, _| opening.techs.contains(id));
    let mut state = opening.clone();
    for action in actions {
        let next = mechcore_document::transition::deployed(
            economy,
            &state,
            std::slice::from_ref(action),
            red,
            declined,
        )
        .map_err(|error| error.to_string())?;
        if let mechcore_document::r#match::Action::UpgradeTechnology { tech, .. } = action {
            history.insert(*tech, state.supply - next.supply);
        }
        state = next;
    }
    Ok(())
}

fn player_timeline(
    recording: &dyn Recording,
    poses: Option<&[(u32, UnitPose)]>,
) -> Result<mechcore_player::Timeline, mechcore_player::Error> {
    let mut timeline = mechcore_player::timeline(recording, poses)?;
    let scores = deployed_levels(recording).and_then(|levels| {
        for unit in &mut timeline.units {
            unit.level = levels.get(&unit.id).map(|level| level.unwrap_or(1));
        }
        score_sources(recording, &levels)
    });
    match scores {
        Ok(sources) => timeline.score_sources = sources,
        Err(error) => eprintln!(
            "mechcore: cannot attribute settlement to survivors; showing team totals: {}",
            error.reason()
        ),
    }
    if timeline
        .deployments
        .iter()
        .any(|d| matches!(d.kind.as_str(), "orbital_bombardment" | "lightning_storm"))
    {
        match mechcore_simulation::battle_skill_forecasts(recording.layout_yaml().as_bytes()) {
            Ok(plans) => {
                for plan in plans {
                    // Repeated releases of the same skill keep their panel order.
                    if let Some(deployment) = timeline.deployments.iter_mut().find(|d| {
                        d.team == plan.team && d.kind == plan.kind && d.forecast.is_none()
                    }) {
                        deployment.set_strike_forecast(plan.radius, plan.points);
                    }
                }
            }
            Err(error) => eprintln!(
                "mechcore: cannot reconstruct random strike forecast; showing release preview only: {error}"
            ),
        }
    }
    Ok(timeline)
}

/// Match by original unit ID so control and rebirth retain the deployment level.
fn deployed_levels(recording: &dyn Recording) -> Result<BTreeMap<u64, Option<i32>>, Failure> {
    let layout = crate::scene::layout(recording)?;
    let initial = crate::scene::snapshot(recording, 1)?;
    let formations = crate::scene::formations(&layout, &initial)?;
    let levels = formations
        .into_iter()
        .filter_map(|(formation, (side, index))| {
            crate::scene::units_of(&layout, side)
                .iter()
                .find(|p| p.index == index)
                .map(|p| (formation, p.level))
        })
        .collect::<BTreeMap<_, _>>();
    Ok(initial
        .live_units
        .iter()
        .filter_map(|u| levels.get(&u.formation_id).map(|level| (u.unit_id, *level)))
        .collect())
}

fn score_sources(
    recording: &dyn Recording,
    levels: &BTreeMap<u64, Option<i32>>,
) -> Result<Vec<mechcore_player::ScoreSource>, Failure> {
    use mechcore_document::reactor_damage::{Survivor, score};
    let final_state = crate::scene::snapshot(recording, recording.terminal_tick())?;
    let mut sources = Vec::new();
    for unit in final_state
        .live_units
        .iter()
        .filter(|u| u.active && u.life.current > 0)
    {
        // Control and rebirth can change formation identity. Creation type
        // belongs to the original unit, not its current formation.
        let level = levels.get(&unit.unit_id);
        let value = score(Survivor {
            unit_type: unit.unit_type_id,
            level: level.copied().flatten(),
            support: level.is_none(),
            reborn: unit.rebirth_count > 0,
            team_changed: unit.team_id != unit.original_team_id,
        })
        .map_err(Failure::refused)?;
        if value > 0 {
            let cm = |v: i64| i64::try_from((i128::from(v) * 100 + (1 << 31)) >> 32).unwrap_or(0);
            sources.push(mechcore_player::ScoreSource {
                unit: unit.unit_id,
                team: unit.team_id,
                score: value,
                x: cm(unit.position.x),
                y: cm(unit.position.y),
                z: cm(unit.position.z),
            });
        }
    }
    for team in [0, 1] {
        let total: i64 = sources
            .iter()
            .filter(|s| s.team == team)
            .map(|s| s.score)
            .sum();
        let recorded = i64::from(recording.settlement().teams[team as usize].score);
        if total != recorded {
            return Err(Failure::refused(format!(
                "team {team} survivor scores total {total}, settlement records {recorded}"
            )));
        }
    }
    Ok(sources)
}

/// Asks the system to open the URL in its default browser, without waiting
/// for the browser.
fn open(url: &str) -> std::io::Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        Command::new("open")
    } else if cfg!(target_os = "windows") {
        let mut start = Command::new("cmd");
        start.args(["/C", "start", ""]);
        start
    } else {
        Command::new("xdg-open")
    };
    command
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(drop)
}

#[cfg(test)]
mod price_tests {
    use super::*;
    use mechcore_document::r#match::{Action, ReinforceChoice, SideState};

    #[test]
    fn research_history_preserves_cost_before_later_discounts_and_across_rounds() {
        let economy = mechcore_document::economy::Economy::embedded().unwrap();
        let opening = SideState {
            supply: 5000,
            ..SideState::default()
        };
        let actions = [
            Action::UpgradeTechnology {
                unit: 5,
                tech: 1109,
            },
            Action::ChooseReinforceItem {
                index: 0,
                id: ReinforceChoice::Card(20003),
            },
            Action::UpgradeTechnology { unit: 5, tech: 905 },
        ];
        let mut history = BTreeMap::new();
        remember_research_prices(economy, &opening, &actions, false, None, &mut history).unwrap();
        assert_eq!(history[&1109], 150);
        assert_eq!(history[&905], 350);
        let next =
            mechcore_document::transition::deployed(economy, &opening, &actions, false, None)
                .unwrap();
        assert_eq!(
            mechcore_document::transition::technology_price(economy, &next, 3005),
            Some(550)
        );
        remember_research_prices(economy, &next, &[], false, None, &mut history).unwrap();
        assert_eq!(history[&1109], 150);
        assert_eq!(history[&905], 350);
    }

    #[test]
    fn an_existing_technology_without_history_has_no_invented_purchase_price() {
        let economy = mechcore_document::economy::Economy::embedded().unwrap();
        let opening = SideState {
            techs: vec![1109],
            ..SideState::default()
        };
        let mut history = BTreeMap::new();
        remember_research_prices(economy, &opening, &[], false, None, &mut history).unwrap();
        assert!(!history.contains_key(&1109));
    }

    #[test]
    fn cache_roundtrip_preserves_header_and_timeline_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("round-0.json");
        let header = CachedHeader {
            schema: mechcore_player::SCHEMA.to_owned(),
            producer: "simulator".to_owned(),
            round: 2,
            ticks: 42,
            seed: Some(7),
            seed_source: Some("layout".to_owned()),
            rosters: [Vec::new(), Vec::new()],
            arsenals: Default::default(),
        };
        let timeline = r#"{"schema":"mechcore.player","round":2,"ticks":42}"#;
        write_cached_round(&path, &header, timeline).unwrap();
        let loaded = read_cached_round(&path).unwrap();
        assert!(loaded.from_cache);
        assert_eq!(loaded.header.round, 2);
        assert_eq!(loaded.header.ticks, 42);
        assert_eq!(loaded.header.seed, Some(7));
        assert_eq!(loaded.timeline_json, timeline);
    }
}
