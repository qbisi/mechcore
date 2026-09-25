//! The `match` namespace: a battle document played as a match.
//!
//! `docs/spec/mechcore/cli.md` is the contract. A match is the document and
//! the turn file beside it: `<match>.yaml` holds the rounds that have been
//! played, `<match>.turn` holds the round in progress, and every operation
//! here takes the turn file's lock for the read and the write it does.
//!
//! A commit is a write. A side's decisions reach the document when that side
//! commits and not before, which is what lets two processes deploy at once
//! without reading each other's board.

use std::{fs, path::PathBuf};

use mechcore_document::{
    battle::{
        Action, Battle, BattleSide, DEFAULT_DEPLOY_TIME, Opening, OpeningOffer, SideState, State,
        Turn as BattleTurn, TurnActions,
    },
    economy::Economy,
    layout::StaticPlacement,
    opening::{self, Stream},
    transition,
};
use serde::Serialize;
use serde_json::Value;

use crate::{
    cli::{Args, Failure, Outcome, Verdict, emit},
    turn::{Held, Side, Turn},
};

const SCHEMA: &str = "mechcore.match.v1";

/// How often a `--wait` looks again, which is short enough to answer a round
/// promptly and long enough not to hold the lock against the side playing it.
const POLL: std::time::Duration = std::time::Duration::from_millis(200);

/// Dispatches one of the namespace's verbs.
///
/// # Errors
///
/// Returns a usage failure for a verb this namespace does not hold, and
/// whatever the verb returns otherwise.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let verb = arguments.operand("a verb: new, show, act or commit")?;
    let outcome = match verb.as_str() {
        "new" => new(arguments),
        "show" => show(arguments),
        "act" => act(arguments),
        "commit" => commit(arguments),
        other => Err(Failure::usage(format!(
            "match has no verb {other:?}; it has new, show, act and commit"
        ))),
    };
    // An error object names the operation, and a verb of this namespace is
    // one: a caller reads `match.act` and knows which request to rewrite.
    outcome.map_err(|failure| failure.at(format!("match.{verb}")))
}

/// Deals a match, or joins one already dealt.
fn new(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let seed = arguments.parsed::<i32>("--seed", "an integer")?;
    let map = arguments.parsed::<i32>("--map", "a map ID")?;
    let deploy_time = arguments.parsed::<i32>("--deploy-time", "a number of seconds")?;
    let loadout = arguments.value("--loadout")?.map(PathBuf::from);
    let path = arguments.path("a match document")?;
    arguments.finish()?;

    let mut held = Held::take(&turn_path(&path))?;
    let existing = held.read()?;
    let dealt = existing.is_some() || path.exists();
    let outcome = if dealt {
        join(&path, &mut held, existing, (seed, map, deploy_time))
    } else {
        deal(
            &path,
            &mut held,
            (seed, map, deploy_time),
            loadout.as_deref(),
        )
    };
    match outcome {
        Ok(game) => {
            let view = game.view(Some(game.side()), false)?;
            emit(&view, format)?;
            Ok(Verdict::Yes)
        }
        Err(failure) => {
            // Taking the lock creates the turn file. A match that was not
            // dealt leaves nothing behind, because a refused operation writes
            // no file.
            if !dealt {
                held.remove()?;
            }
            Err(failure)
        }
    }
}

/// Deals a match nobody has dealt: the header, the empty opening, and the
/// turn file that hands the caller blue.
fn deal(
    path: &std::path::Path,
    held: &mut Held,
    (seed, map, deploy_time): (Option<i32>, Option<i32>, Option<i32>),
    loadout: Option<&std::path::Path>,
) -> Result<Game, Failure> {
    let economy = Economy::embedded().map_err(Failure::failed)?;
    let maps = opening::maps().map_err(Failure::failed)?;
    let mut draw = Draw::new();
    let map_id = match map {
        Some(map) => map,
        None => *draw.one(&maps),
    };
    let seed = match seed {
        Some(seed) if seed < 0 => {
            return Err(Failure::refused(
                "a match seed is not negative; the opening deal is verified for 0 upwards",
            ));
        }
        Some(seed) => seed,
        None => draw.seed(),
    };
    let deploy_time = deploy_time.unwrap_or(DEFAULT_DEPLOY_TIME);
    if deploy_time <= 0 {
        return Err(Failure::refused(
            "a deployment time is a number of seconds a side can deploy in",
        ));
    }
    let dealt = opening::predict(&economy, seed, map_id).map_err(Failure::refused)?;
    let loadout = match loadout {
        Some(path) => read_loadout(path)?,
        None => economy.unit_technologies(),
    };
    // Each player brings a seed of its own, as the game's server hands one
    // out, which an officer that draws its hand-out draws from.
    let mut side = |offers: &[OpeningOffer], constructions: &[StaticPlacement]| BattleSide {
        opening: Opening {
            choose: None,
            offers: offers.to_vec(),
        },
        constructions: constructions.to_vec(),
        tech_loadout: loadout.clone(),
        seed: Some(draw.seed()),
    };
    let battle = Battle {
        game_build: mechcore_document::game_build().to_owned(),
        map_id,
        seed,
        deploy_time: Some(deploy_time),
        blue: side(&dealt.deal.blue, &dealt.constructions.blue),
        red: side(&dealt.deal.red, &dealt.constructions.red),
        turns: Vec::new(),
    };
    let turn = Turn::opening(0, [true, false], false);
    let mut game = Game {
        path: path.to_owned(),
        economy,
        battle,
        turn,
        side: Side::Blue,
        unresolved: None,
    };
    game.write(held)?;
    Ok(game)
}

/// Joins a match already dealt, which is the second caller and no other.
fn join(
    path: &std::path::Path,
    held: &mut Held,
    existing: Option<Turn>,
    (seed, map, deploy_time): (Option<i32>, Option<i32>, Option<i32>),
) -> Result<Game, Failure> {
    let mut game = Game::read(path, held, existing, Side::Blue)?;
    let stated = |name: &str, asked: Option<i32>, held: i32| match asked {
        Some(asked) if asked != held => Err(Failure::refused(format!(
            "this match states {name} {held}, and the caller names {asked}"
        ))),
        _ => Ok(()),
    };
    stated("seed", seed, game.battle.seed)?;
    stated("map", map, game.battle.map_id)?;
    stated("deploy-time", deploy_time, game.deploy_time())?;
    let Some(free) = Side::BOTH
        .into_iter()
        .find(|side| !game.turn.side(*side).given)
    else {
        return Err(Failure::refused(
            "both sides of this match are given; a match is played by two",
        ));
    };
    game.side = free;
    game.turn.side_mut(free).given = true;
    game.write_turn(held)?;
    Ok(game)
}

/// Answers one side's view of the match.
fn show(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let omniscient = arguments.flag("--omniscient")?;
    let wait = arguments.optional_number("--wait")?;
    let side = side_option(&mut arguments, !omniscient)?;
    let path = arguments.path("a match document")?;
    arguments.finish()?;

    let deadline = wait.map(|seconds| {
        (
            std::time::Instant::now(),
            seconds.map(std::time::Duration::from_secs_f64),
        )
    });
    loop {
        let view = {
            let mut held = Held::take(&turn_path(&path))?;
            let game = Game::read(&path, &mut held, None, side.unwrap_or(Side::Blue))?;
            let game = game.settle(&mut held)?;
            game.view(side, omniscient)?
        };
        let Some((since, bound)) = deadline else {
            emit(&view, format)?;
            return Ok(Verdict::Yes);
        };
        // A wait is over when the match is waiting for this side again, and a
        // wait that reaches its own bound answers the phase it is still in.
        let waiting = view.phase == Phase::Over
            || side.is_none_or(|side| !view.sides.of(side).committed && view.phase.decides());
        if waiting || bound.is_some_and(|bound| since.elapsed() >= bound) {
            emit(&view, format)?;
            return Ok(Verdict::Yes);
        }
        std::thread::sleep(POLL);
    }
}

/// Takes one decision for one side.
fn act(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let dry_run = arguments.flag("--dry-run")?;
    let side = side_option(&mut arguments, true)?.unwrap_or(Side::Blue);
    let path = arguments.path("a match document")?;
    let written = arguments.operand("a decision")?;
    arguments.finish()?;
    let decision: Action = serde_yaml::from_str(&written).map_err(|error| {
        Failure::usage(format!(
            "{written:?} is not a decision this build reads: {error}"
        ))
    })?;

    let mut held = Held::take(&turn_path(&path))?;
    let game = Game::read(&path, &mut held, None, side)?;
    let mut game = game.settle(&mut held)?;
    let events = game.take(side, &decision)?;
    // A dry run answers the same answer, so the decision is taken here and
    // only the writing is left out.
    game.turn.side_mut(side).decisions.push(decision);
    if !dry_run {
        game.write_turn(&mut held)?;
    }
    let mut view = game.view(Some(side), false)?;
    view.events = Some(events);
    emit(&view, format)?;
    Ok(Verdict::Yes)
}

/// Writes that side's decisions into the match, which is what playing them
/// means.
fn commit(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let side = side_option(&mut arguments, true)?.unwrap_or(Side::Blue);
    let path = arguments.path("a match document")?;
    arguments.finish()?;

    let mut held = Held::take(&turn_path(&path))?;
    let game = Game::read(&path, &mut held, None, side)?;
    let mut game = game.settle(&mut held)?;
    game.commit(side, &mut held)?;
    let game = game.settle(&mut held)?;
    emit(&game.view(Some(side), false)?, format)?;
    Ok(Verdict::Yes)
}

/// Reads `--side`, which every operation but `new` names itself by.
fn side_option(arguments: &mut Args, required: bool) -> Result<Option<Side>, Failure> {
    match arguments.value("--side")? {
        Some(name) => Side::parse(&name).map(Some),
        None if required => Err(Failure::usage(
            "expected --side blue|red: a process that lives for one operation \
             names the side it was given",
        )),
        None => Ok(None),
    }
}

/// The turn file that belongs to a match document.
fn turn_path(path: &std::path::Path) -> PathBuf {
    path.with_extension("turn")
}

/// Reads a technology loadout, which is one side's own choice of what its
/// units may research.
fn read_loadout(path: &std::path::Path) -> Result<Loadout, Failure> {
    let bytes = fs::read(path)
        .map_err(|error| Failure::failed(format!("cannot read {}: {error}", path.display())))?;
    mechcore_document::names::loadout::deserialize(serde_yaml::Deserializer::from_slice(&bytes))
        .map_err(|error| {
            Failure::refused(format!(
                "{} is not a technology loadout: {error}",
                path.display()
            ))
        })
}

/// What each unit of a side may research, keyed by unit ID.
type Loadout = std::collections::BTreeMap<i32, Vec<i32>>;

/// What phase a match is in, which says what it is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Opening,
    Deploy,
    Fight,
    Over,
}

impl Phase {
    /// Whether the match is waiting for decisions, which is the clock's phase
    /// and the one an `act` belongs to.
    const fn decides(self) -> bool {
        matches!(self, Self::Opening | Self::Deploy)
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Opening => "opening",
            Self::Deploy => "deploy",
            Self::Fight => "fight",
            Self::Over => "over",
        }
    }
}

/// One match, held open with its turn file locked.
struct Game {
    path: PathBuf,
    economy: Economy,
    battle: Battle,
    turn: Turn,
    /// The side this operation was given, which `new` decides and every other
    /// operation names.
    side: Side,
    /// What stopped the fight this match is due, when one is.
    unresolved: Option<String>,
}

impl Game {
    /// Reads the match that a document and its turn file state.
    ///
    /// A turn file that is missing or unreadable is rebuilt from the document
    /// at the round the document is in. What that loses is the round's
    /// uncommitted decisions and its clock, and nothing else: a commit is in
    /// the document, so a rebuild takes each side's commit from there.
    fn read(
        path: &std::path::Path,
        held: &mut Held,
        already: Option<Turn>,
        side: Side,
    ) -> Result<Self, Failure> {
        let economy = Economy::embedded().map_err(Failure::failed)?;
        let bytes = fs::read(path)
            .map_err(|error| Failure::failed(format!("cannot read {}: {error}", path.display())))?;
        let battle = mechcore_document::battle::read(&bytes)
            .map_err(Failure::refused)?
            .ok_or_else(|| {
                Failure::refused(format!("{} is not a battle document", path.display()))
            })?;
        let turn = match already {
            Some(turn) => Some(turn),
            None => held.read()?,
        };
        let mut game = Self {
            path: path.to_owned(),
            economy,
            battle,
            turn: Turn::opening(0, [true, true], true),
            side,
            unresolved: None,
        };
        game.turn = match turn {
            Some(turn) if turn.round == game.round() => turn,
            // A turn file for another round is as lost as none at all: the
            // document has moved on, and what the file held belonged to a
            // round that is over.
            _ => {
                let mut rebuilt = Turn::opening(game.round(), [true, true], true);
                for side in Side::BOTH {
                    rebuilt.side_mut(side).committed = game.written(side);
                }
                held.write(&rebuilt)?;
                rebuilt
            }
        };
        Ok(game)
    }

    /// The round in progress, which is the one the document has not both
    /// written the decisions of.
    fn round(&self) -> i32 {
        self.battle.turns.last().map_or(0, |turn| turn.round)
    }

    fn deploy_time(&self) -> i32 {
        self.battle.deploy_time.unwrap_or(DEFAULT_DEPLOY_TIME)
    }

    const fn side(&self) -> Side {
        self.side
    }

    /// Whether the document states this side's decisions for the round in
    /// progress, which is what a commit writes.
    ///
    /// Round zero says so exactly: an opening is one decision, and a side that
    /// committed it took one. A later round cannot tell a side that committed
    /// nothing from one that has not committed, which is why the turn file
    /// carries the answer and this is only asked when that file is gone.
    fn written(&self, side: Side) -> bool {
        match self.battle.turns.last() {
            None => self.opening(side).choose.is_some(),
            Some(turn) => !actions_of(&turn.actions, side).is_empty(),
        }
    }

    fn opening(&self, side: Side) -> &Opening {
        &self.battle_side(side).opening
    }

    fn battle_side(&self, side: Side) -> &BattleSide {
        match side {
            Side::Blue => &self.battle.blue,
            Side::Red => &self.battle.red,
        }
    }

    /// What the match is waiting for.
    fn phase(&self) -> Phase {
        if self.over() {
            return Phase::Over;
        }
        if Side::BOTH
            .into_iter()
            .all(|side| self.turn.side(side).committed)
        {
            return Phase::Fight;
        }
        if self.round() == 0 {
            Phase::Opening
        } else {
            Phase::Deploy
        }
    }

    /// Whether the match has ended: a side gave up, or a reactor core reached
    /// zero.
    fn over(&self) -> bool {
        let Some(turn) = self.battle.turns.last() else {
            return false;
        };
        let conceded = Side::BOTH
            .into_iter()
            .any(|side| actions_of(&turn.actions, side).contains(&Action::Concede));
        conceded
            || state_of(&turn.state, Side::Blue).reactor_core <= 0
            || state_of(&turn.state, Side::Red).reactor_core <= 0
    }

    /// Carries the match as far as it goes on its own: the clock ends it, and
    /// a round both sides have committed is fought.
    ///
    /// Every operation does this before it answers, so a fight no backend
    /// could resolve is tried again by the next caller rather than left for a
    /// caller that knows to ask.
    fn settle(mut self, held: &mut Held) -> Result<Self, Failure> {
        loop {
            match self.phase() {
                Phase::Over => {
                    // A match that is over takes its turn file with it, and
                    // an operation that finds one anyway takes it away again:
                    // what a reader wants afterwards is the document.
                    held.remove()?;
                    return Ok(self);
                }
                // The opening has no clock and nothing to fight.
                Phase::Opening => return Ok(self),
                Phase::Deploy => {
                    if self.turn.age()? <= f64::from(self.deploy_time()) {
                        return Ok(self);
                    }
                    self.run_out()?;
                }
                Phase::Fight => {
                    if let Err(unresolved) = self.fight(held) {
                        self.unresolved = Some(unresolved);
                        return Ok(self);
                    }
                }
            }
        }
    }

    /// Ends the match against every side that did not commit in time.
    ///
    /// A document has one way to say that a side lost a round it did not
    /// fight, and this platform writes it rather than inventing an action the
    /// game has no evidence for: the side that ran out of time gives up.
    fn run_out(&mut self) -> Result<(), Failure> {
        let out: Vec<Side> = Side::BOTH
            .into_iter()
            .filter(|side| !self.turn.side(*side).committed)
            .collect();
        for side in out {
            let round = self
                .battle
                .turns
                .last_mut()
                .expect("a clock runs only on a round the document holds");
            actions_of_mut(&mut round.actions, side).push(Action::Concede);
        }
        self.write_document()
    }

    /// Runs the round's fight, which opens the round after it.
    ///
    /// Round zero is the one round with none: the opening is not fought, and
    /// what it chose arrives as the first round opens, so this is where a
    /// match crosses from the opening into deployment.
    fn fight(&mut self, held: &mut Held) -> Result<(), String> {
        let round = self.round();
        if round > 0 {
            // A fight that ran and a fight that could not run end the same way
            // here: the round is not settled, and the answer says why.
            return Err(self.fought(round).unwrap_or_else(|reason| reason));
        }
        let mut states = Vec::new();
        for side in Side::BOTH {
            let opened = self
                .opened(side)
                .map_err(|failure| failure.reason().to_owned())?;
            let actions = self.committed_actions(side);
            // Nothing is declined in round zero, which deals no reinforcement,
            // and nothing has drawn from the side's own stream before it.
            let stream = self.battle_side(side).seed.map(Stream::seeded);
            states.push(
                transition::predict(
                    &self.economy,
                    round,
                    &opened,
                    &actions,
                    side.red(),
                    None,
                    stream,
                )
                .map_err(|unsettled| {
                    format!("round {round} {} is not settled: {unsettled}", side.name())
                })?,
            );
        }
        let [blue, red] = [states.remove(0), states.remove(0)];
        self.battle.turns.push(BattleTurn {
            round: round + 1,
            state: State {
                reinforce_offers: None,
                blue,
                red,
            },
            actions: TurnActions::default(),
        });
        let dealt = self.deal()?;
        if let Some(opened) = self.battle.turns.last_mut() {
            opened.state.reinforce_offers = dealt;
        }
        self.turn = Turn::opening(round + 1, self.turn.given(), false);
        self.write(held)
            .map_err(|failure| failure.reason().to_owned())
    }

    /// Deals the reinforcement offers of the round the match has just opened.
    ///
    /// The deal is stateful and the stream it runs on is the header's, so the
    /// match hands the whole document over rather than keeping a dealer
    /// between operations: a round is dealt the same way whichever process
    /// opened it, and a turn file that was lost changes nothing.
    fn deal(&self) -> Result<Option<Vec<i32>>, String> {
        let yaml = mechcore_document::battle::canonical_yaml(&self.battle)?;
        let stated = mechcore_document::opening::stated(yaml.as_bytes())?
            .ok_or("a match in progress is not a battle document")?;
        let opening = opening::verify(&self.economy, &stated)?;
        mechcore_document::reinforcement::deal_last_round(&self.economy, &stated, &opening)
    }

    /// Runs the round's fight and reads what it decided.
    ///
    /// The simulator fights it, over the layout the deployment-end position
    /// projects onto, and [`crate::outcome`] reads the recording for the five
    /// fields `battle.md` says a fight decides. Nothing here writes them into
    /// the next position yet: two of the five have no rule, so every fight
    /// ends at a named gap rather than at a guess. The recording is thrown
    /// away with the directory it was written in, because the same fight is
    /// run again from the match itself.
    ///
    /// # Errors
    ///
    /// Both halves of the answer are the same sentence to a caller: what came
    /// back settles nothing, and what went wrong stopped it earlier.
    fn fought(&self, round: i32) -> Result<String, String> {
        // Both sides have committed, so each one's position is the one its
        // decisions reached: the same position `show` answers with.
        let mut positions = Vec::new();
        for side in Side::BOTH {
            positions.push(
                self.position(side)
                    .map_err(|failure| failure.reason().to_owned())?,
            );
        }
        let [blue, red] = [positions.remove(0), positions.remove(0)];
        let state = State {
            reinforce_offers: None,
            blue,
            red,
        };
        let layout = mechcore_document::project::project(
            &state,
            round,
            self.battle.map_id,
            self.battle.seed,
        )?;
        let yaml = mechcore_document::canonical_yaml(layout)?;
        let directory = tempfile::tempdir()
            .map_err(|error| format!("cannot make room for the fight: {error}"))?;
        let recording = directory.path().join("fight.mcfr");
        mechcore_simulation::simulate_document(yaml.as_bytes(), Some(&recording), None)
            .map_err(|error| format!("round {round} is not fought: {error}"))?;
        let outcome = crate::outcome::read(&recording).map_err(|failure| {
            format!(
                "round {round} was fought and not read: {}",
                failure.reason()
            )
        })?;
        Ok(format!(
            "round {round} is fought and not settled: {}",
            outcome.unresolved.join("; ")
        ))
    }

    /// The position the round in progress opened with, before any decision.
    fn opened(&self, side: Side) -> Result<SideState, Failure> {
        if let Some(turn) = self.battle.turns.last() {
            return Ok(state_of(&turn.state, side).clone());
        }
        // The opening is taken from the position the header deals, which is
        // the map's reactor core and its constructions and nothing else.
        let core =
            opening::reactor_core(self.battle.map_id, side.seat()).map_err(Failure::refused)?;
        Ok(transition::before_opening(
            core,
            self.battle_side(side).constructions.clone(),
        ))
    }

    /// The decisions this side has committed to the round in progress.
    fn committed_actions(&self, side: Side) -> Vec<Action> {
        match self.battle.turns.last() {
            Some(turn) => actions_of(&turn.actions, side).to_vec(),
            None => self.opening(side).action().into_iter().collect(),
        }
    }

    /// This side's decisions in the round in progress, committed or not.
    fn decisions(&self, side: Side) -> Vec<Action> {
        if self.turn.side(side).committed {
            self.committed_actions(side)
        } else {
            self.turn.side(side).decisions.clone()
        }
    }

    /// This side's position in the round in progress: what the round opened
    /// with, and the decisions taken from it.
    fn position(&self, side: Side) -> Result<SideState, Failure> {
        transition::deployed(
            &self.economy,
            &self.opened(side)?,
            &self.decisions(side),
            side.red(),
            None,
        )
        .map_err(|unsettled| {
            Failure::failed(format!(
                "a decision this match holds is not settled: {unsettled}"
            ))
        })
    }

    /// Takes one decision, answering what it did, without keeping it.
    fn take(&self, side: Side, decision: &Action) -> Result<Vec<Value>, Failure> {
        match self.phase() {
            Phase::Opening | Phase::Deploy => {}
            phase => {
                return Err(Failure::refused(format!(
                    "this match is in the {} phase, which holds no decision",
                    phase.name()
                )));
            }
        }
        if self.turn.side(side).committed {
            return Err(Failure::refused(format!(
                "{} has committed round {}, and a side commits a round once",
                side.name(),
                self.round()
            )));
        }
        self.allowed(side, decision)?;
        let before = self.position(side)?;
        let mut placement = mechcore_document::landing::placement(side.red());
        let after =
            transition::step_placing(&self.economy, &before, decision, None, &mut placement)
                .map_err(|unsettled| Failure::refused(unsettled.to_string()))?;
        self.deployable(side, &after)?;
        Ok(events(&before, &after))
    }

    /// Refuses a position the board would not hold.
    ///
    /// A decision is settled by the transition, which says what it costs and
    /// what it leaves; where a formation may stand is the board's rule, and
    /// the layout compiler is where that rule is. So a decision is taken and
    /// then the position it left is projected and compiled, which is the same
    /// check a fight's layout passes before it is fought.
    ///
    /// Round zero places nothing, and a layout states a round from the first,
    /// so nothing is compiled there.
    fn deployable(&self, side: Side, position: &SideState) -> Result<(), Failure> {
        let round = self.round();
        if round == 0 {
            return Ok(());
        }
        let other = self.opened(side.other())?;
        let (blue, red) = match side {
            Side::Blue => (position.clone(), other),
            Side::Red => (other, position.clone()),
        };
        let state = State {
            reinforce_offers: None,
            blue,
            red,
        };
        let layout = mechcore_document::project::project(
            &state,
            round,
            self.battle.map_id,
            self.battle.seed,
        )
        .map_err(Failure::refused)?;
        mechcore_document::compile_layout(layout)
            .map(|_| ())
            .map_err(Failure::refused)
    }

    /// Refuses a decision the round does not hold, which the transition alone
    /// would not catch.
    ///
    /// The opening is dealt to the side privately, so a side may take one of
    /// its own four and no other; nothing else may be taken in round zero, and
    /// an opening may not be taken in any other round.
    fn allowed(&self, side: Side, decision: &Action) -> Result<(), Failure> {
        let opening = self.round() == 0;
        match decision {
            Action::ChooseAdvanceTeam {
                offer,
                id,
                specialist,
            } => {
                if !opening {
                    return Err(Failure::refused(
                        "an opening is round zero's decision, and this round is not round zero",
                    ));
                }
                let dealt = usize::try_from(*offer)
                    .ok()
                    .and_then(|at| self.opening(side).offers.get(at))
                    .ok_or_else(|| {
                        Failure::refused(format!(
                            "offer {offer} is not one of the four {} was dealt",
                            side.name()
                        ))
                    })?;
                if dealt.team != *id || dealt.specialist != *specialist {
                    let held = serde_json::to_string(dealt).unwrap_or_default();
                    return Err(Failure::refused(format!(
                        "offer {offer} holds {held}, and the decision names another"
                    )));
                }
                Ok(())
            }
            _ if opening => Err(Failure::refused(
                "round zero holds one decision, and it is the opening",
            )),
            Action::ChooseReinforceItem { offer, .. } => {
                let dealt = self
                    .battle
                    .turns
                    .last()
                    .and_then(|turn| turn.state.reinforce_offers.as_deref())
                    .unwrap_or_default();
                if *offer == mechcore_document::battle::DECLINED_OFFER {
                    return Ok(());
                }
                if usize::try_from(*offer).is_ok_and(|at| at < dealt.len()) {
                    return Ok(());
                }
                Err(Failure::refused(format!(
                    "offer {offer} is not one of the {} this round deals",
                    dealt.len()
                )))
            }
            _ => Ok(()),
        }
    }

    /// Writes this side's decisions into the document.
    fn commit(&mut self, side: Side, held: &mut Held) -> Result<(), Failure> {
        if !self.phase().decides() {
            return Err(Failure::refused(
                "this match is not waiting for decisions, and a commit writes them",
            ));
        }
        if self.turn.side(side).committed {
            return Err(Failure::refused(format!(
                "{} has committed round {}, and a side commits a round once",
                side.name(),
                self.round()
            )));
        }
        // A commit that cannot be made leaves the draft where it was, so
        // nothing is taken out of the round until it is going to be written.
        if self.round() == 0 {
            let [decision] = self.turn.side(side).decisions.as_slice() else {
                return Err(Failure::refused(
                    "an opening is one decision, and this side has taken another number of them",
                ));
            };
            let Action::ChooseAdvanceTeam { offer, .. } = decision else {
                return Err(Failure::refused("round zero's decision is the opening"));
            };
            let offer = *offer;
            self.turn.side_mut(side).decisions.clear();
            match side {
                Side::Blue => self.battle.blue.opening.choose = Some(offer),
                Side::Red => self.battle.red.opening.choose = Some(offer),
            }
        } else {
            let decisions = std::mem::take(&mut self.turn.side_mut(side).decisions);
            let round = self
                .battle
                .turns
                .last_mut()
                .expect("a round after the opening is one the document holds");
            *actions_of_mut(&mut round.actions, side) = decisions;
        }
        self.turn.side_mut(side).committed = true;
        self.write(held)
    }

    /// Writes the document and then the round in progress.
    ///
    /// The document goes first, always: it is the record, and a turn file that
    /// disagrees with it is rebuilt from it.
    fn write(&mut self, held: &mut Held) -> Result<(), Failure> {
        self.write_document()?;
        self.write_turn(held)
    }

    fn write_document(&self) -> Result<(), Failure> {
        let yaml = mechcore_document::battle::canonical_yaml(&self.battle)
            .map_err(|error| Failure::failed(format!("cannot write the match: {error}")))?;
        fs::write(&self.path, yaml).map_err(|error| {
            Failure::failed(format!("cannot write {}: {error}", self.path.display()))
        })
    }

    fn write_turn(&self, held: &mut Held) -> Result<(), Failure> {
        held.write(&self.turn)
    }

    /// What a caller may see of the match.
    fn view(&self, side: Option<Side>, omniscient: bool) -> Result<View, Failure> {
        let phase = self.phase();
        let remaining = if phase == Phase::Deploy {
            Some(f64::from(self.deploy_time()) - self.turn.age()?)
        } else {
            None
        };
        let mut sides = Vec::new();
        for each in Side::BOTH {
            let own = omniscient || side == Some(each);
            sides.push(self.side_view(each, own)?);
        }
        let [blue, red] = [sides.remove(0), sides.remove(0)];
        Ok(View {
            schema: SCHEMA,
            path: self.path.display().to_string(),
            side,
            phase,
            round: self.round(),
            game_build: mechcore_document::game_build(),
            map_id: self.battle.map_id,
            // The seed deals both openings and every reinforcement offer, so
            // a player that knew it would know what it is not dealt yet.
            seed: omniscient.then_some(self.battle.seed),
            deploy_time: self.deploy_time(),
            opened: self.turn.opened.clone(),
            rebuilt: self.turn.rebuilt,
            remaining,
            reinforce_offers: self
                .battle
                .turns
                .last()
                .and_then(|turn| turn.state.reinforce_offers.clone()),
            unresolved: self.unresolved.clone(),
            events: None,
            sides: Views { blue, red },
        })
    }

    /// One side as the caller may see it.
    ///
    /// A side sees its own in full. Of the other it sees the position the
    /// round opened with, less what `docs/spec/mechcore/cli.md` hides: the
    /// supply it holds, the shop it has unlocked, and the technologies it
    /// could research but has never fielded. Its round in progress is not
    /// there at all.
    fn side_view(&self, side: Side, own: bool) -> Result<SideView, Failure> {
        let position = if own {
            self.position(side)?
        } else {
            self.opened(side)?
        };
        let mut position = serde_json::to_value(&position)
            .map_err(|error| Failure::failed(format!("cannot write a position: {error}")))?;
        let loadout = if own {
            self.battle_side(side).tech_loadout.clone()
        } else {
            let fielded = self.fielded(side);
            if let Some(fields) = position.as_object_mut() {
                fields.remove("supply");
                fields.remove("shop");
            }
            self.battle_side(side)
                .tech_loadout
                .iter()
                .filter(|(unit, _)| {
                    mechcore_document::unit_type_from_id(**unit)
                        .is_some_and(|(name, _)| fielded.contains(name))
                })
                .map(|(unit, techs)| (*unit, techs.clone()))
                .collect()
        };
        Ok(SideView {
            given: self.turn.side(side).given,
            committed: self.turn.side(side).committed,
            position,
            tech_loadout: named_loadout(&loadout)?,
            rounds: self.played(side, own),
            decisions: own.then(|| self.decisions(side)),
            offers: own.then(|| self.opening(side).offers.clone()),
        })
    }

    /// The rounds this side has played, which are the rounds before the one
    /// in progress.
    ///
    /// A player sees of the other side the decisions that reached the position
    /// it is looking at, less the unlocks: a unit type unlocked and never
    /// bought reaches no board, so it stays hidden for good rather than until
    /// the round is over.
    fn played(&self, side: Side, own: bool) -> Vec<RoundView> {
        let mut rounds = Vec::new();
        let ended = self.round();
        // An opening is dealt to a side privately and reaches the board as
        // the first round opens, so the other side's is a round in progress
        // until then however early it was committed.
        if ended > 0 || (own && self.turn.side(side).committed) {
            rounds.push(RoundView {
                round: 0,
                decisions: self.opening(side).action().into_iter().collect(),
            });
        }
        for turn in &self.battle.turns {
            if turn.round >= ended {
                break;
            }
            rounds.push(RoundView {
                round: turn.round,
                decisions: actions_of(&turn.actions, side)
                    .iter()
                    .filter(|action| own || !matches!(action, Action::UnlockUnit { .. }))
                    .cloned()
                    .collect(),
            });
        }
        rounds
    }

    /// Every unit type this side has stood on the board, which is what its
    /// technology loadout is shown for.
    fn fielded(&self, side: Side) -> std::collections::BTreeSet<&str> {
        let mut fielded = std::collections::BTreeSet::new();
        for turn in &self.battle.turns {
            for unit in &state_of(&turn.state, side).units {
                fielded.insert(unit.unit.type_name.as_str());
            }
        }
        fielded
    }
}

/// What one operation answers.
#[derive(Serialize)]
struct View {
    schema: &'static str,
    #[serde(rename = "match")]
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    side: Option<Side>,
    phase: Phase,
    round: i32,
    game_build: &'static str,
    map_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i32>,
    deploy_time: i32,
    opened: String,
    rebuilt: bool,
    /// Seconds left to commit, which only a round being deployed has.
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reinforce_offers: Option<Vec<i32>>,
    /// What stopped a fight this match is due, when one is.
    #[serde(skip_serializing_if = "Option::is_none")]
    unresolved: Option<String>,
    /// What the decision just taken did, which only `act` answers.
    #[serde(skip_serializing_if = "Option::is_none")]
    events: Option<Vec<Value>>,
    sides: Views,
}

#[derive(Serialize)]
struct Views {
    blue: SideView,
    red: SideView,
}

impl Views {
    const fn of(&self, side: Side) -> &SideView {
        match side {
            Side::Blue => &self.blue,
            Side::Red => &self.red,
        }
    }
}

#[derive(Serialize)]
struct SideView {
    given: bool,
    committed: bool,
    position: Value,
    tech_loadout: Value,
    /// The rounds this side has played, in order.
    rounds: Vec<RoundView>,
    /// This side's uncommitted decisions, which only this side sees.
    #[serde(skip_serializing_if = "Option::is_none")]
    decisions: Option<Vec<Action>>,
    /// The four openings this side was dealt, which only this side sees.
    #[serde(skip_serializing_if = "Option::is_none")]
    offers: Option<Vec<OpeningOffer>>,
}

/// One round a side has played.
#[derive(Serialize)]
struct RoundView {
    round: i32,
    decisions: Vec<Action>,
}

/// What one decision did to a position, beyond leaving another one.
///
/// A caller learns the index a purchase created and where the board put it
/// without diffing two positions. What a decision leaves on the board is what
/// this reports; the supply it spent and the allowance it used are in the
/// position it answers with.
fn events(before: &SideState, after: &SideState) -> Vec<Value> {
    let index = |state: &SideState| -> std::collections::BTreeSet<i32> {
        state.units.iter().map(|unit| unit.unit.index).collect()
    };
    let (was, is) = (index(before), index(after));
    let mut events = Vec::new();
    for unit in &after.units {
        if was.contains(&unit.unit.index) {
            continue;
        }
        events.push(serde_json::json!({"created": {
            "index": unit.unit.index,
            "name": unit.unit.type_name,
            "position": unit.unit.position,
        }}));
    }
    for unit in &before.units {
        if !is.contains(&unit.unit.index) {
            events.push(serde_json::json!({"removed": {
                "index": unit.unit.index,
                "name": unit.unit.type_name,
            }}));
        }
    }
    events
}

fn state_of(state: &State, side: Side) -> &SideState {
    match side {
        Side::Blue => &state.blue,
        Side::Red => &state.red,
    }
}

fn actions_of(actions: &TurnActions, side: Side) -> &[Action] {
    match side {
        Side::Blue => &actions.blue,
        Side::Red => &actions.red,
    }
}

fn actions_of_mut(actions: &mut TurnActions, side: Side) -> &mut Vec<Action> {
    match side {
        Side::Blue => &mut actions.blue,
        Side::Red => &mut actions.red,
    }
}

/// A technology loadout written by name, as a document writes it.
fn named_loadout(loadout: &Loadout) -> Result<Value, Failure> {
    mechcore_document::names::loadout::serialize(loadout, serde_json::value::Serializer)
        .map_err(|error| Failure::failed(format!("cannot write a loadout: {error}")))
}

/// Where a seed and a map come from when nobody named one.
///
/// A match nobody configured is as reproducible as one somebody did, because
/// what is drawn is written into the header. Drawing it is not a rule of the
/// game, so this is the process's own clock mixed rather than the match's
/// stream.
struct Draw(u64);

impl Draw {
    fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_nanos()).unwrap_or(u64::MAX)
            });
        Self(nanos ^ (u64::from(std::process::id()) << 40))
    }

    /// One `splitmix64` value.
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn one<'a, T>(&mut self, from: &'a [T]) -> &'a T {
        let count = u64::try_from(from.len()).unwrap_or(1).max(1);
        let at = usize::try_from(self.next() % count).unwrap_or(0);
        &from[at]
    }

    /// A seed the opening deal is verified for, which is a non-negative one.
    fn seed(&mut self) -> i32 {
        i32::try_from(self.next() % u64::from(i32::MAX.unsigned_abs())).unwrap_or(0)
    }
}
