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
use crate::mcfr;
use crate::session::Session;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Operations that require an acquired game.
const NATIVE: &[&str] = &[
    "status",
    "start_test",
    "apply_layout",
    "record_battle",
    "record_replay_round",
    "toggle_fight",
    "speed_up",
    "quit_match",
    "quit_game",
];

/// Operations that run without a game.
const OFFLINE: &[&str] = &["let", "compare", "sim"];

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
        .block_on(execute(script, base()))
}

/// Relative paths resolve against the working directory, matching every other
/// subcommand, so a script reads the same as the commands it replaces.
fn base() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

struct Options {
    script: PathBuf,
    check_only: bool,
}

impl Options {
    fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut script = None;
        let mut check_only = false;
        for argument in arguments {
            match argument.as_str() {
                "--check" => check_only = true,
                other if other.starts_with("--") => {
                    return Err(format!("unexpected option {other}"));
                }
                other if script.is_none() => script = Some(PathBuf::from(other)),
                other => return Err(format!("unexpected argument {other}")),
            }
        }
        Ok(Self {
            script: script.ok_or("usage: mechcore run <script.mcscript> [--check]")?,
            check_only,
        })
    }
}

#[cfg_attr(test, derive(Debug))]
struct Script {
    game: Option<Mode>,
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
            if !matches!(key.as_str(), "game" | "vars" | "steps") {
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
        Ok(Self { game, vars, steps })
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
        let body = mapping
            .get("steps")
            .ok_or("foreach needs steps")?
            .as_array()
            .ok_or("foreach steps must be a list")?
            .iter()
            .map(Self::parse)
            .collect::<Result<Vec<_>, _>>()?;
        if body.is_empty() {
            return Err("foreach needs at least one body step".into());
        }
        if body.iter().any(|step| matches!(step, Self::ForEach(_))) {
            return Err("nested foreach is not supported".into());
        }
        Ok(Self::ForEach(ForEach {
            binding,
            source,
            filter,
            body,
        }))
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

async fn execute(script: Script, base: PathBuf) -> Result<bool, String> {
    let session = Session::new();
    let monitor = tokio::spawn(Session::monitor_status(session.clone()));
    let mut ownership = None;
    if let Some(mode) = script.game {
        match session.acquire(mode).await {
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
    };
    let outcome = run_steps(&script, &mut scope, &session).await;
    let closed = session.release(ownership).await;
    monitor.abort();
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
        "compare" => {
            let fields = arguments
                .as_object()
                .ok_or("compare takes left and right paths")?;
            let left = scope.path(
                fields.get("left").ok_or("compare needs left")?,
                "compare left",
            )?;
            let right = scope.path(
                fields.get("right").ok_or("compare needs right")?,
                "compare right",
            )?;
            let detailed = optional_flag(fields.get("verbose"), "compare verbose")?;
            let (_, report) = mcfr::compare(&left, &right, detailed.unwrap_or(false))?;
            Ok(report)
        }
        "sim" => simulate(arguments, scope),
        "status" => Ok(session.current_status()),
        "start_test" => {
            let seed = arguments
                .as_object()
                .and_then(|fields| fields.get("seed"))
                .and_then(Value::as_i64)
                .map(i32::try_from)
                .transpose()
                .map_err(|_| "seed must fit in a signed 32-bit integer".to_string())?;
            session.start_test(seed).await
        }
        "apply_layout" => {
            let (layout, seed) = split_layout_arguments(arguments)?;
            session.apply_layout(layout, seed).await
        }
        "record_battle" => {
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
            let speed_up = optional_flag(fields.get("speed_up"), "record_battle speed_up")?;
            let force = optional_flag(fields.get("force"), "record_battle force")?;
            session
                .record_battle(output, video, speed_up, force.unwrap_or(false), None)
                .await
                .map_err(|value| value.to_string())
        }
        "record_replay_round" => {
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
            let speed_up = optional_flag(fields.get("speed_up"), "record_replay_round speed_up")?;
            let force = optional_flag(fields.get("force"), "record_replay_round force")?;
            session
                .record_replay_round(grbr, round, output, speed_up, force.unwrap_or(false), None)
                .await
        }
        "toggle_fight" => session.toggle_fight().await,
        "speed_up" => session.speed_up().await,
        "quit_match" => session.quit_match().await,
        "quit_game" => session.quit_game().await,
        other => Err(format!("unknown operation {other}")),
    }
}

/// Split `apply_layout` arguments into the layout and an optional seed override.
///
/// A layout's top-level keys are closed to `seed`, `round` and `sides`, so a
/// `layout` key can only be the wrapper form and never a layout itself.
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
        other => Err(format!("unknown function {other}")),
    }
}

/// Runs the deterministic simulator without a game, returning the same result
/// object `mechcore sim` prints so `expect` can assert any of its fields.
fn simulate(arguments: &Value, scope: &Scope) -> Result<Value, String> {
    let fields = arguments
        .as_object()
        .ok_or("sim takes a mapping with layout and optional seed, output and config")?;
    for key in fields.keys() {
        if !matches!(key.as_str(), "layout" | "seed" | "output" | "config") {
            return Err(format!(
                "sim accepts only layout, seed, output and config, got {key}"
            ));
        }
    }
    let layout = scope.path(
        fields.get("layout").ok_or("sim needs layout")?,
        "sim layout",
    )?;
    let seed = match fields.get("seed") {
        None | Some(Value::Null) => None,
        Some(value) => Some(
            value
                .as_i64()
                .and_then(|seed| i32::try_from(seed).ok())
                .ok_or("sim seed must be a signed 32-bit integer")?,
        ),
    };
    let output = match fields.get("output") {
        None | Some(Value::Null) => None,
        Some(value) => Some(scope.path(value, "sim output")?),
    };
    let config = match fields.get("config") {
        None | Some(Value::Null) => None,
        Some(value) => Some(scope.path(value, "sim config")?),
    };
    let result = mechcore_simulation::simulate_layout_with_config(
        &layout,
        output.as_deref(),
        seed,
        config.as_deref(),
    )
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
/// a recording reports `operation.tick_count`, not `tick_count`.
fn field_at<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.')
        .try_fold(value, |current, part| current.get(part))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope_with(values: &[(&str, Value)]) -> Scope {
        Scope {
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
    fn offline_scripts_may_not_use_native_operations() {
        let script = Script::parse("steps:\n  - start_test: {}\n").unwrap();
        let error = script.check().unwrap_err();
        assert!(error.contains("start_test"), "{error}");
        assert!(error.contains("game:"), "{error}");
    }

    #[test]
    fn offline_scripts_accept_offline_operations() {
        let script = Script::parse("steps:\n  - compare: {left: a.mcfr, right: b.mcfr}\n").unwrap();
        assert!(script.check().is_ok());
        assert!(script.game.is_none());

        let script = Script::parse("steps:\n  - sim: {layout: a.yaml, seed: 7}\n").unwrap();
        assert!(script.check().is_ok());
        assert!(script.game.is_none());
    }

    #[test]
    fn a_declared_game_admits_native_operations() {
        let script = Script::parse("game: launch\nsteps:\n  - start_test: {}\n").unwrap();
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
            "game: attach\nsteps:\n  - status: {}\n    expect: {status: main_menu}\n",
        )
        .unwrap();
        let Step::Call(call) = &script.steps[0] else {
            panic!("expected a plain operation");
        };
        assert_eq!(call.operation, "status");
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
            "steps:\n  - foreach: {case: $cases}\n    steps:\n      - record_battle: {output: a}\n",
        )
        .unwrap();
        let error = script.check().unwrap_err();
        assert!(error.contains("record_battle"), "{error}");
        assert!(error.contains("game:"), "{error}");
    }

    #[test]
    fn foreach_requires_one_binding_and_a_body() {
        assert!(
            Script::parse("steps:\n  - foreach: {a: $x, b: $y}\n    steps:\n      - status: {}\n")
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
            "steps:\n  - foreach: {a: $x}\n    steps:\n      - foreach: {b: $y}\n        steps:\n          - compare: {left: a, right: b}\n",
        )
        .unwrap_err();
        assert!(error.contains("nested foreach"), "{error}");
    }

    #[test]
    fn loop_only_keys_are_refused_on_a_plain_operation() {
        let error =
            Script::parse("game: attach\nsteps:\n  - status: {}\n    steps: []\n").unwrap_err();
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
        let error = optional_flag(Some(&json!(3)), "record_battle speed_up").unwrap_err();
        assert!(error.contains("true or false"), "{error}");
    }

    #[test]
    fn game_accepts_only_the_two_declaration_spellings() {
        assert!(Script::parse("game: auto\nsteps:\n  - status: {}\n").is_err());
    }
}
