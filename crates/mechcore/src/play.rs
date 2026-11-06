//! `play`: writes the page that plays a fight back.
//!
//! A recording is played as it holds the fight. A layout or a fight document is
//! fought by the simulator first, and its timeline is kept in memory and laid
//! out for the page directly: writing a recording only to read it back would
//! cost the encoding and the reading for nothing, and `convert --to mcfr` is
//! what keeps one. The page is `mechcore_player`'s.
//!
//! From the command line the page is opened in the system's browser once it is
//! written, unless `--no-open` says not to.

use std::{
    collections::BTreeMap,
    path::Path,
    process::{Command, Stdio},
};

use mechcore_mcfr::{McfrReader, Recording, UnitPose};
use mechcore_simulation::{Record, SimulationResult};
use serde::Serialize;

use crate::{
    cli::{Args, Failure, Outcome, Verdict},
    kind::Kind,
};

pub(crate) const SCHEMA: &str = "mechcore.play-result";

/// What `play` answers: the page it wrote and the fight on it.
#[derive(Serialize)]
pub(crate) struct Played {
    schema: &'static str,
    page: String,
    /// The kind of the file played.
    kind: &'static str,
    /// Who fought it: `game` or `simulator`.
    producer: &'static str,
    ticks: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    rounds: Option<usize>,
    /// The seed the simulator fought with, for a fight it fought here.
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed_source: Option<&'static str>,
    /// Whether the system was asked to open the page and took the request.
    opened: bool,
}

/// Reads `play <file> [<page>] [--seed <i32>] [--no-open]` off a command line,
/// and opens the page it writes unless told not to.
///
/// # Errors
///
/// Returns a usage failure for a command the contract does not define, and
/// whatever [`play`] fails with.
pub(crate) fn run(mut arguments: Args) -> Outcome {
    let format = arguments.format()?;
    let seed = arguments.parsed::<i32>("--seed", "a signed 32-bit integer")?;
    let stay = arguments.flag("--no-open")?;
    let input = arguments.path("a file to play")?;
    let page = if arguments.is_empty() {
        None
    } else {
        Some(arguments.path("the page to write")?)
    };
    arguments.finish()?;
    let mut played = play(&input, page.as_deref(), seed)?;
    if !stay {
        // A page that cannot be opened is still written, and the answer says
        // which: not opening it is no failure of the command.
        match open(Path::new(&played.page)) {
            Ok(()) => played.opened = true,
            Err(error) => eprintln!(
                "mechcore: wrote {} but cannot open it: {error}",
                played.page
            ),
        }
    }
    crate::cli::emit(&played, format)?;
    Ok(Verdict::Yes)
}

/// Writes the page that plays `input` to `page`, or in the system temporary
/// directory under the input's name with an `.html` extension.
///
/// # Errors
///
/// Returns a refusal for a kind `play` does not take or a fight the simulator
/// does not fight, a usage failure for a seed the file does not take, and a
/// failure for a page that cannot be written.
pub(crate) fn play(
    input: &Path,
    page: Option<&Path>,
    seed: Option<i32>,
) -> Result<Played, Failure> {
    let (kind, bytes) = Kind::read(input)?;
    kind.require("play")?;
    let page = page.map_or_else(
        || {
            std::env::temp_dir()
                .join(input.file_name().unwrap_or_default())
                .with_extension("html")
        },
        Path::to_path_buf,
    );
    let fought = |result: Result<SimulationResult, mechcore_simulation::Error>| {
        let result = result.map_err(|error| Failure::unsimulated(&error))?;
        let recording = result
            .recording
            .ok_or_else(|| Failure::failed("the simulator kept no recording of the fight"))?;
        written(
            &recording,
            None,
            input,
            kind,
            &page,
            Some((result.seed, result.seed_source)),
        )
    };
    match kind {
        Kind::Mcfr => {
            if seed.is_some() {
                return Err(Failure::usage(
                    "a recording was fought already; --seed belongs to a layout",
                ));
            }
            let reader = McfrReader::open(input).map_err(|error| {
                Failure::refused(format!("cannot read {}: {error}", input.display()))
            })?;
            // A recording the game made may hold how it drew each unit.
            let poses = reader.instrument::<UnitPose>().map_err(|error| {
                Failure::refused(format!("cannot read {}'s poses: {error}", input.display()))
            })?;
            written(&reader, poses.as_deref(), input, kind, &page, None)
        }
        Kind::Layout => fought(mechcore_simulation::simulate_layout(
            input,
            Record::Memory,
            seed,
        )),
        Kind::Match => play_match(&bytes, input, &page, seed),
        Kind::Fight => {
            if seed.is_some() {
                return Err(Failure::usage(
                    "a fight document states its own seed; --seed belongs to a layout",
                ));
            }
            let fight = mechcore_document::fight::parse_yaml(&bytes).map_err(Failure::refused)?;
            let layout =
                mechcore_document::canonical_yaml(mechcore_document::fight::project(&fight))
                    .map_err(Failure::failed)?;
            fought(mechcore_simulation::simulate_document(
                layout.as_bytes(),
                Record::Memory,
                None,
            ))
        }
        other => Err(Failure::refused(format!(
            "play does not take a {} file; {}",
            other.name(),
            other.takes()
        ))),
    }
}

/// Projects each completed deployment, keeping each fight's timeline in memory.
fn play_match(
    bytes: &[u8],
    input: &Path,
    page: &Path,
    seed: Option<i32>,
) -> Result<Played, Failure> {
    if seed.is_some() {
        return Err(Failure::usage(
            "a match states its own seed; --seed belongs to a layout",
        ));
    }
    let stated = mechcore_document::opening::stated(bytes)
        .map_err(Failure::refused)?
        .ok_or_else(|| Failure::refused("the document holds no match"))?;
    crate::r#match::warn_build(&stated.build);
    let economy = mechcore_document::economy::Economy::embedded().map_err(Failure::failed)?;
    let reinforcement = mechcore_document::opening::verify(economy, &stated)
        .and_then(|opening| mechcore_document::reinforcement::verify(economy, &stated, &opening))
        .map_err(Failure::refused)?;
    let mut timelines = Vec::new();
    let mut research_history = [BTreeMap::new(), BTreeMap::new()];
    for (index, turn) in stated.turns.iter().enumerate() {
        // A trailing state opens deployment; a concession ends it without a fight.
        if (index + 1 == stated.turns.len() && !stated.ends_on_actions)
            || turn
                .actions
                .blue
                .iter()
                .chain(&turn.actions.red)
                .any(|action| matches!(action, mechcore_document::r#match::Action::Concede))
        {
            continue;
        }
        let deployment = crate::convert::project_match(
            economy,
            &turn.state,
            &turn.actions,
            turn.round,
            stated.map_id,
            stated.seed,
            reinforcement.declined(turn),
        )
        .map_err(Failure::refused)?;
        let sides = [&deployment.state.blue, &deployment.state.red];
        let rosters = match_rosters(
            economy,
            &deployment,
            turn,
            &stated,
            reinforcement.declined(turn),
            &mut research_history,
        )?;
        let yaml = mechcore_document::canonical_yaml(deployment.layout).map_err(Failure::failed)?;
        let result = mechcore_simulation::simulate_document(yaml.as_bytes(), Record::Memory, None)
            .map_err(|error| Failure::unsimulated(&error))?;
        let recording = result
            .recording
            .ok_or_else(|| Failure::failed("the simulator kept no recording of the fight"))?;
        let mut timeline = player_timeline(&recording, None).map_err(|error| {
            Failure::failed(format!("cannot read round {}: {error}", turn.round))
        })?;
        timeline.rosters = rosters;
        timeline.arsenals = sides.map(mechcore_player::arsenal);
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
        timelines.push(timeline);
    }
    if timelines.is_empty() {
        return Err(Failure::refused(
            "this match holds no completed deployment to play",
        ));
    }
    let title = input
        .file_stem()
        .map_or_else(|| "match".into(), |stem| stem.to_string_lossy());
    let html = mechcore_player::match_page(&timelines, &title)
        .map_err(|error| Failure::failed(format!("cannot write the page: {error}")))?;
    std::fs::write(page, html)
        .map_err(|error| Failure::failed(format!("cannot write {}: {error}", page.display())))?;
    Ok(Played {
        schema: SCHEMA,
        page: page.display().to_string(),
        kind: Kind::Match.name(),
        producer: "simulator",
        ticks: timelines.iter().map(|timeline| timeline.ticks).sum(),
        rounds: Some(timelines.len()),
        seed: Some(stated.seed),
        seed_source: Some("match"),
        opened: false,
    })
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

/// Lays a fight out for the page and writes the page.
fn written(
    recording: &dyn Recording,
    poses: Option<&[(u32, UnitPose)]>,
    input: &Path,
    kind: Kind,
    page: &Path,
    fought: Option<(i32, &'static str)>,
) -> Result<Played, Failure> {
    let mut timeline = player_timeline(recording, poses)
        .map_err(|error| Failure::failed(format!("cannot read the fight: {error}")))?;
    if kind == Kind::Layout {
        for core in &mut timeline.cores {
            core.begin = Some(1);
            core.maximum = Some(1);
        }
    }
    let title = input
        .file_stem()
        .map_or_else(|| "fight".into(), |stem| stem.to_string_lossy());
    let html = mechcore_player::page(&timeline, &title)
        .map_err(|error| Failure::failed(format!("cannot write the page: {error}")))?;
    std::fs::write(page, html)
        .map_err(|error| Failure::failed(format!("cannot write {}: {error}", page.display())))?;
    Ok(Played {
        schema: SCHEMA,
        page: page.display().to_string(),
        kind: kind.name(),
        producer: recording.producer().as_str(),
        ticks: recording.terminal_tick(),
        rounds: None,
        seed: fought.map(|(seed, _)| seed),
        seed_source: fought.map(|(_, source)| source),
        opened: false,
    })
}

/// Asks the system to open the page in its default browser, without waiting
/// for the browser.
fn open(page: &Path) -> std::io::Result<()> {
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
        .arg(page)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
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
}
