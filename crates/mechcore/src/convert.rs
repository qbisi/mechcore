//! `convert`: derives a file of another kind from one of these.
//!
//! A conversion is a rewrite, the same content in another form and refused
//! when the other form cannot hold it, or a computation, which derives facts
//! the input does not hold by running the fight. [`crate::kind`] says which
//! pairs exist and which of the two each is.
//!
//! The input is read, never written, and an existing destination is refused
//! unless the caller says to replace it: a conversion that silently replaced a
//! file would make a document's provenance unrecoverable.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::cli::{Args, Failure, Format, Outcome, Verdict};
use crate::kind::{Conversion, Kind};
use mechcore_simulation::Record;

/// How many unequal leaves to name before counting the rest.
const FAILURES_SHOWN: usize = 5;

/// One conversion, as its four callers name it.
pub(crate) struct Request {
    pub(crate) input: PathBuf,
    pub(crate) to: Kind,
    pub(crate) output: Option<PathBuf>,
    pub(crate) seed: Option<i32>,
    pub(crate) round: Option<i32>,
    /// Whether an existing output is replaced rather than refused.
    pub(crate) force: bool,
    /// Who fights a computation into a recording.
    pub(crate) backend: Backend,
    /// The instrument channels the game records into it.
    pub(crate) instrument: Vec<mechcore_protocol::InstrumentChannel>,
}

/// Who fights a file into a recording: the simulator this binary carries, or
/// the game, headless, through the Adapter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Backend {
    #[default]
    Simulator,
    Game,
}

impl Backend {
    /// The backend `--backend` names.
    ///
    /// # Errors
    ///
    /// Returns a usage failure for a name that is no backend.
    pub(crate) fn parse(name: &str) -> Result<Self, Failure> {
        match name {
            "simulator" => Ok(Self::Simulator),
            "game" => Ok(Self::Game),
            other => Err(Failure::usage(format!(
                "no backend is called {other:?}; the backends are simulator and game"
            ))),
        }
    }
}

/// What a conversion answers.
pub(crate) enum Answer {
    /// A report of what was written or computed, with its text rendering when
    /// it has one.
    Report { value: Value, text: Option<String> },
    /// The document itself, for a rewrite asked for without a destination.
    Document(String),
}

/// Reads `convert <in> --to <kind> [<out>]` off a command line.
///
/// # Errors
///
/// Returns a usage failure for a command the contract does not define, and
/// whatever the conversion refuses.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let force = arguments.flag("--force")?;
    let seed = arguments.parsed::<i32>("--seed", "a signed 32-bit integer")?;
    let round = arguments.parsed::<i32>("--round", "a round number")?;
    let backend = arguments
        .value("--backend")?
        .map_or(Ok(Backend::Simulator), |name| Backend::parse(&name))?;
    let instrument = crate::game::instrument(&mut arguments)?;
    let level = crate::acquire::level(&mut arguments)?;
    let profile = arguments.value("--profile")?.map(PathBuf::from);
    let to = arguments
        .value("--to")?
        .ok_or_else(|| Failure::usage("expected --to <kind>: the kind to convert to"))?;
    let to = parse_kind(&to)?;
    let input = arguments.path("a file to convert")?;
    let output = if arguments.is_empty() {
        None
    } else {
        Some(arguments.path("the file to write")?)
    };
    arguments.finish()?;
    let request = Request {
        input,
        to,
        output,
        seed,
        round,
        force,
        backend,
        instrument,
    };
    if backend == Backend::Game {
        if profile.is_some() {
            return Err(Failure::usage(
                "--profile samples this binary's own conversion; the game's is not sampled",
            ));
        }
        if request.to == Kind::Fight {
            return fought_in_game(&request, level, format);
        }
        return crate::game::record_attached(recorded(&request)?, force, level);
    }
    let mut answer = crate::profile::sampled(profile.as_deref(), force, || convert(&request))?;
    if let (Some(profile), Answer::Report { value, .. }) = (&profile, &mut answer)
        && let Some(fields) = value.as_object_mut()
    {
        fields.insert(
            "flamegraph".to_owned(),
            Value::String(profile.display().to_string()),
        );
    }
    match answer {
        Answer::Document(text) => print!("{text}"),
        Answer::Report {
            text: Some(text), ..
        } if format == Format::Text => println!("{text}"),
        Answer::Report { value, .. } => crate::cli::emit(&value, format)?,
    }
    Ok(Verdict::Yes)
}

/// The kind `--to` names.
///
/// # Errors
///
/// Returns a usage failure for a name that is no kind.
pub(crate) fn parse_kind(name: &str) -> Result<Kind, Failure> {
    Kind::parse(name).ok_or_else(|| {
        Failure::usage(format!(
            "no file is of kind {name:?}; the kinds are {}",
            Kind::ALL.map(Kind::name).join(", ")
        ))
    })
}

/// Converts one file.
///
/// # Errors
///
/// Returns a refusal for a pair of kinds with no conversion, an option the
/// pair does not take, an existing destination without `force`, and whatever
/// the conversion itself refuses.
pub(crate) fn convert(request: &Request) -> Result<Answer, Failure> {
    if request.backend == Backend::Game {
        return Err(Failure::usage(
            "the game fights a file into a recording in a session; see recorded()",
        ));
    }
    if !request.instrument.is_empty() {
        return Err(Failure::usage(
            "--instrument records what the game did; it takes --backend game",
        ));
    }
    let (from, bytes) = Kind::read(&request.input)?;
    let how = from.conversion(request.to).ok_or_else(|| {
        let reaches = from
            .conversions()
            .iter()
            .map(|(kind, _)| kind.name())
            .collect::<Vec<_>>();
        Failure::refused(if reaches.is_empty() {
            format!("a {} file converts to nothing", from.name())
        } else {
            format!(
                "a {} file does not convert to {}; it converts to {}",
                from.name(),
                request.to.name(),
                reaches.join(", ")
            )
        })
    })?;
    let takes_seed = from == Kind::Layout;
    let takes_round = matches!(
        (from, request.to),
        (Kind::Match, Kind::Layout) | (Kind::Grbr, Kind::Mcfr | Kind::Fight)
    );
    if request.seed.is_some() && !takes_seed {
        return Err(Failure::usage(format!(
            "--seed belongs to a layout; a {} states its own",
            from.name()
        )));
    }
    if request.round.is_some() && !takes_round {
        return Err(Failure::usage(
            "--round names the round of a match to convert to a layout",
        ));
    }
    let output = request.output.as_deref();
    // A rewrite that has no document form to answer with writes a file, and
    // answering with a binary file on standard output is not answering.
    let document = matches!(request.to, Kind::Layout | Kind::Fight);
    if output.is_none() && how == Conversion::Rewrite && !document {
        return Err(Failure::usage(format!(
            "converting to {} writes a file; name it after the input",
            request.to.name()
        )));
    }
    if let Some(output) = output
        && output.exists()
    {
        if !request.force {
            return Err(Failure::refused(format!(
                "{} already exists; pass --force to replace it",
                output.display()
            )));
        }
        if !output.is_file() {
            return Err(Failure::refused(format!(
                "{} exists and is not a file",
                output.display()
            )));
        }
    }
    match (from, request.to) {
        (Kind::Grbr, Kind::Match) => replay_to_match(&bytes, output.unwrap_or(Path::new(""))),
        (Kind::Match, Kind::Grbr) => match_to_replay(&bytes, output.unwrap_or(Path::new(""))),
        (Kind::Layout, Kind::Grbr) => {
            layout_to_replay(&bytes, request.seed, output.unwrap_or(Path::new("")))
        }
        (Kind::Match, Kind::Layout) => project(&bytes, request.round, output),
        (Kind::Layout, Kind::Mcfr) => simulate(&request.input, request.seed, output),
        (Kind::Mcfr, Kind::Fight) => written(crate::outcome::fight(&request.input)?, output),
        (Kind::Layout, Kind::Fight) => fight(&request.input, request.seed, output),
        (Kind::Fight, Kind::Mcfr) => simulate_fight(&bytes, output),
        (Kind::Grbr, Kind::Mcfr | Kind::Fight) => Err(Failure::refused(
            "the simulator does not open a replay; the game fights its round with \
             --backend game --round <n>",
        )),
        _ => unreachable!("every pair the kind table names is converted here"),
    }
}

fn write(path: &Path, bytes: impl AsRef<[u8]>) -> Result<(), Failure> {
    std::fs::write(path, bytes)
        .map_err(|error| Failure::failed(format!("cannot write {}: {error}", path.display())))
}

fn report<T: Serialize>(value: &T, text: Option<String>) -> Result<Answer, Failure> {
    Ok(Answer::Report {
        value: serde_json::to_value(value)
            .map_err(|error| Failure::failed(format!("cannot write the result: {error}")))?,
        text,
    })
}

/// What a replay was written as, and how much of it the rules predict.
#[derive(Serialize)]
struct MatchReport {
    schema: &'static str,
    r#match: String,
    map_id: i32,
    seed: i32,
    rounds: usize,
    actions: usize,
    coverage: mechcore_document::coverage::Counts,
    #[serde(skip_serializing_if = "Option::is_none")]
    reinforcement_deal: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    unequal: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    further_unequal: Option<usize>,
}

/// Reads a native replay and writes the match document it records.
fn replay_to_match(grbr: &[u8], destination: &Path) -> Result<Answer, Failure> {
    let economy = mechcore_document::economy::Economy::embedded().map_err(Failure::failed)?;
    // The document is measured as it was written, the way `verify` reads it.
    let mechcore_document::convert::Converted {
        r#match,
        yaml,
        stated,
    } = mechcore_document::convert::document(economy, grbr).map_err(Failure::refused)?;
    let deal = mechcore_document::opening::verify(economy, &stated)
        .and_then(|opening| mechcore_document::reinforcement::verify(economy, &stated, &opening));
    let coverage = mechcore_document::coverage::measure(
        economy,
        &stated,
        deal.as_ref().map_err(String::as_str),
    );
    write(destination, &yaml)?;

    let converted = MatchReport {
        schema: "mechcore.replay-convert-result.v1",
        r#match: destination.display().to_string(),
        map_id: r#match.map_id,
        seed: r#match.seed,
        rounds: r#match.turns.len(),
        actions: r#match
            .turns
            .iter()
            .map(|turn| turn.actions.blue.len() + turn.actions.red.len())
            .sum(),
        coverage: coverage.total,
        reinforcement_deal: deal.err(),
        unequal: coverage
            .unequal
            .iter()
            .take(FAILURES_SHOWN)
            .map(|difference| {
                format!(
                    "round {} {} {}: predicted {} and holds {}",
                    difference.round,
                    difference.side,
                    difference.path,
                    difference.predicted.as_deref().unwrap_or("nothing"),
                    difference.recorded.as_deref().unwrap_or("nothing")
                )
            })
            .collect(),
        further_unequal: coverage
            .unequal
            .len()
            .checked_sub(FAILURES_SHOWN)
            .filter(|further| *further > 0),
    };
    let text = match_text(&converted);
    report(&converted, Some(text))
}

/// The same report, as a person reads it.
fn match_text(report: &MatchReport) -> String {
    use std::fmt::Write;
    let mut text = format!(
        "{} map {} seed {} rounds {} actions {}",
        report.r#match, report.map_id, report.seed, report.rounds, report.actions
    );
    let counts = &report.coverage;
    let _ = write!(
        text,
        "\n  transitions: {} leaves equal, {} unequal, {} unimplemented, {} the fight's",
        counts.equal, counts.unequal, counts.unimplemented, counts.fight
    );
    if let Some(error) = &report.reinforcement_deal {
        let _ = write!(text, "\n    reinforcement deal: {error}");
    }
    for difference in &report.unequal {
        let _ = write!(text, "\n    {difference}");
    }
    if let Some(further) = report.further_unequal {
        let _ = write!(text, "\n    and {further} more");
    }
    text
}

/// What a match was written as.
#[derive(Serialize)]
struct MatchReplayReport {
    schema: &'static str,
    replay: String,
    map_id: i32,
    seed: i32,
    rounds: usize,
}

/// Writes a match back as the replay it converts from.
fn match_to_replay(bytes: &[u8], destination: &Path) -> Result<Answer, Failure> {
    let stated = mechcore_document::opening::stated(bytes)
        .map_err(Failure::refused)?
        .ok_or_else(|| Failure::refused("the document names itself a match and holds none"))?;
    let economy = mechcore_document::economy::Economy::embedded().map_err(Failure::failed)?;
    let replay = mechcore_document::match_replay::match_replay(
        economy,
        &stated,
        mechcore_document::game_build(),
    )
    .map_err(Failure::refused)?;
    write(destination, replay)?;
    let written = MatchReplayReport {
        schema: "mechcore.replay-convert-match-result.v1",
        replay: destination.display().to_string(),
        map_id: stated.map_id,
        seed: stated.seed,
        rounds: stated.turns.len(),
    };
    let text = format!(
        "{} map {} seed {} rounds {}",
        written.replay, written.map_id, written.seed, written.rounds
    );
    report(&written, Some(text))
}

/// What a layout was written as.
#[derive(Serialize)]
struct LayoutReplayReport {
    schema: &'static str,
    replay: String,
    map_id: i32,
    seed: i32,
    round: i32,
}

/// Writes a layout as a replay the game fights: one deployment round whose
/// snapshot is the layout, which `game record` records at that round. The
/// layout is compiled first, so a layout the Training Ground would refuse is
/// refused here too.
fn layout_to_replay(
    bytes: &[u8],
    seed: Option<i32>,
    destination: &Path,
) -> Result<Answer, Failure> {
    let layout: Value = serde_yaml::from_slice(bytes)
        .map_err(|error| Failure::refused(format!("cannot parse the layout: {error}")))?;
    let mut plan = mechcore_document::compile(&layout).map_err(Failure::refused)?;
    plan.seed = seed.or(plan.seed);
    let replay =
        mechcore_document::layout_replay::layout_replay(&plan, mechcore_document::game_build())
            .map_err(Failure::refused)?;
    write(destination, replay)?;
    let written = LayoutReplayReport {
        schema: "mechcore.replay-convert-layout-result.v1",
        replay: destination.display().to_string(),
        map_id: plan
            .map_id
            .unwrap_or(mechcore_document::layout_replay::DEFAULT_MAP_ID),
        seed: plan.seed.unwrap_or_default(),
        round: plan.round,
    };
    let text = format!(
        "{} map {} seed {} round {}",
        written.replay, written.map_id, written.seed, written.round
    );
    report(&written, Some(text))
}

/// Writes the layout a round's fight starts from.
///
/// A round's decisions applied to the position it opened with, and that
/// position projected: it is how a fight is run again without a recording
/// being kept of one, which is why a match keeps none.
fn project(bytes: &[u8], round: Option<i32>, output: Option<&Path>) -> Result<Answer, Failure> {
    let round =
        round.ok_or_else(|| Failure::usage("expected --round <n>: a match holds several"))?;
    let stated = mechcore_document::opening::stated(bytes)
        .map_err(Failure::refused)?
        .ok_or_else(|| Failure::refused("the document names itself a match and holds none"))?;
    let turn = stated
        .turns
        .iter()
        .find(|turn| turn.round == round)
        .ok_or_else(|| {
            Failure::refused(format!(
                "this match holds no round {round}; it holds {} of them",
                stated.turns.len()
            ))
        })?;
    let economy = mechcore_document::economy::Economy::embedded().map_err(Failure::failed)?;
    // What declining this round's offer pays is the deal's to say, and a
    // decision that declined one cannot be applied without it.
    let declined = mechcore_document::opening::verify(economy, &stated)
        .and_then(|opening| mechcore_document::reinforcement::verify(economy, &stated, &opening))
        .map_err(Failure::refused)?
        .rounds
        .iter()
        .find(|dealt| dealt.round == round)
        .map(|dealt| dealt.declined);
    let deployed = |state, actions, red| {
        mechcore_document::transition::deployed(economy, state, actions, red, declined).map_err(
            |unsettled| Failure::refused(format!("round {round} is not settled: {unsettled}")),
        )
    };
    let state = mechcore_document::r#match::State {
        reinforce_offers: None,
        blue: deployed(&turn.state.blue, &turn.actions.blue, false)?,
        red: deployed(&turn.state.red, &turn.actions.red, true)?,
    };
    let layout =
        mechcore_document::project::project(&turn.state, &state, round, stated.map_id, stated.seed)
            .map_err(Failure::refused)?;
    let yaml = mechcore_document::canonical_yaml(layout).map_err(Failure::refused)?;
    match output {
        Some(output) => {
            write(output, &yaml)?;
            report(
                &serde_json::json!({
                    "schema": "mechcore.convert-layout-result.v1",
                    "layout": output.display().to_string(),
                    "round": round,
                }),
                Some(format!("{} round {round}", output.display())),
            )
        }
        None => Ok(Answer::Document(yaml)),
    }
}

/// Simulates one fight from a layout, writing the recording when asked to.
///
/// The recording writer never replaces a file, so a recording that replaces
/// one is written beside it and moved over it once the fight has run: a fight
/// that is refused leaves the old file as it was.
fn simulate(layout: &Path, seed: Option<i32>, output: Option<&Path>) -> Result<Answer, Failure> {
    let simulate = |at: Option<&Path>| {
        mechcore_simulation::simulate_layout(layout, at.map_or(Record::Hash, Record::File), seed)
            .map_err(|error| Failure::refused(error.to_string()))
    };
    let Some(output) = output.filter(|output| output.exists()) else {
        return report(&simulate(output)?, None);
    };
    let unwritable = |error: std::io::Error| {
        Failure::failed(format!("cannot write {}: {error}", output.display()))
    };
    let beside = tempfile::Builder::new()
        .prefix(".mechcore-convert-")
        .tempdir_in(
            output
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new(".")),
        )
        .map_err(unwritable)?;
    let staged = beside.path().join("recording.mcfr");
    let mut result = simulate(Some(&staged))?;
    std::fs::rename(&staged, output).map_err(unwritable)?;
    result.output = Some(output.display().to_string());
    report(&result, None)
}

/// Fights a layout in the simulator and reads the recording it makes, which
/// is `convert --to mcfr` followed by `convert --to fight`: the document's
/// `source` is the simulator, because the simulator made the recording.
fn fight(layout: &Path, seed: Option<i32>, output: Option<&Path>) -> Result<Answer, Failure> {
    written(
        fought(|record| mechcore_simulation::simulate_layout(layout, record, seed))?,
        output,
    )
}

/// A fight document fought again by the simulator, from its projection and
/// the seed it states.
fn simulate_fight(bytes: &[u8], output: Option<&Path>) -> Result<Answer, Failure> {
    let fight = mechcore_document::fight::parse_yaml(bytes).map_err(Failure::refused)?;
    let layout = mechcore_document::canonical_yaml(mechcore_document::fight::project(&fight))
        .map_err(Failure::failed)?;
    let staged = tempfile::Builder::new()
        .suffix(".yaml")
        .tempfile()
        .map_err(|error| Failure::failed(format!("cannot stage the fight's layout: {error}")))?;
    write(staged.path(), layout)?;
    simulate(staged.path(), None, output)
}

/// What the game records of a file `--backend game` names: the same pairs
/// the simulator converts into a recording, and a replay's round, which only
/// the game opens.
///
/// # Errors
///
/// Returns a usage failure for anything but a recording to write, and
/// whatever the recording request refuses.
pub(crate) fn recorded(request: &Request) -> Result<crate::game::Record, Failure> {
    if request.to != Kind::Mcfr {
        return Err(Failure::usage(format!(
            "the game fights a file into a recording: --to mcfr, not --to {}",
            request.to.name()
        )));
    }
    let output = request
        .output
        .clone()
        .ok_or_else(|| Failure::usage("the game writes its recording to a file; name it"))?;
    let (from, _) = Kind::read(&request.input)?;
    if from.conversion(Kind::Mcfr).is_none() {
        return Err(Failure::refused(format!(
            "a {} file is not fought into a recording",
            from.name()
        )));
    }
    // The Adapter opens a replay by the path it is given, from the game's
    // own working directory.
    let input = std::fs::canonicalize(&request.input).map_err(|error| {
        Failure::refused(format!("cannot read {}: {error}", request.input.display()))
    })?;
    crate::game::RecordRequest {
        input: Some(input),
        output: Some(output),
        seed: request.seed,
        round: request.round,
        instrument: request.instrument.clone(),
        ..crate::game::RecordRequest::default()
    }
    .decide()
}

/// A file fought in the game into the fight document its recording states,
/// on a game this command attaches to.
///
/// # Errors
///
/// Returns `unavailable` when no game answers, and what [`game_fight`]
/// refuses.
fn fought_in_game(request: &Request, level: u8, format: Format) -> Outcome {
    let value = crate::game::with_game(level, async |session| game_fight(request, session).await)??;
    crate::cli::emit(&value, format)?;
    Ok(Verdict::Yes)
}

/// A file fought in the game into the fight document its recording states:
/// `convert --to mcfr --backend game` into a recording kept aside, then read
/// as `mcfr` to `fight` reads one. A replay's round is the case only the game
/// fights; a layout or a fight document is fought from its projection.
///
/// # Errors
///
/// Returns a usage failure without a destination, a refusal for one that
/// exists without `force`, what the recording request refuses, and what the
/// recording does not answer.
pub(crate) async fn game_fight(
    request: &Request,
    session: &crate::session::Session,
) -> Result<Value, Failure> {
    let output = request
        .output
        .clone()
        .ok_or_else(|| Failure::usage("the game's fight document goes to a file; name it"))?;
    if output.exists() && !request.force {
        return Err(Failure::refused(format!(
            "{} exists; --force replaces it",
            output.display()
        )));
    }
    let beside = tempfile::Builder::new()
        .tempdir()
        .map_err(|error| Failure::failed(format!("cannot stage the recording: {error}")))?;
    let staged = beside.path().join("recording.mcfr");
    let record = recorded(&Request {
        input: request.input.clone(),
        to: Kind::Mcfr,
        output: Some(staged.clone()),
        seed: request.seed,
        round: request.round,
        force: true,
        backend: request.backend,
        instrument: request.instrument.clone(),
    })?;
    record.run(session, true).await?;
    match written(crate::outcome::fight(&staged)?, Some(&output))? {
        Answer::Report { value, .. } => Ok(value),
        Answer::Document(_) => Err(Failure::failed(
            "a fight written to a file answers a report",
        )),
    }
}

/// The fight document the simulator fights a layout into: `simulate` keeps
/// the recording in memory, as it is told to, and the recording is read as
/// `mcfr` to `fight` reads one on disk.
///
/// `verify` checks a fight document through this same path, so what it
/// compares against is exactly what `convert --to fight` would write.
///
/// # Errors
///
/// Returns a refusal naming what the simulator does not fight, or what the
/// recording does not answer.
pub(crate) fn fought(
    simulate: impl FnOnce(
        Record<'static>,
    )
        -> Result<mechcore_simulation::SimulationResult, mechcore_simulation::Error>,
) -> Result<mechcore_document::Fight, Failure> {
    let simulated =
        simulate(Record::Memory).map_err(|error| Failure::refused(error.to_string()))?;
    let recording = simulated
        .recording
        .ok_or_else(|| Failure::failed("the simulator kept no recording of the fight"))?;
    crate::outcome::read(&recording)?.fight()
}

/// A fight document on standard output, or written to `output` and reported.
fn written(fight: mechcore_document::Fight, output: Option<&Path>) -> Result<Answer, Failure> {
    let source = fight.source.as_str();
    let round = fight.round;
    let yaml = mechcore_document::fight::canonical_yaml(fight).map_err(Failure::failed)?;
    let Some(output) = output else {
        return Ok(Answer::Document(yaml));
    };
    write(output, &yaml)?;
    report(
        &serde_json::json!({
            "schema": "mechcore.convert-fight-result.v1",
            "fight": output.display().to_string(),
            "round": round,
            "source": source,
        }),
        Some(format!(
            "{} round {round} source {source}",
            output.display()
        )),
    )
}
