//! Declarative execution scripts.
//!
//! A `.mcscript` is a YAML document describing one bounded run: an optional
//! game acquisition, named variables, and an ordered list of steps. Steps bind
//! results to names so later steps can consume them, which is what separates
//! this from a flat command list.
//!
//! A script that omits `game:` is gameless and may not use a native operation.
//! That is a static rule, checked before anything runs, so `--check` answers
//! "does this need the game?" without touching it.

use crate::acquire::{Launch, Mode};
use crate::session::Session;
use mechcore_protocol::{DEFAULT_LEVEL, InstrumentChannel, MAX_LEVEL};
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
    "game.record",
    "game.toggle_fight",
    "game.speed_up",
    "game.quit_match",
    "game.watch_scenes",
    "game.watch_scene",
    "game.save_replay",
    "game.quit_game",
];

/// Operations that run without a game.
const GAMELESS: &[&str] = &["let", "verify", "convert", "diff", "show"];

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
                "headless": matches!(script.game, Some(Mode::Launch(how)) if how.headless),
                "offline": matches!(script.game, Some(Mode::Launch(how)) if how.offline),
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
            if !matches!(
                key.as_str(),
                "game" | "level" | "headless" | "offline" | "vars" | "steps"
            ) {
                return Err(format!("unknown top-level key {key}"));
            }
        }

        // Only a launch decides how the game runs; a game that is attached to
        // keeps the window it was started with.
        let switch = |key: &str| match document.get(key) {
            None => Ok(false),
            Some(Value::Bool(on)) => Ok(*on),
            Some(other) => Err(format!("{key} must be true or false, got {other}")),
        };
        let how = Launch {
            headless: switch("headless")?,
            offline: switch("offline")?,
        };
        let game = match document.get("game") {
            None => None,
            Some(Value::String(value)) if value == "launch" => Some(Mode::Launch(how)),
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
        for key in ["headless", "offline"] {
            if document.contains_key(key) && !matches!(game, Some(Mode::Launch(_))) {
                return Err(format!(
                    "{key} says how a launched game runs, but the script does not declare \
                     `game: launch`"
                ));
            }
        }
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
            if !NATIVE.contains(&operation) && !GAMELESS.contains(&operation) {
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
    /// gameless rule cannot be evaded by hiding a native call in a loop.
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
    if let Some(mode) = script.game
        && let Err(failure) = session.acquire(mode, script.level).await
    {
        monitor.abort();
        return Err(failure);
    }

    let mut scope = Scope {
        values: BTreeMap::new(),
        base,
        overwrite,
    };
    let outcome = run_steps(&script, &mut scope, &session).await;
    session.release().await;
    monitor.abort();
    // Losing the game to a higher claim is not a failed script. The run ends
    // where it was interrupted, says so, and leaves the game to its claimant.
    if session.was_evicted() {
        println!("{}", json!({"operation": "evicted", "completed": false}));
        return Ok(true);
    }
    outcome?;
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
        "diff" => {
            let fields = closed(arguments, "diff", &["left", "right", "fields", "tick"])?;
            let left = scope.path(fields.get("left").ok_or("diff needs left")?, "diff left")?;
            let right = scope.path(fields.get("right").ok_or("diff needs right")?, "diff right")?;
            // A selection is a list of field groups, or one group by itself.
            let selection = match fields.get("fields") {
                None => None,
                Some(Value::String(group)) => Some(vec![group.clone()]),
                Some(Value::Array(groups)) => Some(
                    groups
                        .iter()
                        .map(|group| {
                            group
                                .as_str()
                                .map(str::to_owned)
                                .ok_or_else(|| format!("diff fields names groups, got {group}"))
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                Some(other) => {
                    return Err(format!(
                        "diff fields is a group or a list of groups, got {other}"
                    ));
                }
            };
            let tick = optional_u64(fields.get("tick"), "diff tick")?
                .map(|tick| u32::try_from(tick).map_err(|_| "diff tick is too large"))
                .transpose()?;
            let (_, report) = crate::diff::diff(&left, &right, selection, tick).map_err(reason)?;
            Ok(report)
        }
        "verify" => verify(arguments, scope),
        "convert" => convert(arguments, scope).await,
        "show" => {
            let fields = closed(arguments, "show", &["input", "view", "tick"])?;
            let input = scope.path(fields.get("input").ok_or("show needs input")?, "show input")?;
            let view = fields
                .get("view")
                .and_then(Value::as_str)
                .ok_or("show needs a view, one of outcome, stats and buildings")?;
            let tick = optional_u64(fields.get("tick"), "show tick")?
                .map(|tick| u32::try_from(tick).map_err(|_| "show tick is too large"))
                .transpose()?;
            // A fight nothing can settle is still a fight worth reading: the
            // survivors and their life are the measurement a capture is taken
            // for, and `unresolved` says what the reading does not cover.
            let (_, shown) = crate::show::show(&input, view, tick).map_err(reason)?;
            Ok(shown)
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
        "game.record" => {
            let record = record(arguments, scope)?;
            let destinations = record.destinations();
            let force = confirm_overwrite(scope, &destinations).await?;
            record.run(session, force).await.map_err(reason)
        }
        "game.toggle_fight" => session.toggle_fight().await,
        "game.speed_up" => session.speed_up().await,
        "game.quit_match" => session.quit_match().await,
        "game.watch_scenes" => {
            let fields = arguments.as_object().cloned().unwrap_or_default();
            if let Some(key) = fields.keys().find(|key| key.as_str() != "refresh") {
                return Err(format!("watch_scenes accepts only refresh, got {key}"));
            }
            let refresh = match fields.get("refresh") {
                None => true,
                Some(value) => value
                    .as_bool()
                    .ok_or("watch_scenes refresh must be a boolean")?,
            };
            session.watch_scenes(refresh).await
        }
        "game.watch_scene" => {
            let fields = arguments.as_object().ok_or("watch_scene takes a mapping")?;
            if let Some(key) = fields.keys().find(|key| key.as_str() != "scene_id") {
                return Err(format!("watch_scene accepts only scene_id, got {key}"));
            }
            let scene_id = fields
                .get("scene_id")
                .and_then(Value::as_i64)
                .and_then(|id| i32::try_from(id).ok())
                .ok_or("watch_scene scene_id must be an i32")?;
            session.watch_scene(scene_id).await
        }
        "game.save_replay" => {
            let fields = arguments.as_object().cloned().unwrap_or_default();
            if let Some(key) = fields.keys().find(|key| key.as_str() != "output") {
                return Err(format!("save_replay accepts only output, got {key}"));
            }
            let output = fields
                .get("output")
                .map(|value| scope.path(value, "save_replay output"))
                .transpose()?;
            session.save_replay(output).await
        }
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

/// The instrument channels a recording step asks for: a list of channel names,
/// recorded into the step's own MCFR.
fn instrument(value: Option<&Value>) -> Result<Vec<InstrumentChannel>, String> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let mut channels: Vec<InstrumentChannel> = serde_json::from_value(value.clone())
        .map_err(|error| format!("instrument must be a list of channel names: {error}"))?;
    channels.sort_unstable();
    channels.dedup();
    Ok(channels)
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
        "glob" => glob(argument.trim(), scope),
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

/// Checks files as `mechcore verify` does, answering every report in one
/// result.
///
/// `input` is one path or a list of them, which is what `glob` binds: naming
/// the files is the caller's, as it is on a command line. The answer is an
/// answer, as `diff`'s is: `valid` says whether every input verified and
/// `invalid` names each one that did not with its reason, so a step that
/// expects `invalid: []` fails naming all of them rather than the first.
fn verify(arguments: &Value, scope: &Scope) -> Result<Value, String> {
    let fields = closed(arguments, "verify", &["input"])?;
    let inputs = match fields.get("input") {
        Some(Value::Array(inputs)) => inputs.clone(),
        Some(input) => vec![input.clone()],
        None => return Err("verify needs input, a file or a list of files".into()),
    };
    if inputs.is_empty() {
        return Err("verify needs at least one input; the list is empty".into());
    }
    let mut reports = Vec::new();
    let mut invalid = Vec::new();
    for input in &inputs {
        let report = crate::verify::check(&scope.path(input, "verify input")?);
        if !report.valid {
            invalid.push(json!({"path": input, "error": report.error}));
        }
        reports.push(
            serde_json::to_value(&report)
                .map_err(|error| format!("cannot write the report: {error}"))?,
        );
    }
    Ok(json!({
        "valid": invalid.is_empty(),
        "invalid": invalid,
        "reports": reports,
    }))
}

/// The files a pattern names, sorted, each spelled as the pattern's directory
/// joined with its name.
///
/// `*` and `?` match within the last component only: a pattern names the
/// files of one directory, which is what a topic keeps its fixtures in. A
/// directory that holds none of them answers an empty list, which a step
/// taking files refuses.
fn glob(pattern: &str, scope: &Scope) -> Result<Value, String> {
    let (directory, name) = match pattern.rsplit_once('/') {
        Some((directory, name)) => (directory, name),
        None => ("", pattern),
    };
    if directory.contains(['*', '?']) {
        return Err(format!(
            "glob {pattern}: only the last component may hold * or ?"
        ));
    }
    let listed = scope.path(
        &Value::String(if directory.is_empty() { "." } else { directory }.into()),
        "glob",
    )?;
    let entries = std::fs::read_dir(&listed)
        .map_err(|error| format!("cannot list {}: {error}", listed.display()))?;
    let mut matched = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot list {}: {error}", listed.display()))?;
        let file = entry.file_name();
        let Some(file) = file.to_str() else { continue };
        if entry.path().is_file() && wildcard(name, file) {
            matched.push(if directory.is_empty() {
                file.to_owned()
            } else {
                format!("{directory}/{file}")
            });
        }
    }
    matched.sort();
    Ok(Value::Array(
        matched.into_iter().map(Value::String).collect(),
    ))
}

/// Whether `name` matches `pattern`, where `*` is any run of characters and
/// `?` any one.
fn wildcard(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    // The last `*` seen, and where in `name` its match ends so far: a
    // mismatch after it lets it take one more character and tries again.
    let (mut at, mut from) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while from < name.len() {
        match pattern.get(at) {
            Some('*') => {
                star = Some((at, from));
                at += 1;
            }
            Some(&character) if character == '?' || character == name[from] => {
                at += 1;
                from += 1;
            }
            _ => match star {
                Some((star_at, star_from)) => {
                    at = star_at + 1;
                    from = star_from + 1;
                    star = Some((star_at, star_from + 1));
                }
                None => return false,
            },
        }
    }
    pattern[at..].iter().all(|character| *character == '*')
}

/// Converts a file as `mechcore convert` does, returning the same result
/// object it prints so `expect` can assert any of its fields.
///
/// An existing `output` is a destination like a recording's: the run replaces
/// it only when `confirm_overwrite` says so. A rewrite answered without a
/// destination answers the document it would have written.
async fn convert(arguments: &Value, scope: &Scope) -> Result<Value, String> {
    let fields = closed(
        arguments,
        "convert",
        &["input", "to", "output", "seed", "round"],
    )?;
    let input = scope.path(
        fields.get("input").ok_or("convert needs input")?,
        "convert input",
    )?;
    let to = fields
        .get("to")
        .and_then(Value::as_str)
        .ok_or("convert needs to, the kind to convert to")?;
    let to = crate::convert::parse_kind(to).map_err(reason)?;
    let output = match fields.get("output") {
        None | Some(Value::Null) => None,
        Some(value) => Some(scope.path(value, "convert output")?),
    };
    let force = match &output {
        Some(output) => confirm_overwrite(scope, &[output.as_path()]).await?,
        None => false,
    };
    let answer = crate::convert::convert(&crate::convert::Request {
        input,
        to,
        output,
        seed: optional_i32(fields.get("seed"), "convert seed")?,
        round: optional_i32(fields.get("round"), "convert round")?,
        force,
    })
    .map_err(reason)?;
    match answer {
        crate::convert::Answer::Report { value, .. } => Ok(value),
        crate::convert::Answer::Document(text) => serde_yaml::from_str(&text)
            .map_err(|error| format!("cannot read the converted document back: {error}")),
    }
}

/// Reads a `game.record` step into what it records.
///
/// `input` is a layout given whole, or a path whose kind the file itself says.
fn record(arguments: &Value, scope: &Scope) -> Result<crate::game::Record, String> {
    let fields = closed(
        arguments,
        "game.record",
        &[
            "input",
            "output",
            "seed",
            "round",
            "video_output",
            "speed_up",
            "instrument",
            "watch",
            "output_dir",
            "wait_for_scene_seconds",
            "match_timeout_seconds",
        ],
    )?;
    let path = |key: &str| {
        fields
            .get(key)
            .map(|value| scope.path(value, &format!("game.record {key}")))
            .transpose()
    };
    let (layout, input) = match fields.get("input") {
        None => (None, None),
        Some(Value::Object(layout)) => (Some(Value::Object(layout.clone())), None),
        Some(value) => (None, Some(scope.path(value, "game.record input")?)),
    };
    crate::game::RecordRequest {
        layout,
        input,
        output: path("output")?,
        seed: optional_i32(fields.get("seed"), "game.record seed")?,
        round: optional_i32(fields.get("round"), "game.record round")?,
        video: path("video_output")?,
        speed_up: optional_flag(fields.get("speed_up"), "game.record speed_up")?,
        instrument: instrument(fields.get("instrument"))?,
        watch: optional_flag(fields.get("watch"), "game.record watch")?.unwrap_or(false),
        output_dir: path("output_dir")?,
        wait_for_scene_seconds: optional_u64(
            fields.get("wait_for_scene_seconds"),
            "game.record wait_for_scene_seconds",
        )?,
        match_timeout_seconds: optional_u64(
            fields.get("match_timeout_seconds"),
            "game.record match_timeout_seconds",
        )?,
    }
    .decide()
    .map_err(reason)
}

/// A step's argument mapping, refusing a field the operation does not take.
///
/// `force` is named apart: replacing a file is the run's decision, not the
/// script's.
fn closed<'a>(
    arguments: &'a Value,
    operation: &str,
    accepted: &[&str],
) -> Result<&'a Map<String, Value>, String> {
    let fields = arguments
        .as_object()
        .ok_or_else(|| format!("{operation} takes a mapping"))?;
    if fields.contains_key("force") {
        return Err("force is not a script field; pass --force to mechcore run".to_string());
    }
    if let Some(key) = fields.keys().find(|key| !accepted.contains(&key.as_str())) {
        return Err(format!(
            "{operation} accepts only {}, got {key}",
            accepted.join(", ")
        ));
    }
    Ok(fields)
}

/// What a failed operation says, as a step's error.
#[allow(clippy::needless_pass_by_value)]
fn reason(failure: crate::cli::Failure) -> String {
    failure.reason().to_owned()
}

/// Read an optional signed 32-bit integer field.
fn optional_i32(value: Option<&Value>, what: &str) -> Result<Option<i32>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .and_then(|number| i32::try_from(number).ok())
            .map(Some)
            .ok_or_else(|| format!("{what} must be a signed 32-bit integer, got {value}")),
    }
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
    fn a_recording_asks_for_channels_by_name() {
        assert!(instrument(None).unwrap().is_empty());
        let channels = instrument(Some(&json!([
            "target_search",
            "target_refs",
            "target_refs"
        ])))
        .unwrap();
        assert_eq!(
            channels,
            [
                InstrumentChannel::TargetRefs,
                InstrumentChannel::TargetSearch
            ]
        );
        assert!(instrument(Some(&json!(["rvo"]))).is_err());
        assert!(instrument(Some(&json!(["selector_score"]))).is_err());
        assert!(instrument(Some(&json!({"output": "local.h5"}))).is_err());
    }

    #[test]
    fn a_wildcard_matches_within_one_name() {
        assert!(wildcard("*.yaml", "tower.yaml"));
        assert!(wildcard("*.yaml", ".yaml"));
        assert!(!wildcard("*.yaml", "tower.yml"));
        assert!(wildcard("t?wer-*-*.yaml", "tower-1-b.yaml"));
        assert!(!wildcard("t?wer", "tower-1"));
        assert!(wildcard("*", "anything"));
        assert!(wildcard("a*b*c", "abxbc"));
        assert!(!wildcard("a*b*c", "abxbd"));
    }

    /// A glob lists one directory's matching files, sorted and spelled as
    /// the pattern spells its directory; a verify step reads every one and
    /// names each that does not verify.
    #[test]
    fn a_verify_step_names_every_file_that_does_not_verify() {
        let directory = tempfile::tempdir().unwrap();
        let layout = "kind: layout\nround: 1\nseed: 7\nblue:\n  units: [{name: marksman, index: 0, position: {x: 0, y: -50}}]\nred:\n  units: [{name: arclight, index: 0, position: {x: 0, y: -50}}]\n";
        std::fs::create_dir(directory.path().join("fights")).unwrap();
        std::fs::write(directory.path().join("fights/b.yaml"), layout).unwrap();
        std::fs::write(directory.path().join("fights/a.yaml"), "kind: novel\n").unwrap();
        std::fs::write(directory.path().join("fights/c.yaml"), "round: 1\n").unwrap();
        std::fs::write(directory.path().join("fights/notes.md"), "").unwrap();
        let scope = Scope {
            overwrite: Overwrite::Ask,
            values: BTreeMap::new(),
            base: directory.path().to_path_buf(),
        };
        let listed = evaluate(&json!("glob(fights/*.yaml)"), &scope).unwrap();
        assert_eq!(
            listed,
            json!(["fights/a.yaml", "fights/b.yaml", "fights/c.yaml"])
        );
        assert_eq!(
            evaluate(&json!("glob(fights/*.json)"), &scope).unwrap(),
            json!([])
        );
        assert!(evaluate(&json!("glob(*/a.yaml)"), &scope).is_err());

        let result = verify(&json!({"input": listed}), &scope).unwrap();
        assert_eq!(result["valid"], false);
        assert_eq!(result["reports"].as_array().unwrap().len(), 3);
        let invalid = result["invalid"].as_array().unwrap();
        assert_eq!(invalid.len(), 2, "{result}");
        assert_eq!(invalid[0]["path"], "fights/a.yaml");
        assert_eq!(invalid[1]["path"], "fights/c.yaml");
        assert!(invalid[1]["error"].as_str().unwrap().contains("kind"));

        let one = verify(&json!({"input": "fights/b.yaml"}), &scope).unwrap();
        assert_eq!(one["valid"], true);
        assert_eq!(one["invalid"], json!([]));
        assert!(verify(&json!({"input": []}), &scope).is_err());
        assert!(verify(&json!({"inputs": "fights/b.yaml"}), &scope).is_err());
    }

    #[tokio::test]
    async fn convert_replaces_an_existing_output_under_force() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("simulated.mcfr");
        std::fs::write(&output, b"existing").unwrap();
        let scope = Scope {
            overwrite: Overwrite::Always,
            values: BTreeMap::new(),
            base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."),
        };
        let arguments = json!({
            "input": "tests/regression/crawlers-vs-crawlers.yaml",
            "to": "mcfr",
            "seed": 1_787_778_788,
            "output": output.display().to_string(),
        });

        let result = convert(&arguments, &scope).await.unwrap();
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
            operation: "game.record".into(),
            arguments: json!({"output": "/tmp/a.mcfr", "force": true}),
            expect: None,
        };
        let error = perform(&call, &call.arguments.clone(), &mut scope, &session)
            .await
            .unwrap_err();
        assert!(error.contains("--force"), "{error}");
    }

    #[test]
    fn gameless_scripts_may_not_use_native_operations() {
        let script = Script::parse("steps:\n  - game.start_test: {}\n").unwrap();
        let error = script.check().unwrap_err();
        assert!(error.contains("game.start_test"), "{error}");
        assert!(error.contains("game:"), "{error}");
    }

    #[test]
    fn gameless_scripts_accept_gameless_operations() {
        let script = Script::parse("steps:\n  - diff: {left: a.mcfr, right: b.mcfr}\n").unwrap();
        assert!(script.check().is_ok());
        assert!(script.game.is_none());

        let script =
            Script::parse("steps:\n  - convert: {input: a.yaml, to: mcfr, seed: 7}\n").unwrap();
        assert!(script.check().is_ok());
        assert!(script.game.is_none());
    }

    #[test]
    fn a_declared_game_admits_native_operations() {
        let script = Script::parse("game: launch\nsteps:\n  - game.start_test: {}\n").unwrap();
        assert!(script.check().is_ok());
        assert_eq!(script.game, Some(Mode::Launch(Launch::default())));
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
    fn a_loop_body_cannot_smuggle_a_native_operation_past_the_gameless_rule() {
        let script = Script::parse(
            "steps:\n  - foreach: {case: $cases}\n    steps:\n      - game.record: {output: a}\n",
        )
        .unwrap();
        let error = script.check().unwrap_err();
        assert!(error.contains("game.record"), "{error}");
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
            "steps:\n  - foreach: {a: $x}\n    steps:\n      - foreach: {b: $y}\n        steps:\n          - diff: {left: a, right: b}\n",
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
        let error = optional_flag(Some(&json!(3)), "game.record speed_up").unwrap_err();
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
            Script::parse("game: launch\nsteps:\n  - game.record: {watch: true}\n").unwrap();
        assert!(script.check().is_ok());

        let session = Session::new();
        let mut scope = scope_with(&[]);
        let call = Call {
            operation: "game.record".into(),
            arguments: json!({
                "watch": true,
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
        let error = Script::parse("level: 2\nsteps:\n  - diff: {left: a, right: b}\n").unwrap_err();
        assert!(error.contains("game:"), "{error}");
    }

    #[test]
    fn headless_and_offline_belong_to_a_launch() {
        let script = Script::parse(
            "game: launch\nheadless: true\noffline: true\nsteps:\n  - game.status: {}\n",
        )
        .unwrap();
        assert_eq!(
            script.game,
            Some(Mode::Launch(Launch {
                headless: true,
                offline: true
            }))
        );
        let script = Script::parse("game: launch\nsteps:\n  - game.status: {}\n").unwrap();
        assert_eq!(script.game, Some(Mode::Launch(Launch::default())));

        for key in ["headless", "offline"] {
            assert!(
                Script::parse(&format!(
                    "game: launch\n{key}: yes please\nsteps:\n  - game.status: {{}}\n"
                ))
                .is_err()
            );
            // An attached game was started by somebody else, window, network
            // and all.
            for header in ["game: attach\n", ""] {
                let error = Script::parse(&format!(
                    "{header}{key}: true\nsteps:\n  - diff: {{left: a, right: b}}\n"
                ))
                .unwrap_err();
                assert!(error.contains("game: launch"), "{error}");
            }
        }
    }

    #[test]
    fn game_accepts_only_the_two_declaration_spellings() {
        assert!(Script::parse("game: auto\nsteps:\n  - game.status: {}\n").is_err());
    }
}
