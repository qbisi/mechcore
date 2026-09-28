//! `watch`: record live standard 1v1 matches by visiting them in turn.
//!
//! One account watches one match at a time, and a spectator runs about 100 s
//! (`BattleInfo.WatchDelay`) behind the match it watches: it performs an
//! action once the action is that old. A replay saved mid-match nonetheless
//! holds every round the spectator was present for, complete once it has
//! entered that round's fight, so a match is recorded a round at a time by a
//! spectator that comes and goes. This command keeps a handful of rooms and
//! visits them one after another.
//!
//! Round r is recorded from its fight until round r + 1's fight begins. While
//! round r + 1 deploys the server still reports round r, a spectator joining
//! then is put at round r, and it enters round r's fight as soon as the
//! round's last action is 100 s old: about 5 s after joining once the
//! deployment has run for 65 s, about 50 s after joining 20 s in. So a round is
//! captured by joining late in the next round's deployment, before its time is
//! up: a visit that finds a deployment young leaves and comes back.
//!
//! - A room is admitted from the lobby's page while its round is at most 2,
//!   while round 1 can still be recorded; recorded before round 2, the replay
//!   also holds round 0, the opening specialist choice.
//! - A round the match has gone past unrecorded is missed. A room whose join
//!   the server does not answer, or whose match finishes, has ended.
//!
//! `docs/spec/mechcore/cli.md` is the contract; every visit is journalled to
//! `<out>/watch.jsonl`, and each capture saved as `<out>/<scene>/r<round>.grbr`.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::acquire::{Launch, Mode};
use crate::cli::{Args, Failure, Outcome, Verdict};
use crate::session::{Session, is_status};

/// How the schedule is paced, in seconds.
#[derive(Clone, Copy, Debug)]
struct Timing {
    /// How often the lobby's page is read for new rooms.
    list_interval: f64,
    /// The least time between two visits of one room.
    min_interval: f64,
    /// How far into the next deployment a round is captured: late enough that
    /// the round's last action has aged past the watch delay.
    capture_from: f64,
    /// A deployment with less than this left is captured at once, whatever
    /// its age, before its fight closes the window.
    capture_last: f64,
    /// The shortest fight, after which the next deployment is looked for.
    min_fight: f64,
    /// How long a room not yet deploying is left before it is looked at again.
    opening_poll: f64,
    /// How long a capture waits for the spectator to enter the fight.
    capture_timeout: f64,
    /// How far behind the match a spectator runs: every action reaches it
    /// 101 s after it was made, measured on 33 actions of one match.
    delay: f64,
    /// A room's own deployments, as short as they have been, are captured this
    /// long before their shortest end.
    deploy_margin: f64,
    /// Idle time a capture must leave before another room falls due.
    idle_margin: f64,
    /// The longest wait a capture is taken early for. An early capture sees
    /// only the visits already planned, and waits of a minute or two blocked
    /// rooms admitted or falling due meanwhile: 22 of 36 rounds missed in a
    /// half-hour run were missed during another room's capture of over 30 s.
    idle_wait: f64,
}

const TIMING: Timing = Timing {
    list_interval: 20.0,
    min_interval: 10.0,
    capture_from: 55.0,
    capture_last: 20.0,
    min_fight: 20.0,
    opening_poll: 20.0,
    capture_timeout: 200.0,
    delay: 101.0,
    deploy_margin: 15.0,
    idle_margin: 10.0,
    idle_wait: 20.0,
};

fn seconds(value: f64) -> Duration {
    Duration::from_secs_f64(value.max(0.0))
}

/// Where the server said a match was.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    /// Loading, preparing, or anything before the first deployment.
    Before,
    /// Deploying the round after the one reported, `elapsed` seconds in, with
    /// `remaining` left of its time.
    Deploy {
        elapsed: f64,
        remaining: f64,
    },
    /// Fighting the round reported, `elapsed` seconds in.
    Fighting {
        elapsed: f64,
    },
    Ending,
}

/// A visit's reading: the round the server reported, which in deployment is
/// the round last fought, and the stage.
#[derive(Clone, Copy, Debug)]
struct Seen {
    at: Instant,
    round: i32,
    stage: Stage,
}

/// How long each round's deployment has lasted in the corpus: for every round,
/// a low quantile of the moment the later player finished deploying
/// (`PAD_FinishDeploy`'s `LocalTime`), seconds into the deployment.
type DeployTable = BTreeMap<i32, f64>;

/// The quantile of a round's deployments a capture plans around: one in ten
/// ended sooner.
const DEPLOY_QUANTILE: f64 = 0.1;

/// Reads every replay under `corpus` and tabulates its rounds' deployments.
fn deploy_table(corpus: &Path) -> Result<DeployTable, String> {
    let mut ends: BTreeMap<i32, Vec<f64>> = BTreeMap::new();
    let entries = std::fs::read_dir(corpus)
        .map_err(|error| format!("cannot read {}: {error}", corpus.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("grbr") {
            continue;
        }
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let Ok(record) = mechcore_document::record::read(&bytes) else {
            continue;
        };
        let players = &record.players.entries;
        if players.len() != 2 {
            continue;
        }
        let mut finished: BTreeMap<i32, Vec<f64>> = BTreeMap::new();
        for player in players {
            for round in &player.rounds.entries {
                if let Some(at) = round
                    .actions
                    .entries
                    .iter()
                    .filter(|action| action.kind == "PAD_FinishDeploy")
                    .filter_map(|action| action.local_time)
                    .reduce(f64::max)
                {
                    finished.entry(round.round).or_default().push(at);
                }
            }
        }
        for (round, times) in finished {
            if round >= 1 && times.len() == 2 {
                ends.entry(round)
                    .or_default()
                    .push(times.into_iter().fold(0.0, f64::max));
            }
        }
    }
    Ok(ends
        .into_iter()
        .map(|(round, mut times)| {
            times.sort_by(f64::total_cmp);
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                clippy::cast_precision_loss,
                reason = "an index into a short list"
            )]
            let index = ((times.len() as f64) * DEPLOY_QUANTILE) as usize;
            (round, times[index.min(times.len() - 1)])
        })
        .collect())
}

/// The early end of `round`'s deployment, seconds in: the table's, or the full
/// time for a round the corpus has none of.
fn early_end(table: &DeployTable, round: i32) -> f64 {
    table.get(&round).copied().unwrap_or(100.0)
}

/// How far into `round`'s deployment the round before it is captured: late
/// enough that its actions have aged, and before that deployment's early end.
fn capture_from(timing: &Timing, table: &DeployTable, round: i32) -> f64 {
    timing
        .capture_from
        .min(early_end(table, round) - timing.deploy_margin)
        .max(0.0)
}

#[derive(Debug)]
struct Room {
    scene_id: i32,
    admitted: Instant,
    seen: Option<Seen>,
    last_visit: Option<Instant>,
    captured: BTreeSet<i32>,
    missed: BTreeSet<i32>,
    files: Vec<PathBuf>,
    ended: Option<&'static str>,
}

impl Room {
    fn new(scene_id: i32, at: Instant) -> Self {
        Self {
            scene_id,
            admitted: at,
            seen: None,
            last_visit: None,
            captured: BTreeSet::new(),
            missed: BTreeSet::new(),
            files: Vec::new(),
            ended: None,
        }
    }

    /// The longest a capture started now would wait, from what was seen.
    fn capture_wait(seen: Seen, timing: &Timing) -> f64 {
        match seen.stage {
            // The round's last action was made before this deployment began,
            // and its fight lasted at least the shortest fight.
            Stage::Deploy { elapsed, .. } => timing.delay - timing.min_fight - elapsed,
            Stage::Fighting { elapsed } => timing.delay - elapsed,
            Stage::Before | Stage::Ending => 0.0,
        }
        .max(0.0)
    }

    /// When this room should next be visited.
    fn due(&self, timing: &Timing, table: &DeployTable) -> Instant {
        let planned = match self.seen {
            None => self.admitted,
            Some(Seen { at, round, stage }) => {
                let open = round >= 1 && !self.captured.contains(&round);
                match stage {
                    Stage::Before => at + seconds(timing.opening_poll),
                    // Round `round` can still be captured: come back late in
                    // this deployment of round + 1, before its early end.
                    Stage::Deploy { elapsed, remaining } if open => {
                        at + seconds(
                            (capture_from(timing, table, round + 1) - elapsed)
                                .min(remaining - timing.capture_last),
                        )
                    }
                    // The next round is captured in the deployment after its
                    // fight: this deployment ends by its early end at the
                    // soonest, then a fight, then the next deployment.
                    Stage::Deploy { elapsed, remaining } => {
                        at + seconds(
                            (early_end(table, round + 1) - elapsed)
                                .min(remaining)
                                .max(0.0)
                                + timing.min_fight
                                + capture_from(timing, table, round + 2),
                        )
                    }
                    // This fight's round is captured in the deployment after
                    // it.
                    Stage::Fighting { elapsed } => {
                        at + seconds(
                            timing.min_fight - elapsed + capture_from(timing, table, round + 1),
                        )
                    }
                    Stage::Ending => at + Duration::from_secs(3600),
                }
            }
        };
        match self.last_visit {
            Some(last) => planned.max(last + seconds(timing.min_interval)),
            None => planned,
        }
    }

    /// Takes what a visit saw, marks the rounds the match has gone past
    /// unrecorded, and answers the round to capture now, if any. A round that
    /// can be captured is captured now when it is late enough in its window,
    /// or when the wait fits before `free_until`, the next other room's visit.
    fn observe(
        &mut self,
        seen: Seen,
        timing: &Timing,
        table: &DeployTable,
        free_until: Option<Instant>,
    ) -> Option<i32> {
        let first = self.seen.is_none();
        self.seen = Some(seen);
        self.last_visit = Some(seen.at);
        // The earliest round that can still be captured.
        let open = match seen.stage {
            Stage::Deploy { .. } | Stage::Fighting { .. } => seen.round.max(1),
            Stage::Before | Stage::Ending => 1,
        };
        if seen.stage == Stage::Ending {
            self.ended = Some("ending");
            return None;
        }
        if first && open > 1 {
            self.ended = Some("admitted after round 1");
            return None;
        }
        for round in 1..open {
            if !self.captured.contains(&round) {
                self.missed.insert(round);
            }
        }
        let capturable = seen.round >= 1
            && !self.captured.contains(&seen.round)
            && matches!(seen.stage, Stage::Deploy { .. } | Stage::Fighting { .. });
        if !capturable {
            return None;
        }
        let late = match seen.stage {
            Stage::Deploy { elapsed, remaining } => {
                elapsed >= capture_from(timing, table, seen.round + 1)
                    || remaining <= timing.capture_last
            }
            _ => false,
        };
        let wait = Self::capture_wait(seen, timing);
        let idle = wait <= timing.idle_wait
            && free_until.is_none_or(|free| seen.at + seconds(wait + timing.idle_margin) <= free);
        (late || idle).then_some(seen.round)
    }
}

/// `watch --out <dir> [--duration <seconds>] [--rooms <n>] [--window] [--level <n>]`
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let out = arguments
        .value("--out")?
        .map(PathBuf::from)
        .ok_or_else(|| Failure::usage("watch needs --out <directory>"))?;
    let duration = number(&mut arguments, "--duration", 3600)?;
    let rooms = number(&mut arguments, "--rooms", 4)?;
    let table = match arguments.value("--corpus")? {
        Some(corpus) => deploy_table(Path::new(&corpus)).map_err(Failure::failed)?,
        None => DeployTable::new(),
    };
    let window = arguments.flag("--window")?;
    let level = crate::acquire::level(&mut arguments)?;
    arguments.finish()?;
    if rooms == 0 {
        return Err(Failure::usage("--rooms must be at least 1"));
    }
    std::fs::create_dir_all(&out)
        .map_err(|error| Failure::failed(format!("cannot create {}: {error}", out.display())))?;
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| Failure::failed(format!("cannot create async runtime: {error}")))?
        .block_on(async {
            let session = Session::new();
            let monitor = tokio::spawn(Session::monitor_status(session.clone()));
            let mode = Mode::Launch(Launch {
                headless: !window,
                offline: false,
            });
            if let Err(failure) = session.acquire(mode, level).await {
                monitor.abort();
                return Err(Failure::unavailable(failure));
            }
            let mut watcher = Watcher::new(
                session.clone(),
                out,
                usize::try_from(rooms).unwrap_or(usize::MAX),
                table,
            )?;
            let outcome = watcher.run(Duration::from_secs(duration)).await;
            session.release().await;
            monitor.abort();
            let summary = outcome.map_err(Failure::failed)?;
            crate::cli::emit(&summary, crate::cli::Format::Json)?;
            Ok(Verdict::Yes)
        })
}

fn number(arguments: &mut Args, name: &str, default: u64) -> Result<u64, Failure> {
    arguments.value(name)?.map_or(Ok(default), |value| {
        value
            .parse()
            .map_err(|_| Failure::usage(format!("{name} takes a whole number, got {value}")))
    })
}

struct Watcher {
    session: Arc<Session>,
    out: PathBuf,
    journal: std::fs::File,
    capacity: usize,
    rooms: BTreeMap<i32, Room>,
    visits: u64,
    table: DeployTable,
}

impl Watcher {
    fn new(
        session: Arc<Session>,
        out: PathBuf,
        capacity: usize,
        table: DeployTable,
    ) -> Result<Self, Failure> {
        let path = out.join("watch.jsonl");
        let journal = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|error| Failure::failed(format!("cannot open {}: {error}", path.display())))?;
        Ok(Self {
            session,
            out,
            journal,
            capacity,
            rooms: BTreeMap::new(),
            visits: 0,
            table,
        })
    }

    fn note(&mut self, mut event: Value) {
        event["unix"] = json!(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0.0, |elapsed| elapsed.as_secs_f64())
        );
        let _ = writeln!(self.journal, "{event}");
    }

    async fn run(&mut self, duration: Duration) -> Result<Value, String> {
        let start = Instant::now();
        let deadline = start + duration;
        let table = self.table.clone();
        self.note(json!({"event": "start", "deploy_table": table}));
        let mut listed: Option<Instant> = None;
        while Instant::now() < deadline {
            let now = Instant::now();
            if listed.is_none_or(|at| now >= at + seconds(TIMING.list_interval)) {
                self.admit().await?;
                listed = Some(Instant::now());
            }
            let next = self
                .rooms
                .values()
                .filter(|room| room.ended.is_none())
                .map(|room| (room.due(&TIMING, &self.table), room.scene_id))
                .min();
            match next {
                Some((due, scene_id)) if due <= Instant::now() => {
                    if let Err(error) = self.visit(scene_id).await {
                        // A visit that went wrong is the room's, not the run's:
                        // it is journalled, the room is let go, and the run goes
                        // on from the main menu.
                        self.note(json!({"event": "error", "scene": scene_id, "error": error}));
                        let room = self.room(scene_id);
                        room.ended = Some("visit failed");
                        room.last_visit = Some(Instant::now());
                        if !is_status(&self.session.current_status(), "main_menu") {
                            self.session.quit_match().await?;
                        }
                    }
                }
                other => {
                    let wake = other
                        .map_or(deadline, |(due, _)| due)
                        .min(listed.map_or(deadline, |at| at + seconds(TIMING.list_interval)))
                        .min(deadline);
                    let pause = wake.saturating_duration_since(Instant::now());
                    tokio::time::sleep(pause.min(Duration::from_secs(2))).await;
                }
            }
        }
        if !is_status(&self.session.current_status(), "main_menu") {
            let _ = self.session.quit_match().await;
        }
        Ok(self.summary(start.elapsed()))
    }

    /// Reads the lobby's page and admits the standard rooms at round two or
    /// before, while there is room for them.
    async fn admit(&mut self) -> Result<(), String> {
        let page = self.session.watch_scenes(true).await?;
        let active = self
            .rooms
            .values()
            .filter(|room| room.ended.is_none())
            .count();
        let mut free = self.capacity.saturating_sub(active);
        let scenes = page
            .get("scenes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut admitted = Vec::new();
        for scene in &scenes {
            if free == 0 {
                break;
            }
            let standard = scene.get("standard").and_then(Value::as_bool) == Some(true);
            let round = scene
                .get("round")
                .and_then(Value::as_i64)
                .unwrap_or(i64::MAX);
            let Some(scene_id) = scene
                .get("scene_id")
                .and_then(Value::as_i64)
                .and_then(|id| i32::try_from(id).ok())
            else {
                continue;
            };
            // The lobby lists the round being deployed, one past the round
            // the server reports; round 1 is recorded until round 2 fights.
            if !standard || round > 2 || self.rooms.contains_key(&scene_id) {
                continue;
            }
            self.rooms
                .insert(scene_id, Room::new(scene_id, Instant::now()));
            admitted.push(scene_id);
            free -= 1;
        }
        self.note(json!({"event": "list", "scenes": scenes.len(), "admitted": admitted}));
        Ok(())
    }

    async fn visit(&mut self, scene_id: i32) -> Result<(), String> {
        self.visits += 1;
        let started = Instant::now();
        let joined = self.session.watch_scene(scene_id).await?;
        if joined.get("joined").and_then(Value::as_bool) != Some(true) {
            let room = self.room(scene_id);
            room.ended = Some("join not answered");
            room.last_visit = Some(Instant::now());
            self.note(json!({"event": "end", "scene": scene_id, "reason": "join not answered"}));
            return Ok(());
        }
        let status = joined.get("status").cloned().unwrap_or(Value::Null);
        let live = status.get("live").cloned().unwrap_or(Value::Null);
        let seen = seen(&live, started);
        let free_until = self
            .rooms
            .values()
            .filter(|room| room.ended.is_none() && room.scene_id != scene_id)
            .map(|room| room.due(&TIMING, &self.table))
            .min();
        let table = self.table.clone();
        let capture = self
            .room(scene_id)
            .observe(seen, &TIMING, &table, free_until);
        let join_seconds = started.elapsed().as_secs_f64();
        let mut event = json!({
            "event": if capture.is_some() { "capture" } else { "probe" },
            "scene": scene_id,
            "live": live,
            "join_seconds": join_seconds,
        });
        if let Some(round) = capture {
            match self.capture(scene_id, round).await {
                Ok(capture) => event["capture"] = capture,
                Err(error) => event["error"] = json!(error),
            }
        }
        let room = self.room(scene_id);
        event["captured"] = json!(room.captured);
        event["missed"] = json!(room.missed);
        event["ended"] = json!(room.ended);
        event["visit_seconds"] = json!(started.elapsed().as_secs_f64());
        self.note(event);
        if !is_status(&self.session.current_status(), "main_menu") {
            self.session.quit_match().await?;
        }
        Ok(())
    }

    /// Stays until the spectator has entered `round`'s fight, or the match
    /// has finished, and saves the replay.
    async fn capture(&mut self, scene_id: i32, round: i32) -> Result<Value, String> {
        let started = Instant::now();
        let reached = self
            .session
            .wait_status(
                "the spectator entering the fight",
                seconds(TIMING.capture_timeout),
                |status| {
                    let client = status
                        .get("round_count")
                        .and_then(Value::as_i64)
                        .unwrap_or(0);
                    let fighting = status.get("fighting").and_then(Value::as_bool) == Some(true);
                    let finished = status.get("finished").and_then(Value::as_bool) == Some(true);
                    !is_status(status, "spectating")
                        || finished
                        || client > i64::from(round)
                        || (client == i64::from(round) && fighting)
                },
            )
            .await?;
        if !is_status(&reached, "spectating") {
            return Err(format!("left the match before round {round}: {reached}"));
        }
        let output = slice_path(&self.out, scene_id, round);
        if let Some(directory) = output.parent() {
            std::fs::create_dir_all(directory)
                .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
        }
        self.session.save_replay(Some(output.clone())).await?;
        let finished = reached.get("finished").and_then(Value::as_bool) == Some(true);
        let room = self.room(scene_id);
        room.captured.insert(round);
        room.files.push(output.clone());
        if finished {
            room.ended = Some("finished");
        }
        Ok(json!({
            "round": round,
            "output": output,
            "wait_seconds": started.elapsed().as_secs_f64(),
            "finished": finished,
        }))
    }

    fn room(&mut self, scene_id: i32) -> &mut Room {
        self.rooms
            .entry(scene_id)
            .or_insert_with(|| Room::new(scene_id, Instant::now()))
    }

    fn summary(&self, elapsed: Duration) -> Value {
        json!({
            "seconds": elapsed.as_secs_f64(),
            "visits": self.visits,
            "rooms": self.rooms.values().map(|room| json!({
                "scene": room.scene_id,
                "captured": room.captured,
                "missed": room.missed,
                "ended": room.ended,
                "files": room.files,
            })).collect::<Vec<_>>(),
        })
    }
}

/// What a visit's `live` says, as of `at`.
fn seen(live: &Value, at: Instant) -> Seen {
    let number = |key: &str| live.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let round = live
        .get("round")
        .and_then(Value::as_i64)
        .and_then(|round| i32::try_from(round).ok())
        .unwrap_or(0);
    let stage = match live.get("state").and_then(Value::as_str) {
        Some("deploy") => Stage::Deploy {
            elapsed: number("state_elapsed_seconds"),
            remaining: number("deploy_remaining_seconds"),
        },
        Some("fighting") => Stage::Fighting {
            elapsed: number("state_elapsed_seconds"),
        },
        Some("ending") => Stage::Ending,
        _ => Stage::Before,
    };
    Seen { at, round, stage }
}

/// A new file for a round's replay: a round saved twice keeps both.
fn slice_path(out: &Path, scene_id: i32, round: i32) -> PathBuf {
    let directory = out.join(scene_id.to_string());
    let first = directory.join(format!("r{round}.grbr"));
    if !first.exists() {
        return first;
    }
    let mut copy = 2_u32;
    loop {
        let path = directory.join(format!("r{round}-{copy}.grbr"));
        if !path.exists() {
            return path;
        }
        copy += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: Instant, seconds: f64) -> Instant {
        start + Duration::from_secs_f64(seconds)
    }

    fn deploy(start: Instant, round: i32, elapsed: f64) -> Seen {
        Seen {
            at: start,
            round,
            stage: Stage::Deploy {
                elapsed,
                remaining: 100.0 - elapsed,
            },
        }
    }

    fn none() -> DeployTable {
        DeployTable::new()
    }

    #[test]
    fn a_new_room_is_due_at_once() {
        let start = Instant::now();
        assert_eq!(Room::new(1, start).due(&TIMING, &none()), start);
    }

    #[test]
    fn a_young_deployment_is_left_until_late_in_it() {
        let start = Instant::now();
        let mut room = Room::new(1, start);
        let seen = deploy(start, 3, 20.0);
        assert_eq!(room.observe(seen, &TIMING, &none(), Some(start)), None);
        assert_eq!(room.due(&TIMING, &none()), at(start, 35.0));
    }

    #[test]
    fn a_round_that_deploys_quickly_is_captured_sooner() {
        let start = Instant::now();
        let table = DeployTable::from([(4, 33.0)]);
        let mut room = Room::new(1, start);
        room.observe(deploy(start, 3, 5.0), &TIMING, &table, Some(start));
        // Round 4 deploys by 33 s one time in ten: come back at 18 s in.
        assert_eq!(room.due(&TIMING, &table), at(start, 13.0));
    }

    #[test]
    fn a_late_deployment_captures_the_round_last_fought() {
        let start = Instant::now();
        let mut room = Room::new(1, start);
        room.observe(deploy(start, 1, 10.0), &TIMING, &none(), Some(start));
        let seen = deploy(at(start, 50.0), 1, 60.0);
        assert_eq!(room.observe(seen, &TIMING, &none(), Some(start)), Some(1));
    }

    #[test]
    fn a_deployment_about_to_end_is_captured_however_young() {
        let start = Instant::now();
        let mut room = Room::new(1, start);
        let seen = Seen {
            at: start,
            round: 1,
            stage: Stage::Deploy {
                elapsed: 10.0,
                remaining: 15.0,
            },
        };
        assert_eq!(room.observe(seen, &TIMING, &none(), Some(start)), Some(1));
    }

    #[test]
    fn a_capture_that_fits_before_the_next_visit_is_taken_now() {
        let start = Instant::now();
        let mut room = Room::new(1, start);
        room.observe(deploy(start, 1, 5.0), &TIMING, &none(), Some(start));
        // Deployed 20 s: waits up to 101 - 20 - 20 = 61 s, too long to take early.
        let young = deploy(at(start, 15.0), 1, 20.0);
        assert_eq!(room.observe(young, &TIMING, &none(), None), None);
        // Fought 85 s: waits up to 16 s, taken when 26 s are free.
        let fighting = Seen {
            at: at(start, 15.0),
            round: 1,
            stage: Stage::Fighting { elapsed: 85.0 },
        };
        assert_eq!(
            room.observe(fighting, &TIMING, &none(), Some(at(start, 35.0))),
            None
        );
        assert_eq!(
            room.observe(fighting, &TIMING, &none(), Some(at(start, 45.0))),
            Some(1)
        );
    }

    #[test]
    fn a_captured_round_waits_for_the_deployment_after_the_next_fight() {
        let start = Instant::now();
        let table = DeployTable::from([(3, 50.0), (4, 60.0)]);
        let mut room = Room::new(1, start);
        room.observe(deploy(start, 2, 20.0), &TIMING, &table, Some(start));
        room.captured.insert(2);
        // Round 3's deployment ends by 50 s at the soonest, a 20 s fight, and
        // round 4's is entered 45 s in.
        assert_eq!(room.due(&TIMING, &table), at(start, 30.0 + 20.0 + 45.0));
    }

    #[test]
    fn rounds_gone_past_are_missed() {
        let start = Instant::now();
        let mut room = Room::new(1, start);
        room.observe(deploy(start, 1, 10.0), &TIMING, &none(), Some(start));
        let seen = Seen {
            at: at(start, 300.0),
            round: 3,
            stage: Stage::Fighting { elapsed: 5.0 },
        };
        room.observe(seen, &TIMING, &none(), Some(start));
        assert_eq!(room.missed, BTreeSet::from([1, 2]));
    }

    #[test]
    fn a_room_first_seen_past_round_one_is_dropped() {
        let start = Instant::now();
        let mut room = Room::new(1, start);
        assert_eq!(
            room.observe(deploy(start, 2, 30.0), &TIMING, &none(), None),
            None
        );
        assert_eq!(room.ended, Some("admitted after round 1"));
    }

    #[test]
    fn round_one_deploying_after_round_zero_is_not_yet_capturable() {
        let start = Instant::now();
        let mut room = Room::new(1, start);
        assert_eq!(
            room.observe(deploy(start, 0, 90.0), &TIMING, &none(), None),
            None
        );
        assert!(room.missed.is_empty());
    }

    #[test]
    fn visits_keep_their_distance() {
        let start = Instant::now();
        let mut room = Room::new(1, start);
        room.observe(deploy(start, 1, 50.0), &TIMING, &none(), Some(start));
        assert_eq!(room.due(&TIMING, &none()), at(start, 10.0));
    }
}
