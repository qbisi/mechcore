//! Declarative execution scripts.
//!
//! A `.mcscript` is a YAML document describing one bounded run: an optional
//! game acquisition, named variables, and an ordered list of steps. Steps bind
//! results to names so later steps can consume them, which is what separates
//! this from a flat command list.
//!
//! A script that omits `game:` is offline and may not use a native operation.
//! That is a static rule, checked before anything runs, so `--check` answers
//! "does this need the game?" without touching it.

use crate::acquire::Mode;
use crate::fight;
use crate::session::Session;
use mechcore_protocol::{
    DEFAULT_LEVEL, DEFAULT_WATCH_MATCH_TIMEOUT_SECONDS, DEFAULT_WATCH_SCENE_WAIT_SECONDS,
    MAX_LEVEL, RecordBattleInstrumentation,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Operations that require an acquired game.
const NATIVE: &[&str] = &[
    "game.status",
    "game.start_test",
    "game.apply_layout",
    "game.record_battle",
    "game.record_replay_round",
    "game.record_watch_replay",
    "game.toggle_fight",
    "game.speed_up",
    "game.quit_match",
    "game.quit_game",
];

/// Operations that run without a game.
const OFFLINE: &[&str] = &[
    "let",
    "fight.compare",
    "fight.stats",
    "fight.buildings",
    "fight.outcome",
    "fight.run",
];

/// Step keys that are structure rather than an operation name.
const RESERVED: &[&str] = &["expect", "steps", "where"];

pub(crate) fn run(arguments: impl Iterator<Item = String>) -> Result<bool, String> {
    let options = Options::parse(arguments)?;
    let text = std::fs::read_to_string(&options.script)
        .map_err(|error| format!("cannot read {}: {error}", options.script.display()))?;
    let script = Script::parse(&text)?;
    script.check()?;
    if options.check_only {
        println!(
            "{}",
            json!({
                "schema": "mechcore.mcscript-check.v1",
                "script": options.script.display().to_string(),
                "game": script.game.map(Mode::as_str),
                "level": script.game.map(|_| script.level),
                "steps": script.steps.len(),
                "valid": true,
            })
        );
        return Ok(true);
    }
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("cannot create async runtime: {error}"))?
        .block_on(execute(
            script,
            base(),
            if options.force {
                Overwrite::Always
            } else {
                Overwrite::Ask
            },
        ))
}

/// Relative paths resolve against the working directory, matching every other
/// subcommand, so a script reads the same as the commands it replaces.
fn base() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

struct Options {
    script: PathBuf,
    check_only: bool,
    force: bool,
}

/// What a recording step does when its destination already exists.
///
/// Overwriting is an invocation decision, not something a script declares: the
/// same script is re-run to replace its outputs and run once to produce them.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Overwrite {
    /// Ask, when there is a terminal to ask on. Refuse otherwise.
    Ask,
    /// `--force`: replace without asking.
    Always,
}

impl Options {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut script = None;
        let mut check_only = false;
        let mut force = false;
        for argument in arguments {
            match argument.as_str() {
                "--check" => check_only = true,
                "-f" | "--force" => force = true,
                other if other.starts_with("--") => {
                    return Err(format!("unexpected option {other}"));
                }
                other if script.is_none() => script = Some(PathBuf::from(other)),
                other => return Err(format!("unexpected argument {other}")),
            }
        }
        Ok(Self {
            script: script.ok_or("usage: mechcore run <script.mcscript> [--check] [--force]")?,
            check_only,
            force,
        })
    }
}

#[cfg_attr(test, derive(Debug))]
struct Script {
    game: Option<Mode>,
    level: u8,
    vars: Vec<(String, Value)>,
    steps: Vec<Step>,
}

#[cfg_attr(test, derive(Debug))]
enum Step {
    Call(Call),
    ForEach(ForEach),
}

#[cfg_attr(test, derive(Debug))]
struct Call {
    operation: String,
    arguments: Value,
    expect: Option<Map<String, Value>>,
}

/// Repeat a body once per item of a list, binding the item to a name.
///
/// The body is the unit of failure: a case that fails stops the run, because a
/// step that depends on a failed predecessor cannot produce a meaningful
/// result.
#[cfg_attr(test, derive(Debug))]
struct ForEach {
    binding: String,
    source: Value,
    filter: Option<Map<String, Value>>,
    body: Vec<Step>,
}

impl Script {
    fn parse(text: &str) -> Result<Self, String> {
        let document: Value =
            serde_yaml::from_str(text).map_err(|error| format!("cannot parse script: {error}"))?;
        let document = document
            .as_object()
            .ok_or("script must be a mapping with optional game, vars and steps")?;
        for key in document.keys() {
            if !matches!(key.as_str(), "game" | "level" | "vars" | "steps") {
                return Err(format!("unknown top-level key {key}"));
            }
        }

        let game = match document.get("game") {
            None => None,
            Some(Value::String(value)) if value == "launch" => Some(Mode::Launch),
            Some(Value::String(value)) if value == "attach" => Some(Mode::Attach),
            Some(other) => {
                return Err(format!("game must be launch or attach, got {other}"));
            }
        };

        // The level orders this run against the other clients of one game: a
        // higher one takes the game from a lower one. It is only meaningful
        // for a script that acquires a game at all.
        let level = match document.get("level") {
            None => DEFAULT_LEVEL,
            Some(Value::Number(value)) => {
                let level = value
                    .as_u64()
                    .filter(|level| *level <= u64::from(MAX_LEVEL))
                    .ok_or_else(|| format!("level must be 0..={MAX_LEVEL}, got {value}"))?;
                u8::try_from(level).unwrap_or(MAX_LEVEL)
            }
            Some(other) => return Err(format!("level must be 0..={MAX_LEVEL}, got {other}")),
        };
        if document.contains_key("level") && game.is_none() {
            return Err(
                "level orders clients of one game, but the script declares no `game:` key".into(),
            );
        }

        let mut vars = Vec::new();
        if let Some(declared) = document.get("vars") {
            let declared = declared
                .as_object()
                .ok_or("vars must be a mapping of name to value")?;
            for (name, value) in declared {
                vars.push((name.clone(), value.clone()));
            }
        }

        let steps = document
            .get("steps")
            .ok_or("script must declare steps")?
            .as_array()
            .ok_or("steps must be a list")?
            .iter()
            .map(Step::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if steps.is_empty() {
            return Err("script must declare at least one step".into());
        }
        Ok(Self {
            game,
            level,
            vars,
            steps,
        })
    }

    /// Reject a script before it runs, without probing or launching anything.
    fn check(&self) -> Result<(), String> {
        for operation in self.steps.iter().flat_map(Step::operations) {
            if !NATIVE.contains(&operation) && !OFFLINE.contains(&operation) {
                return Err(format!("unknown operation {operation}"));
            }
            if self.game.is_none() && NATIVE.contains(&operation) {
                return Err(format!(
                    "{operation} needs a game, but the script declares no `game:` key"
                ));
            }
        }
        Ok(())
    }
}

impl Step {
    fn parse(value: &Value) -> Result<Self, String> {
        let mapping = value
            .as_object()
            .ok_or("each step must be a mapping with one operation key")?;
        let operations: Vec<&String> = mapping
            .keys()
            .filter(|key| !RESERVED.contains(&key.as_str()))
            .collect();
        let [operation] = operations.as_slice() else {
            return Err(format!(
                "each step needs exactly one operation key, found {operations:?}"
            ));
        };
        let expect = match mapping.get("expect") {
            None => None,
            Some(Value::Object(fields)) => Some(fields.clone()),
            Some(other) => return Err(format!("expect must be a mapping, got {other}")),
        };
        let arguments = mapping[operation.as_str()].clone();

        if operation.as_str() != "foreach" {
            for key in ["steps", "where"] {
                if mapping.contains_key(key) {
                    return Err(format!("{key} belongs to foreach, not to {operation}"));
                }
            }
            return Ok(Self::Call(Call {
                operation: (*operation).clone(),
                arguments,
                expect,
            }));
        }

        if expect.is_some() {
            return Err("expect asserts on one operation's result; put it on a body step".into());
        }
        let binding = {
            let fields = arguments
                .as_object()
                .ok_or("foreach takes one mapping of binding name to list")?;
            let [name] = fields.keys().collect::<Vec<_>>()[..] else {
                return Err("foreach binds exactly one name".into());
            };
            name.clone()
        };
        let source = arguments[binding.as_str()].clone();
        let filter = match mapping.get("where") {
            None => None,
            Some(Value::Object(fields)) => Some(fields.clone()),
            Some(other) => return Err(format!("where must be a mapping, got {other}")),
        };
        let body = Self::body(mapping, "foreach")?;
        Ok(Self::ForEach(ForEach {
            binding,
            source,
            filter,
            body,
        }))
    }

    /// A loop body: at least one step, and no loop of its own.
    fn body(mapping: &Map<String, Value>, loop_: &str) -> Result<Vec<Self>, String> {
        let body = mapping
            .get("steps")
            .ok_or_else(|| format!("{loop_} needs steps"))?
            .as_array()
            .ok_or_else(|| format!("{loop_} steps must be a list"))?
            .iter()
            .map(Self::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if body.is_empty() {
            return Err(format!("{loop_} needs at least one body step"));
        }
        if body.iter().any(|step| matches!(step, Self::ForEach(_))) {
            return Err(format!("a loop inside {loop_} is not supported"));
        }
        Ok(body)
    }

    /// Every operation this step can reach, loop bodies included, so the
    /// offline rule cannot be evaded by hiding a native call in a loop.
    fn operations(&self) -> Vec<&str> {
        match self {
            Self::Call(call) => vec![call.operation.as_str()],
            Self::ForEach(loop_) => loop_.body.iter().flat_map(Self::operations).collect(),
        }
    }
}

fn is_reference_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_' || character == '.'
}

/// The reference when `text` is nothing but one, in either spelling.
fn whole_reference(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('$')?;
    if let Some(inner) = rest.strip_prefix('{') {
        return inner.strip_suffix('}').filter(|inner| !inner.is_empty());
    }
    (!rest.is_empty() && rest.chars().all(is_reference_char)).then_some(rest)
}

/// Variable scope; values are whatever a step bound or a var declared.
struct Scope {
    values: BTreeMap<String, Value>,
    base: PathBuf,
    overwrite: Overwrite,
}

impl Scope {
    fn resolve(&self, value: &Value) -> Result<Value, String> {
        match value {
            Value::String(text) => self.expand(text),
            Value::Array(items) => items
                .iter()
                .map(|item| self.resolve(item))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            Value::Object(fields) => fields
                .iter()
                .map(|(key, item)| self.resolve(item).map(|item| (key.clone(), item)))
                .collect::<Result<Map<_, _>, _>>()
                .map(Value::Object),
            other => Ok(other.clone()),
        }
    }

    /// A string that is exactly one reference keeps the referenced type;
    /// a reference inside longer text is stringified and spliced.
    ///
    /// `${name.field}` delimits explicitly, which a bare `$name.field` cannot
    /// do when the value is followed by more path: `$out/${case.name}.mcfr`
    /// would otherwise read `name.mcfr` as a field lookup.
    fn expand(&self, text: &str) -> Result<Value, String> {
        if let Some(reference) = whole_reference(text) {
            return self.lookup(reference);
        }
        let mut out = String::new();
        let mut rest = text;
        while let Some(index) = rest.find('$') {
            out.push_str(&rest[..index]);
            let tail = &rest[index + 1..];
            let (reference, remainder) = if let Some(braced) = tail.strip_prefix('{') {
                let end = braced
                    .find('}')
                    .ok_or_else(|| format!("unterminated ${{ in {text}"))?;
                (&braced[..end], &braced[end + 1..])
            } else {
                let length = tail
                    .find(|character: char| !is_reference_char(character))
                    .unwrap_or(tail.len());
                if length == 0 {
                    out.push('$');
                    rest = tail;
                    continue;
                }
                tail.split_at(length)
            };
            let value = self.lookup(reference.trim_end_matches('.'))?;
            match value {
                Value::String(text) => out.push_str(&text),
                other => out.push_str(&other.to_string()),
            }
            rest = remainder;
        }
        out.push_str(rest);
        Ok(Value::String(out))
    }

    fn lookup(&self, reference: &str) -> Result<Value, String> {
        let mut parts = reference.split('.');
        let root = parts.next().unwrap_or_default();
        let mut current = self
            .values
            .get(root)
            .ok_or_else(|| format!("undefined variable ${root}"))?
            .clone();
        for field in parts {
            current = current
                .get(field)
                .ok_or_else(|| format!("${reference}: no field {field}"))?
                .clone();
        }
        Ok(current)
    }

    fn path(&self, value: &Value, what: &str) -> Result<PathBuf, String> {
        let text = value
            .as_str()
            .ok_or_else(|| format!("{what} must be a string path, got {value}"))?;
        let path = Path::new(text);
        Ok(if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base.join(path)
        })
    }
}

async fn execute(script: Script, base: PathBuf, overwrite: Overwrite) -> Result<bool, String> {
    let session = Session::new();
    let monitor = tokio::spawn(Session::monitor_status(session.clone()));
    let mut ownership = None;
    if let Some(mode) = script.game {
        match session.acquire(mode, script.level).await {
            Ok(owned) => ownership = Some(owned),
            Err(failure) => {
                monitor.abort();
                return Err(failure);
            }
        }
    }

    let mut scope = Scope {
        values: BTreeMap::new(),
        base,
        overwrite,
    };
    let outcome = run_steps(&script, &mut scope, &session).await;
    let closed = session.release(ownership).await;
    monitor.abort();
    // Losing the game to a higher claim is not a failed script. The run ends
    // where it was interrupted, says so, and leaves the game to its claimant.
    if session.was_evicted() {
        println!("{}", json!({"operation": "evicted", "completed": false}));
        closed?;
        return Ok(true);
    }
    outcome?;
    closed?;
    Ok(true)
}

async fn run_steps(
    script: &Script,
    scope: &mut Scope,
    session: &Arc<Session>,
) -> Result<(), String> {
    for (name, value) in &script.vars {
        let resolved = scope.resolve(value)?;
        scope.values.insert(name.clone(), resolved);
    }
    run_body(&script.steps, scope, session, None).await
}

/// Execute one list of steps. `iteration` labels output produced inside a loop.
async fn run_body(
    steps: &[Step],
    scope: &mut Scope,
    session: &Arc<Session>,
    iteration: Option<usize>,
) -> Result<(), String> {
    for (index, step) in steps.iter().enumerate() {
        let position = index + 1;
        match step {
            Step::Call(call) => {
                run_call(call, position, scope, session, iteration).await?;
            }
            Step::ForEach(loop_) => {
                run_loop(loop_, position, scope, session).await?;
            }
        }
    }
    Ok(())
}

async fn run_call(
    call: &Call,
    position: usize,
    scope: &mut Scope,
    session: &Arc<Session>,
    iteration: Option<usize>,
) -> Result<(), String> {
    let label = |error: String| match iteration {
        Some(index) => format!(
            "step {position} ({}) in iteration {index}: {error}",
            call.operation
        ),
        None => format!("step {position} ({}): {error}", call.operation),
    };
    let arguments = scope.resolve(&call.arguments).map_err(label)?;
    let started = std::time::Instant::now();
    let result = perform(call, &arguments, scope, session)
        .await
        .map_err(label)?;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    if let Some(expect) = &call.expect {
        let expect = scope
            .resolve(&Value::Object(expect.clone()))
            .map_err(label)?;
        let expect = expect
            .as_object()
            .ok_or_else(|| label("expect did not resolve to a mapping".into()))?;
        check_expectations(expect, &result).map_err(label)?;
    }
    let mut line = json!({
        "step": position,
        "operation": call.operation,
        "elapsed_ms": elapsed_ms,
        "result": result,
    });
    if let Some(index) = iteration {
        line["iteration"] = json!(index);
    }
    println!("{line}");
    Ok(())
}

async fn run_loop(
    loop_: &ForEach,
    position: usize,
    scope: &mut Scope,
    session: &Arc<Session>,
) -> Result<(), String> {
    let label = |error: String| format!("step {position} (foreach): {error}");
    let source = scope.resolve(&loop_.source).map_err(&label)?;
    let items = source
        .as_array()
        .ok_or_else(|| label(format!("foreach source is not a list: {source}")))?;
    let started = std::time::Instant::now();
    let mut executed = 0;
    for item in items {
        if let Some(filter) = &loop_.filter
            && check_expectations(filter, item).is_err()
        {
            continue;
        }
        // Each iteration gets its own bindings: the loop variable and anything
        // the body binds must not leak into the next case or past the loop.
        let outer = scope.values.clone();
        scope.values.insert(loop_.binding.clone(), item.clone());
        let outcome = Box::pin(run_body(&loop_.body, scope, session, Some(executed))).await;
        scope.values = outer;
        outcome?;
        executed += 1;
    }
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    println!(
        "{}",
        json!({
            "step": position,
            "operation": "foreach",
            "elapsed_ms": elapsed_ms,
            "result": {"iterations": executed, "considered": items.len()},
        })
    );
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn perform(
    step: &Call,
    arguments: &Value,
    scope: &mut Scope,
    session: &Arc<Session>,
) -> Result<Value, String> {
    match step.operation.as_str() {
        "let" => {
            let bindings = arguments
                .as_object()
                .ok_or("let takes a mapping of name to value")?;
            let mut bound = Map::new();
            for (name, value) in bindings {
                let value = evaluate(value, scope)?;
                bound.insert(name.clone(), value.clone());
                scope.values.insert(name.clone(), value);
            }
            Ok(Value::Object(bound))
        }
        "fight.compare" => {
            let fields = arguments
                .as_object()
                .ok_or("fight.compare takes left and right paths")?;
            let left = scope.path(
                fields.get("left").ok_or("fight.compare needs left")?,
                "fight.compare left",
            )?;
            let right = scope.path(
                fields.get("right").ok_or("fight.compare needs right")?,
                "fight.compare right",
            )?;
            if let Some(key) = fields
                .keys()
                .find(|key| !matches!(key.as_str(), "left" | "right" | "fields" | "tick"))
            {
                return Err(format!(
                    "fight.compare accepts only left, right, fields and tick, got {key}"
                ));
            }
            // A selection is a list of field groups, or one group by itself.
            let selection = match fields
                .get("fields")
                .map(|value| scope.resolve(value))
                .transpose()?
            {
                None => crate::difference::Selection::default(),
                Some(Value::String(group)) => crate::difference::Selection::of([group]),
                Some(Value::Array(groups)) => crate::difference::Selection::of(
                    groups
                        .iter()
                        .map(|group| {
                            group.as_str().map(str::to_owned).ok_or_else(|| {
                                format!("fight.compare fields names groups, got {group}")
                            })
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                Some(other) => {
                    return Err(format!(
                        "fight.compare fields is a group or a list of groups, got {other}"
                    ));
                }
            };
            let tick = optional_u64(fields.get("tick"), "fight.compare tick")?
                .map(|tick| u32::try_from(tick).map_err(|_| "fight.compare tick is too large"))
                .transpose()?;
            let (_, report) = fight::compare(&left, &right, &selection, tick)?;
            Ok(report)
        }
        "fight.run" => simulate(arguments, scope).await,
        "fight.outcome" => {
            let recording = scope.path(
                arguments
                    .as_object()
                    .and_then(|fields| fields.get("recording"))
                    .ok_or("fight.outcome takes a recording")?,
                "fight.outcome recording",
            )?;
            // A fight nothing can settle is still a fight worth reading: the
            // survivors and their life are the measurement a capture is taken
            // for, and `unresolved` says what the reading does not cover.
            let outcome =
                crate::outcome::read(&recording).map_err(|failure| failure.reason().to_owned())?;
            serde_json::to_value(&outcome).map_err(|error| error.to_string())
        }
        "fight.buildings" => {
            let fields = arguments.as_object();
            let recording = scope.path(
                fields
                    .and_then(|fields| fields.get("recording"))
                    .ok_or("fight.buildings takes a recording")?,
                "fight.buildings recording",
            )?;
            let tick = match fields.and_then(|fields| fields.get("tick")) {
                None => None,
                Some(value) => Some(
                    scope
                        .resolve(value)?
                        .as_u64()
                        .and_then(|tick| u32::try_from(tick).ok())
                        .ok_or("fight.buildings tick must be a tick the recording holds")?,
                ),
            };
            let standing = crate::buildings::read(&recording, tick)
                .map_err(|failure| failure.reason().to_owned())?;
            serde_json::to_value(&standing).map_err(|error| error.to_string())
        }
        "fight.stats" => {
            let fields = arguments.as_object();
            let recording = scope.path(
                fields
                    .and_then(|fields| fields.get("recording"))
                    .ok_or("fight.stats takes a recording")?,
                "fight.stats recording",
            )?;
            let tick = match fields.and_then(|fields| fields.get("tick")) {
                None => None,
                Some(value) => Some(
                    scope
                        .resolve(value)?
                        .as_u64()
                        .and_then(|tick| u32::try_from(tick).ok())
                        .ok_or("fight.stats tick must be a tick the recording holds")?,
                ),
            };
            let written = crate::stats::read(&recording, tick)
                .map_err(|failure| failure.reason().to_owned())?;
            serde_json::to_value(&written).map_err(|error| error.to_string())
        }
        "game.status" => Ok(session.current_status()),
        "game.start_test" => {
            let seed = arguments
                .as_object()
                .and_then(|fields| fields.get("seed"))
                .and_then(Value::as_i64)
                .map(i32::try_from)
                .transpose()
                .map_err(|_| "seed must fit in a signed 32-bit integer".to_string())?;
            let map_id = arguments
                .get("map_id")
                .filter(|value| !value.is_null())
                .map(|value| {
                    value
                        .as_i64()
                        .and_then(|id| i32::try_from(id).ok())
                        .ok_or_else(|| "map_id must be a signed 32-bit integer".to_owned())
                })
                .transpose()?;
            session.start_test(seed, map_id).await
        }
        "game.apply_layout" => {
            let (layout, seed) = split_layout_arguments(arguments)?;
            session.apply_layout(layout, seed).await
        }
        "game.record_battle" => {
            let fields = arguments
                .as_object()
                .ok_or("record_battle takes a mapping")?;
            let output = scope.path(
                fields.get("output").ok_or("record_battle needs output")?,
                "record_battle output",
            )?;
            let video = fields
                .get("video_output")
                .map(|value| scope.path(value, "record_battle video_output"))
                .transpose()?;
            let speed_up = optional_flag(fields.get("speed_up"), "game.record_battle speed_up")?;
            if fields.contains_key("force") {
                return Err("force is not a script field; pass --force to mechcore run".to_string());
            }
            let instrumentation = instrumentation(fields.get("instrumentation"), scope)?;
            let mut destinations = vec![output.as_path()];
            destinations.extend(video.as_deref());
            destinations.extend(
                instrumentation
                    .as_ref()
                    .map(|sidecar| sidecar.output.as_path()),
            );
            let force = confirm_overwrite(scope, &destinations).await?;
            session
                .record_battle(
                    output.clone(),
                    video.clone(),
                    speed_up,
                    force,
                    instrumentation,
                )
                .await
                .map_err(|value| value.to_string())
        }
        "game.record_replay_round" => {
            let fields = arguments
                .as_object()
                .ok_or("record_replay_round takes a mapping")?;
            let grbr = scope.path(
                fields.get("grbr").ok_or("record_replay_round needs grbr")?,
                "record_replay_round grbr",
            )?;
            let output = scope.path(
                fields
                    .get("output")
                    .ok_or("record_replay_round needs output")?,
                "record_replay_round output",
            )?;
            let round = fields
                .get("round")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or("record_replay_round needs an integer round")?;
            let speed_up =
                optional_flag(fields.get("speed_up"), "game.record_replay_round speed_up")?;
            if fields.contains_key("force") {
                return Err("force is not a script field; pass --force to mechcore run".to_string());
            }
            let instrumentation = instrumentation(fields.get("instrumentation"), scope)?;
            let mut destinations = vec![output.as_path()];
            destinations.extend(
                instrumentation
                    .as_ref()
                    .map(|sidecar| sidecar.output.as_path()),
            );
            let force = confirm_overwrite(scope, &destinations).await?;
            session
                .record_replay_round(
                    grbr,
                    round,
                    output.clone(),
                    speed_up,
                    force,
                    instrumentation,
                )
                .await
        }
        "game.record_watch_replay" => {
            let fields = arguments
                .as_object()
                .ok_or("record_watch_replay takes a mapping")?;
            for key in fields.keys() {
                if !matches!(
                    key.as_str(),
                    "output_dir" | "wait_for_scene_seconds" | "match_timeout_seconds"
                ) {
                    return Err(format!(
                        "record_watch_replay accepts only output_dir, wait_for_scene_seconds and \
                         match_timeout_seconds, got {key}"
                    ));
                }
            }
            let output_dir = fields
                .get("output_dir")
                .map(|value| scope.path(value, "record_watch_replay output_dir"))
                .transpose()?;
            let wait_for_scene_seconds = optional_u64(
                fields.get("wait_for_scene_seconds"),
                "record_watch_replay wait_for_scene_seconds",
            )?
            .unwrap_or(DEFAULT_WATCH_SCENE_WAIT_SECONDS);
            let match_timeout_seconds = optional_u64(
                fields.get("match_timeout_seconds"),
                "record_watch_replay match_timeout_seconds",
            )?
            .unwrap_or(DEFAULT_WATCH_MATCH_TIMEOUT_SECONDS);
            session
                .record_watch_replay(output_dir, wait_for_scene_seconds, match_timeout_seconds)
                .await
        }
        "game.toggle_fight" => session.toggle_fight().await,
        "game.speed_up" => session.speed_up().await,
        "game.quit_match" => session.quit_match().await,
        "game.quit_game" => session.quit_game().await,
        other => Err(format!("unknown operation {other}")),
    }
}

/// Decide whether a recording may replace the destinations it would publish.
///
/// `--force` answers yes without asking. Otherwise an existing destination is a
/// question for whoever started the run, so it is asked once, naming every file
/// at stake. With no terminal to ask on there is nobody to answer, and the
/// answer is no: the session then refuses and says which file blocked it.
async fn confirm_overwrite(scope: &Scope, destinations: &[&Path]) -> Result<bool, String> {
    if scope.overwrite == Overwrite::Always {
        return Ok(true);
    }
    let existing = destinations
        .iter()
        .filter(|path| path.exists())
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    if existing.is_empty() || !std::io::stdin().is_terminal() {
        return Ok(false);
    }
    let mut out = tokio::io::stdout();
    out.write_all(format!("replace {}? [y/N] ", existing.join(", ")).as_bytes())
        .await
        .map_err(|error| format!("cannot prompt: {error}"))?;
    out.flush()
        .await
        .map_err(|error| format!("cannot prompt: {error}"))?;
    let mut answer = String::new();
    BufReader::new(tokio::io::stdin())
        .read_line(&mut answer)
        .await
        .map_err(|error| format!("cannot read the answer: {error}"))?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

fn instrumentation(
    value: Option<&Value>,
    scope: &Scope,
) -> Result<Option<RecordBattleInstrumentation>, String> {
    let Some(value) = value else { return Ok(None) };
    let mut parameters: RecordBattleInstrumentation = serde_json::from_value(value.clone())
        .map_err(|error| format!("invalid instrumentation: {error}"))?;
    parameters.output = scope.path(&json!(parameters.output), "instrumentation output")?;
    Ok(Some(parameters))
}

/// Split `apply_layout` arguments into the layout and an optional seed override.
///
/// A layout's top-level keys are closed to `kind`, `map_id`, `seed`, `round` and
/// `sides`, so a `layout` key can only be the wrapper form and never a layout
/// itself.
fn split_layout_arguments(arguments: &Value) -> Result<(Value, Option<i32>), String> {
    let Some(wrapped) = arguments.get("layout") else {
        return Ok((arguments.clone(), None));
    };
    let seed = match arguments.get("seed") {
        None => None,
        Some(value) => Some(
            value
                .as_i64()
                .and_then(|seed| i32::try_from(seed).ok())
                .ok_or("apply_layout seed must be a signed 32-bit integer")?,
        ),
    };
    for key in arguments
        .as_object()
        .map(|fields| fields.keys())
        .into_iter()
        .flatten()
    {
        if !matches!(key.as_str(), "layout" | "seed") {
            return Err(format!(
                "apply_layout accepts only layout and seed, got {key}"
            ));
        }
    }
    Ok((wrapped.clone(), seed))
}

/// Read an optional boolean field, refusing anything that is not a boolean.
fn optional_flag(value: Option<&Value>, what: &str) -> Result<Option<bool>, String> {
    match value {
        None => Ok(None),
        Some(Value::Bool(flag)) => Ok(Some(*flag)),
        Some(other) => Err(format!("{what} must be true or false, got {other}")),
    }
}

/// Read an optional unsigned integer field without accepting floats or signs.
fn optional_u64(value: Option<&Value>, what: &str) -> Result<Option<u64>, String> {
    match value {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| format!("{what} must be an unsigned integer, got {value}")),
    }
}

/// Evaluate a `let` right-hand side: a built-in call, or a plain value.
fn evaluate(value: &Value, scope: &Scope) -> Result<Value, String> {
    let resolved = scope.resolve(value)?;
    let Some(text) = resolved.as_str() else {
        return Ok(resolved);
    };
    let Some((name, rest)) = text.split_once('(') else {
        return Ok(resolved);
    };
    let Some(argument) = rest.strip_suffix(')') else {
        return Ok(resolved);
    };
    match name.trim() {
        "read_yaml" => {
            let path = scope.path(&Value::String(argument.trim().into()), "read_yaml")?;
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            serde_yaml::from_str(&text)
                .map_err(|error| format!("cannot parse {}: {error}", path.display()))
        }
        "embedded_layout" => {
            let path = scope.path(&Value::String(argument.trim().into()), "embedded_layout")?;
            let reader = mechcore_mcfr::McfrReader::open(&path)
                .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
            serde_yaml::from_str(reader.layout_yaml())
                .map_err(|error| format!("cannot parse the embedded layout: {error}"))
        }
        "range" => {
            let count = argument
                .trim()
                .parse::<u64>()
                .map_err(|error| format!("range count is not an unsigned integer: {error}"))?;
            if count > 10_000 {
                return Err(format!("range count {count} exceeds 10000"));
            }
            Ok(Value::Array((0..count).map(Value::from).collect()))
        }
        other => Err(format!("unknown function {other}")),
    }
}

/// Runs the deterministic simulator without a game, returning the same result
/// object `mechcore fight run` prints so `expect` can assert any of its fields.
///
/// An existing `output` is a destination like a recording's: the run replaces
/// it only when `confirm_overwrite` says so, and the simulator, which refuses
/// to overwrite, is handed a path that no longer exists.
async fn simulate(arguments: &Value, scope: &Scope) -> Result<Value, String> {
    let fields = arguments
        .as_object()
        .ok_or("fight.run takes a mapping with layout and optional seed and output")?;
    for key in fields.keys() {
        if !matches!(key.as_str(), "layout" | "seed" | "output") {
            return Err(format!(
                "fight.run accepts only layout, seed and output, got {key}"
            ));
        }
    }
    let layout = scope.path(
        fields.get("layout").ok_or("fight.run needs layout")?,
        "fight.run layout",
    )?;
    let seed = match fields.get("seed") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_i64()
                .and_then(|seed| i32::try_from(seed).ok())
                .ok_or("fight.run seed must be a signed 32-bit integer")?,
        ),
    };
    let output = match fields.get("output") {
        None | Some(Value::Null) => None,
        Some(value) => Some(scope.path(value, "fight.run output")?),
    };
    if let Some(output) = &output {
        let force = confirm_overwrite(scope, &[output.as_path()]).await?;
        crate::session::remove_existing_outputs(&[(output.as_path(), "fight.run output")], force)?;
    }
    let result = mechcore_simulation::simulate_layout(&layout, output.as_deref(), seed)
        .map_err(|error| format!("{}: {error}", layout.display()))?;
    serde_json::to_value(result)
        .map_err(|error| format!("cannot serialize the simulation result: {error}"))
}

/// Every declared field must be present and equal; extra result fields are fine.
fn check_expectations(expect: &Map<String, Value>, result: &Value) -> Result<(), String> {
    for (key, wanted) in expect {
        let actual = field_at(result, key)
            .ok_or_else(|| format!("expected {key}={wanted}, but the result has no {key}"))?;
        if actual != wanted {
            return Err(format!("expected {key}={wanted}, got {actual}"));
        }
    }
    Ok(())
}

/// Follow a dotted key into a result. The interesting values are nested:
/// a recording reports `operation.tick_count`, not `tick_count`, and a
/// measurement is commonly one entry of a list, as
/// `sides.red.survivors.0.life` is.
fn field_at<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(value, |current, part| {
        match (current, part.parse::<usize>()) {
            (Value::Array(items), Ok(at)) => items.get(at),
            _ => current.get(part),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_instrumentation_resolves_paths_and_preserves_scope() {
        let scope = scope_with(&[]);
        assert!(instrumentation(None, &scope).unwrap().is_none());
        let value = json!({"output":"local.h5", "profile":"target_refs_rvo_v1",
            "rvo_scope":{"start_tick":4,"end_tick":12,"unit_ids":[72,117]}});
        let parsed = instrumentation(Some(&value), &scope).unwrap().unwrap();
        assert_eq!(parsed.output, PathBuf::from("/base/local.h5"));
        assert_eq!(parsed.rvo_scope.unwrap().unit_ids, vec![72, 117]);
        let mut invalid = value;
        invalid["unknown"] = json!(true);
        assert!(instrumentation(Some(&invalid), &scope).is_err());
    }

    #[tokio::test]
    async fn fight_run_replaces_an_existing_output_under_force() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("simulated.mcfr");
        std::fs::write(&output, b"existing").unwrap();
        let scope = Scope {
            overwrite: Overwrite::Always,
            values: BTreeMap::new(),
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        };
        let arguments = json!({
            "layout": "tests/regression/crawlers-vs-crawlers.yaml",
            "seed": 1_787_778_788,
            "output": output.display().to_string(),
        });

        let result = simulate(&arguments, &scope).await.unwrap();
        assert_eq!(result["steps"], json!(351), "{result}");
        mechcore_mcfr::McfrReader::open(&output).expect("the output is the new recording");
    }

    fn scope_with(values: &[(&str, Value)]) -> Scope {
        Scope {
            overwrite: Overwrite::Ask,
            values: values
                .iter()
                .map(|(name, value)| ((*name).to_string(), value.clone()))
                .collect(),
            base: PathBuf::from("/base"),
        }
    }

    #[test]
    fn a_whole_string_reference_keeps_the_referenced_type() {
        let scope = scope_with(&[("layout", json!({"seed": 31_103_914}))]);
        assert_eq!(
            scope.expand("$layout.seed").unwrap(),
            json!(31_103_914),
            "a seed must stay a number, not become a string"
        );
    }

    #[test]
    fn an_embedded_reference_splices_into_the_surrounding_text() {
        let scope = scope_with(&[("out", json!("work/x"))]);
        assert_eq!(
            scope.expand("$out/replay.mcfr").unwrap(),
            json!("work/x/replay.mcfr")
        );
    }

    #[test]
    fn undefined_references_are_reported_by_name() {
        let scope = scope_with(&[]);
        assert!(scope.expand("$missing").unwrap_err().contains("$missing"));
    }

    #[test]
    fn run_accepts_force_in_either_spelling_and_defaults_to_asking() {
        let plain = Options::parse(["a.mcscript".to_string()].into_iter()).unwrap();
        assert!(!plain.force);
        assert!(!plain.check_only);
        for spelling in ["-f", "--force"] {
            let forced =
                Options::parse(["a.mcscript".to_string(), spelling.to_string()].into_iter())
                    .unwrap();
            assert!(forced.force, "{spelling}");
        }
        assert!(
            Options::parse(["a.mcscript".to_string(), "--clobber".to_string()].into_iter())
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_script_that_still_declares_force_is_told_where_it_moved() {
        let session = Session::new();
        let mut scope = scope_with(&[]);
        let call = Call {
            operation: "game.record_battle".into(),
            arguments: json!({"output": "/tmp/a.mcfr", "force": true}),
            expect: None,
        };
        let error = perform(&call, &call.arguments.clone(), &mut scope, &session)
            .await
            .unwrap_err();
        assert!(error.contains("--force"), "{error}");
    }

    #[test]
    fn offline_scripts_may_not_use_native_operations() {
        let script = Script::parse("steps:\n  - game.start_test: {}\n").unwrap();
        let error = script.check().unwrap_err();
        assert!(error.contains("game.start_test"), "{error}");
        assert!(error.contains("game:"), "{error}");
    }

    #[test]
    fn offline_scripts_accept_offline_operations() {
        let script =
            Script::parse("steps:\n  - fight.compare: {left: a.mcfr, right: b.mcfr}\n").unwrap();
        assert!(script.check().is_ok());
        assert!(script.game.is_none());

        let script = Script::parse("steps:\n  - fight.run: {layout: a.yaml, seed: 7}\n").unwrap();
        assert!(script.check().is_ok());
        assert!(script.game.is_none());
    }

    #[test]
    fn a_declared_game_admits_native_operations() {
        let script = Script::parse("game: launch\nsteps:\n  - game.start_test: {}\n").unwrap();
        assert!(script.check().is_ok());
        assert_eq!(script.game, Some(Mode::Launch));
    }

    #[test]
    fn a_step_must_carry_exactly_one_operation() {
        let error = Script::parse("steps:\n  - {status: {}, speed_up: {}}\n").unwrap_err();
        assert!(error.contains("exactly one operation"), "{error}");
    }

    #[test]
    fn expect_is_not_mistaken_for_an_operation() {
        let script = Script::parse(
            "game: attach\nsteps:\n  - game.status: {}\n    expect: {status: main_menu}\n",
        )
        .unwrap();
        let Step::Call(call) = &script.steps[0] else {
            panic!("expected a plain operation");
        };
        assert_eq!(call.operation, "game.status");
        assert!(call.expect.is_some());
    }

    #[test]
    fn expectations_compare_only_the_declared_fields() {
        let expect: Map<String, Value> = serde_json::from_value(json!({"equal": true})).unwrap();
        assert!(check_expectations(&expect, &json!({"equal": true, "extra": 1})).is_ok());
        let error = check_expectations(&expect, &json!({"equal": false})).unwrap_err();
        assert!(error.contains("expected equal=true"), "{error}");
        assert!(
            check_expectations(&expect, &json!({"other": 1}))
                .unwrap_err()
                .contains("no equal")
        );
    }

    /// A measurement is commonly one entry of a list, so a dotted key walks
    /// into one by position.
    #[test]
    fn an_expectation_reaches_into_a_list_by_position() {
        let result = json!({"sides": {"red": {"survivors": [{"life": 14639}]}}});
        let expect: Map<String, Value> =
            serde_json::from_value(json!({"sides.red.survivors.0.life": 14639})).unwrap();
        check_expectations(&expect, &result).unwrap();
        let missing: Map<String, Value> =
            serde_json::from_value(json!({"sides.red.survivors.1.life": 14639})).unwrap();
        assert!(
            check_expectations(&missing, &result)
                .unwrap_err()
                .contains("no sides.red.survivors.1.life")
        );
    }

    #[test]
    fn unknown_keys_and_operations_are_rejected_before_running() {
        assert!(Script::parse("nope: 1\nsteps: []\n").is_err());
        let script = Script::parse("steps:\n  - frobnicate: {}\n").unwrap();
        assert!(script.check().unwrap_err().contains("frobnicate"));
    }

    #[test]
    fn a_braced_reference_delimits_a_path_that_text_would_swallow() {
        let scope = scope_with(&[("out", json!("work/x")), ("case", json!({"name": "rhino"}))]);
        assert_eq!(
            scope.expand("$out/${case.name}.mcfr").unwrap(),
            json!("work/x/rhino.mcfr"),
            "a bare $case.name.mcfr would look up a field named mcfr"
        );
        // The braced spelling also survives as a whole-string reference.
        assert_eq!(scope.expand("${case.name}").unwrap(), json!("rhino"));
    }

    #[test]
    fn expectations_follow_dotted_paths_into_the_result() {
        let expect: Map<String, Value> =
            serde_json::from_value(json!({"operation.tick_count": 91})).unwrap();
        let result = json!({"operation": {"tick_count": 91}});
        assert!(check_expectations(&expect, &result).is_ok());
        let wrong = json!({"operation": {"tick_count": 92}});
        assert!(check_expectations(&expect, &wrong).is_err());
    }

    #[test]
    fn a_loop_body_cannot_smuggle_a_native_operation_past_the_offline_rule() {
        let script = Script::parse(
            "steps:\n  - foreach: {case: $cases}\n    steps:\n      - game.record_battle: {output: a}\n",
        )
        .unwrap();
        let error = script.check().unwrap_err();
        assert!(error.contains("game.record_battle"), "{error}");
        assert!(error.contains("game:"), "{error}");
    }

    #[test]
    fn foreach_requires_one_binding_and_a_body() {
        assert!(
            Script::parse(
                "steps:\n  - foreach: {a: $x, b: $y}\n    steps:\n      - game.status: {}\n"
            )
            .is_err()
        );
        assert!(Script::parse("game: attach\nsteps:\n  - foreach: {case: $c}\n").is_err());
        assert!(
            Script::parse("game: attach\nsteps:\n  - foreach: {case: $c}\n    steps: []\n")
                .is_err()
        );
    }

    #[test]
    fn nested_loops_are_refused() {
        let error = Script::parse(
            "steps:\n  - foreach: {a: $x}\n    steps:\n      - foreach: {b: $y}\n        steps:\n          - fight.compare: {left: a, right: b}\n",
        )
        .unwrap_err();
        assert!(error.contains("a loop inside foreach"), "{error}");
    }

    #[test]
    fn loop_only_keys_are_refused_on_a_plain_operation() {
        let error = Script::parse("game: attach\nsteps:\n  - game.status: {}\n    steps: []\n")
            .unwrap_err();
        assert!(error.contains("belongs to foreach"), "{error}");
    }

    #[test]
    fn speed_up_must_be_a_boolean() {
        assert_eq!(optional_flag(None, "x").unwrap(), None);
        assert_eq!(
            optional_flag(Some(&json!(false)), "x").unwrap(),
            Some(false)
        );
        assert_eq!(optional_flag(Some(&json!(true)), "x").unwrap(), Some(true));
        // A multiplier is not a thing the native vote can express, so a number
        // must be refused rather than silently treated as "on".
        let error = optional_flag(Some(&json!(3)), "game.record_battle speed_up").unwrap_err();
        assert!(error.contains("true or false"), "{error}");
    }

    #[test]
    fn range_builds_a_bounded_batch_source() {
        let scope = scope_with(&[]);
        assert_eq!(
            evaluate(&json!("range(4)"), &scope).unwrap(),
            json!([0, 1, 2, 3])
        );
        assert_eq!(evaluate(&json!("range(0)"), &scope).unwrap(), json!([]));
        assert!(evaluate(&json!("range(-1)"), &scope).is_err());
        assert!(evaluate(&json!("range(10001)"), &scope).is_err());
    }

    #[test]
    fn watch_recording_is_native_and_its_timeouts_are_unsigned() {
        let script =
            Script::parse("game: launch\nsteps:\n  - game.record_watch_replay: {}\n").unwrap();
        assert!(script.check().is_ok());

        let session = Session::new();
        let mut scope = scope_with(&[]);
        let call = Call {
            operation: "game.record_watch_replay".into(),
            arguments: json!({
                "output_dir": "/tmp/corpus",
                "wait_for_scene_seconds": -1,
            }),
            expect: None,
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(perform(
                &call,
                &call.arguments.clone(),
                &mut scope,
                &session,
            ))
            .unwrap_err();
        assert!(error.contains("unsigned integer"), "{error}");
    }

    #[test]
    fn level_is_bounded_and_belongs_to_a_script_that_takes_a_game() {
        let script =
            Script::parse("game: launch\nlevel: 0\nsteps:\n  - game.status: {}\n").unwrap();
        assert_eq!(script.level, 0);
        // Ordinary work outranks a background batch without saying anything.
        let script = Script::parse("game: attach\nsteps:\n  - game.status: {}\n").unwrap();
        assert_eq!(script.level, DEFAULT_LEVEL);
        assert!(script.level > 0);

        assert!(Script::parse("game: attach\nlevel: 5\nsteps:\n  - game.status: {}\n").is_err());
        assert!(Script::parse("game: attach\nlevel: -1\nsteps:\n  - game.status: {}\n").is_err());
        assert!(Script::parse("game: attach\nlevel: high\nsteps:\n  - game.status: {}\n").is_err());
        // A level with nothing to order is a mistake worth naming.
        let error = Script::parse("level: 2\nsteps:\n  - fight.compare: {left: a, right: b}\n")
            .unwrap_err();
        assert!(error.contains("game:"), "{error}");
    }

    #[test]
    fn game_accepts_only_the_two_declaration_spellings() {
        assert!(Script::parse("game: auto\nsteps:\n  - game.status: {}\n").is_err());
    }
}
