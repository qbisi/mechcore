//! Projects a match position onto the layout a fight simulates.
//!
//! `docs/spec/document/state.md` defines the projection: what a fight cannot observe is
//! dropped, most of what it can is copied, and three fields are translated.
//! A supply, a shop, a reinforcement offer and the two allocators do not
//! survive; formations, constructions, contraptions and the tower levels do,
//! and the units inherited from earlier rounds are the legacy ones.
//!
//! The three that are neither dropped nor copied are the Research Center's
//! blueprints, of which a layout keeps only the two enhancement chains, the
//! Energy Tower's skills, of which it keeps only the two a fight can see, and
//! the skill panel, of which it keeps what earlier releases left standing and
//! what this round released.

use crate::DocumentKind;
use crate::catalog::battle_skill_type_from_id;
use crate::layout::{
    BattleSkillEntry, BattleSkillRelease, FIGHT_VISIBLE_ENERGY_TOWER_SKILLS, Layout, Side,
    UnitSource,
};
use crate::r#match::{Action, Offers, SideState, SkillTarget, State, TurnActions};

/// Reads and projects a standalone state, preserving its optional map and seed.
/// Unit reinforcement names are interpreted in the state's stated round.
///
/// # Errors
///
/// Returns a malformed state or a side whose effects cannot be projected.
pub fn read_state(bytes: &[u8]) -> Result<Layout, String> {
    let standalone = crate::standalone::StandaloneState::from_yaml_bytes(bytes)?;
    let build = standalone.effective_build().to_owned();
    let crate::standalone::StandaloneBody::Round(round_body) = standalone.body else {
        return Err("state needs round >= 1".into());
    };
    Ok(Layout {
        kind: DocumentKind::Layout,
        game_build: build,
        map_id: standalone.map_id,
        seed: Some(standalone.seed),
        round: standalone.round,
        blue: project_side(&round_body.state.blue, "blue")?,
        red: project_side(&round_body.state.red, "red")?,
    })
}

/// A deployment state, its projected layout, and any projection warnings.
pub struct Deployment {
    pub state: State,
    pub layout: Layout,
    pub warnings: Vec<String>,
}

/// Projects an ordered deployment, warning when technology initialization
/// history cannot be represented by the resulting layout.
///
/// # Errors
///
/// Returns a deployment or projection error; warnings do not refuse a layout.
pub fn deployment(
    economy: &crate::economy::Economy,
    opening: &State,
    actions: &TurnActions,
    round: i32,
    map_id: i32,
    seed: i32,
) -> Result<Deployment, String> {
    let mut warnings = Vec::new();
    let mut deploy = |opened: &SideState, actions: &[Action], red| {
        let (position, side_warnings) = deploy_side(
            economy,
            opened,
            actions,
            red,
            opening.reinforce_offers.as_ref(),
        )?;
        let side = if red { "red" } else { "blue" };
        warnings.extend(
            side_warnings
                .into_iter()
                .map(|warning| format!("round {round} {side}: {warning}")),
        );
        Ok::<_, String>(position)
    };
    let state = State {
        random_state: opening.random_state,
        reinforce_pool: opening.reinforce_pool.clone(),
        reinforce_offers: opening.reinforce_offers.clone(),
        blue: deploy(&opening.blue, &actions.blue, false)?,
        red: deploy(&opening.red, &actions.red, true)?,
    };
    let layout = project(opening, &state, round, map_id, seed)?;
    Ok(Deployment {
        state,
        layout,
        warnings,
    })
}

/// Apply one side's decisions and identify history that a layout cannot carry.
/// Conversion and formatting use the same boundary as battle verification.
fn deploy_side(
    economy: &crate::economy::Economy,
    opened: &SideState,
    actions: &[Action],
    red: bool,
    offers: Option<&Offers>,
) -> Result<(SideState, Vec<String>), String> {
    let mut position = opened.clone();
    let mut history = InitializationHistory::default();
    for action in actions {
        let next = crate::transition::deployed(
            economy,
            &position,
            std::slice::from_ref(action),
            red,
            offers,
        )
        .map_err(|error| error.to_string())?;
        history.record(&position, &next, action);
        position = next;
    }
    let warnings = history.warnings(opened, &position);
    Ok((position, warnings))
}

/// Initialization effects that cannot be reconstructed from endpoint state.
/// Record these while stepping the actual history, before normalizing actions.
#[derive(Clone, Default)]
pub struct InitializationHistory {
    cloak_after_flank: std::collections::BTreeSet<i32>,
    landed_extra_weapons: bool,
}

impl InitializationHistory {
    pub fn record(&mut self, before: &SideState, after: &SideState, action: &Action) {
        if matches!(action, Action::UpgradeTechnology { tech: 3925, .. }) {
            self.cloak_after_flank.extend(
                before
                    .active_units()
                    .filter(|entry| {
                        entry.type_name == "phantom_ray" && entry.travelling == Some(true)
                    })
                    .map(|entry| entry.index),
            );
        }
        if matches!(action, Action::UpgradeTechnology { tech: 110_181, .. })
            && before.techs.contains(&4418)
        {
            self.landed_extra_weapons = true;
        }
        self.cloak_after_flank.retain(|index| {
            after
                .active_units()
                .any(|entry| entry.index == *index && entry.travelling == Some(true))
        });
    }

    #[must_use]
    pub fn warnings(&self, opened: &SideState, position: &SideState) -> Vec<String> {
        let mut warnings = Vec::new();
        if !self.cloak_after_flank.is_empty() {
            warnings.push(format!(
            "Stealth Cloak activated after flank deployment of Phantom Ray formations {:?}. The layout activates technologies before flank deployment and cannot preserve that native initialization order. The match retains the historical operation order instead of reconstructing the standard sequence.", self.cloak_after_flank
        ));
        }
        let affected = travelling_research_formations(opened, position, 4430, "void_eye");
        if position.officers.contains(&20021) && !affected.is_empty() {
            warnings.push(format!(
                "Aerial Mode acquired with Aerial Specialist and travelling Void Eye formations {affected:?}. Native life corrections depend on whether research follows flank deployment. State/layout does not preserve this initialization history, including after match normalization; the projected fight may differ."
            ));
        }
        let affected = travelling_research_formations(opened, position, 4228, "hound");
        if !affected.is_empty() {
            warnings.push(format!(
                "Fire Extinguisher acquired with travelling Hound formations {affected:?}. Native terrain-clearing activation depends on whether research follows flank deployment. State/layout does not preserve this initialization history, including after match normalization; the projected fight may differ."
            ));
        }
        if (opened.techs.contains(&4418) || self.landed_extra_weapons)
            && position.techs.contains(&110_181)
            && position.officers.contains(&20021)
        {
            let affected = position
                .active_units()
                .filter(|entry| entry.type_name == "wraith")
                .map(|entry| entry.index)
                .collect::<Vec<_>>();
            if !affected.is_empty() {
                warnings.push(format!(
                "Wraith formations {affected:?} have Floating Artillery Array, Aerial Specialist and Land Cruiser. Native extra-weapon officer corrections can retain cross-round initialization history or differ when Floating Artillery Array is researched after Land Cruiser in this round. State/layout does not preserve those corrections; the projected fight may differ."
            ));
            }
        }
        warnings
    }
}

/// Travelling formations exposed to a technology's acquisition this deployment.
fn travelling_research_formations(
    opening: &SideState,
    deployed: &SideState,
    technology: i32,
    unit_type: &str,
) -> Vec<i32> {
    if opening.techs.contains(&technology) || !deployed.techs.contains(&technology) {
        return Vec::new();
    }
    deployed
        .active_units()
        .filter(|entry| entry.type_name == unit_type && entry.travelling == Some(true))
        .map(|entry| entry.index)
        .collect()
}

/// Projects one round's position onto a layout: `state`, which the round
/// opened with as `opening` or reached from it.
///
/// # Errors
///
/// Returns an error when a released skill has no layout type or aims at a
/// target a layout cannot state.
pub fn project(
    opening: &State,
    state: &State,
    round: i32,
    map_id: i32,
    seed: i32,
) -> Result<Layout, String> {
    project_standalone(
        opening,
        state,
        round,
        crate::economy::game_build(),
        Some(map_id),
        Some(seed),
    )
}

/// Projects one round's position onto a layout for standalone `state` inputs
/// where `map_id` and `seed` may be omitted.
///
/// # Errors
///
/// Returns an error when a released skill has no layout type or aims at a
/// target a layout cannot state.
pub fn project_standalone(
    opening: &State,
    state: &State,
    round: i32,
    build: &str,
    map_id: Option<i32>,
    seed: Option<i32>,
) -> Result<Layout, String> {
    Ok(Layout {
        kind: DocumentKind::Layout,
        game_build: build.to_owned(),
        map_id,
        seed,
        round,
        blue: with_opening(&opening.blue, &state.blue, "blue")?,
        red: with_opening(&opening.red, &state.red, "red")?,
    })
}

/// Preserves original round-opening pose on moved `Legacy` units across
/// intermediate deployment states (`act` chaining).
pub fn record_legacy_pose(before: &SideState, after: &mut SideState) {
    let earlier: std::collections::BTreeMap<_, _> = before
        .active_units()
        .map(|unit| (unit.index, unit))
        .collect();
    for unit in &mut after.units {
        if !unit.recovered
            && unit.source == UnitSource::Legacy
            && let Some(prev) = earlier.get(&unit.index)
        {
            let orig_pos = prev.record_position.or(prev.position);
            let orig_rot = prev
                .record_rotated
                .unwrap_or_else(|| prev.rotated.unwrap_or(false));
            unit.record_position = orig_pos.filter(|pos| Some(*pos) != unit.position);
            unit.record_rotated =
                Some(orig_rot).filter(|rot| *rot != unit.rotated.unwrap_or(false));
        }
    }
}

/// Each unit's source and the side's recovered squads, from where the side's
/// unit allocator stood as its round opened, its officers' deliveries made.
/// The allocator had named every unit the side carried in and then the squads
/// its officers delivered, one for each officer whose `opening_unit` the round
/// activates, which take the indices just below it. A delivered squad the side
/// no longer holds was recovered; a unit of a higher index joined during the round.
/// In round 1 the advance team also joined: no unit came from an earlier round.
///
/// # Errors
///
/// Returns an error when the build's officer table cannot be read.
pub fn assign_sources(side: &mut Side, allocator: i32, round: i32) -> Result<(), String> {
    let economy = crate::economy::Economy::embedded()?;
    let deliveries = side
        .officers
        .iter()
        .filter(|officer| {
            economy
                .officer(**officer)
                .is_some_and(|row| row.opening_unit.is_some() && row.active_round.contains(&round))
        })
        .count();
    let delivered_from = allocator - i32::try_from(deliveries).unwrap_or(i32::MAX);
    for unit in &mut side.units {
        unit.source = if unit.index >= allocator || (round == 1 && unit.index < delivered_from) {
            UnitSource::Joined
        } else if unit.index >= delivered_from {
            UnitSource::Delivered
        } else {
            UnitSource::Legacy
        };
    }
    side.recovered = (delivered_from..allocator)
        .filter(|index| side.units.iter().all(|unit| unit.index != *index))
        .collect();
    Ok(())
}

fn with_opening(opening: &SideState, closed: &SideState, side_name: &str) -> Result<Side, String> {
    let mut side = project_side(closed, side_name)?;
    let earlier: std::collections::BTreeMap<_, _> = opening
        .units
        .iter()
        .map(|unit| (unit.index, unit))
        .collect();
    for unit in &closed.units {
        match earlier.get(&unit.index) {
            Some(before)
                if before.source == unit.source
                    && before.type_name == unit.type_name
                    && (!before.recovered || unit.recovered) => {}
            None if unit.index >= opening.next_index.unit && unit.source == UnitSource::Joined => {}
            _ => {
                return Err(format!(
                    "side {side_name} formation {} disagrees with its opening source",
                    unit.index
                ));
            }
        }
    }
    for index in earlier.keys() {
        if !closed.units.iter().any(|unit| unit.index == *index) {
            return Err(format!(
                "side {side_name} opening formation {index} has no closing unit or tombstone"
            ));
        }
    }
    for unit in &mut side.units {
        if unit.source == UnitSource::Legacy
            && let Some(before) = earlier.get(&unit.index)
        {
            let orig_pos = before.record_position.or(before.position);
            let orig_rot = before
                .record_rotated
                .unwrap_or_else(|| before.rotated.unwrap_or(false));
            unit.record_position = orig_pos.filter(|position| *position != unit.position);
            unit.record_rotated =
                Some(orig_rot).filter(|rotated| *rotated != unit.rotated.unwrap_or(false));
        }
    }
    Ok(side)
}

/// Projects every round of a match both ways and compiles each layout.
///
/// A round is projected from the position it opens with, and from the
/// position its decisions deploy onto, which is the layout `convert --to layout`
/// writes and a fight is run over. The opening puts every formation the round
/// inherited through the layout rules; the deployment puts what the round
/// itself did through them, releases included, which no opening carries.
/// Answers how many layouts compiled.
///
/// # Errors
///
/// Names the round and the projection that does not project, does not
/// deploy, or does not compile.
pub fn every_round(
    economy: &crate::economy::Economy,
    stated: &crate::opening::Stated,
) -> Result<usize, String> {
    let compiled = |opening: &State, state: &State, round: i32, which: &str| {
        project(opening, state, round, stated.map_id, stated.seed)
            .and_then(crate::compile_layout)
            .map_err(|error| format!("round {round}'s {which} position: {error}"))
    };
    let mut layouts = 0;
    for turn in &stated.turns {
        compiled(&turn.state, &turn.state, turn.round, "opening")?;
        let offers = turn.state.reinforce_offers.as_ref();
        let deployed = |state, actions, red| {
            crate::transition::deployed(economy, state, actions, red, offers)
                .map_err(|unsettled| format!("round {} does not deploy: {unsettled}", turn.round))
        };
        let state = State {
            random_state: turn.state.random_state,
            reinforce_pool: turn.state.reinforce_pool.clone(),
            reinforce_offers: turn.state.reinforce_offers.clone(),
            blue: deployed(&turn.state.blue, &turn.actions.blue, false)?,
            red: deployed(&turn.state.red, &turn.actions.red, true)?,
        };
        compiled(&turn.state, &state, turn.round, "deployed")?;
        layouts += 2;
    }
    Ok(layouts)
}

/// Projects one side of a position, every unit joining it during the round:
/// the units' sources are the round's opening position's to say.
///
/// # Errors
///
/// Returns an error when a released skill has no layout type or aims at a
/// target a layout cannot state.
pub fn project_side(state: &SideState, side_name: &str) -> Result<Side, String> {
    state
        .check_board(false)
        .map_err(|error| format!("side {side_name}: {error}"))?;
    let mut techs = state.techs.clone();
    techs.sort_unstable();
    let mut officers = state.officers.clone();
    officers.sort_unstable();

    let mut recovered: Vec<_> = state
        .units
        .iter()
        .filter(|unit| unit.recovered && unit.source == UnitSource::Delivered)
        .map(|unit| unit.index)
        .collect();
    recovered.sort_unstable();

    Ok(Side {
        officers,
        techs,
        blueprints: chain_blueprints(&state.blueprints),
        energy_tower_skills: energy_tower_skills(&state.energy_tower_skills),
        tower_strengthen_levels: tower_strengthen_levels(&state.tower_strengthen_levels),
        recovered,
        units: state
            .active_units()
            .map(crate::r#match::StateUnit::placement)
            .collect::<Result<_, _>>()?,
        constructions: state.constructions.clone(),
        contraptions: state.contraptions.clone(),
        battle_skills: project_battle_skills(state, side_name)?,
    })
}

/// The Officer list a layout carries.
///
/// The blueprints a layout carries: the Research Center's enhancement chains,
/// which a fight sees as the officer each hands out.
///
/// A blueprint that grants a commander skill reaches a fight only as that
/// skill's release, so a layout leaves it out.
#[must_use]
pub fn chain_blueprints(blueprints: &[i32]) -> Vec<i32> {
    let mut kept: Vec<i32> = blueprints
        .iter()
        .copied()
        .filter(|blueprint| crate::catalog::chain_officer(*blueprint).is_some())
        .collect();
    kept.sort_unstable();
    kept
}

/// The Energy Tower skills a layout carries: the two a fight can see.
///
/// The rest buy supply or discount a round's shopping, which is a state's
/// business and not a fight's.
#[must_use]
pub fn energy_tower_skills(activated: &[i32]) -> Vec<i32> {
    let mut kept: Vec<i32> = activated
        .iter()
        .copied()
        .filter(|skill| FIGHT_VISIBLE_ENERGY_TOWER_SKILLS.contains(skill))
        .collect();
    kept.sort_unstable();
    kept
}

/// The tower levels a layout carries, in normal form.
///
/// The list is copied, keyed the same way in both documents (`0 = energy_tower`,
/// `1 = research_center`). An all-zero list is what a layout omits, so it
/// projects onto nothing.
#[must_use]
pub fn tower_strengthen_levels(levels: &[i32]) -> Vec<i32> {
    if levels.iter().all(|level| *level == 0) {
        Vec::new()
    } else {
        levels.to_vec()
    }
}

/// The panel's battle skills a layout carries: one standing entry per object
/// a slot's earlier releases left standing, in normal-form order, then the
/// slots this round released, in release order.
fn project_battle_skills(
    state: &SideState,
    side_name: &str,
) -> Result<Vec<BattleSkillEntry>, String> {
    let mut standing = Vec::new();
    for slot in &state.battle_skills {
        for object in &slot.standing {
            object.require_skill(slot.id)?;
            standing.push(object.clone());
        }
    }
    standing.sort_by_cached_key(crate::layout::Standing::sort_key);
    let mut entries: Vec<BattleSkillEntry> = standing
        .into_iter()
        .map(BattleSkillEntry::Standing)
        .collect();
    entries.extend(
        project_releases(state, side_name)?
            .into_iter()
            .map(BattleSkillEntry::Release),
    );
    Ok(entries)
}

/// Keeps the panel entries this round released, in release order.
fn project_releases(state: &SideState, side_name: &str) -> Result<Vec<BattleSkillRelease>, String> {
    let mut released: Vec<_> = state
        .battle_skills
        .iter()
        .filter_map(|slot| Some((slot.battle_order()?, slot.id, slot.release.as_ref()?)))
        .collect();
    released.sort_by_key(|(order, _, _)| *order);
    released
        .into_iter()
        .map(|(_, id, release)| {
            let type_name = battle_skill_type_from_id(id).ok_or_else(|| {
                format!("side {side_name} released commander skill {id}, which has no layout type")
            })?;
            let SkillTarget::Area(positions) = &release.target else {
                return Err(format!(
                    "side {side_name} released commander skill {id} at one object, which a layout \
                     cannot state"
                ));
            };
            Ok(BattleSkillRelease {
                type_name: type_name.to_owned(),
                positions: positions.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn side() -> Side {
        serde_json::from_value(json!({"units": [
            {"name": "marksman", "index": 0, "position": {"x": 0, "y": -100}},
            {"name": "marksman", "index": 4, "position": {"x": 40, "y": -100}},
            {"name": "marksman", "index": 5, "position": {"x": 80, "y": -100}},
            {"name": "marksman", "index": 6, "position": {"x": 120, "y": -100}}
        ]}))
        .unwrap()
    }

    #[test]
    fn legacy_record_pose_comes_from_the_round_opening_by_index() {
        let units = side().units;
        let as_state = |unit: &crate::layout::UnitPlacement| {
            serde_yaml::from_value::<crate::r#match::StateUnit>(serde_yaml::to_value(unit).unwrap())
                .unwrap()
        };
        let opening = SideState {
            units: units[..3].iter().map(as_state).collect(),
            next_index: crate::r#match::NextIndex {
                unit: 6,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut moved = side();
        moved.units[0].rotated = Some(true);
        moved.units[1].position.x = 200;
        moved.units[1].rotated = Some(true);
        moved.units[3].position.x = 240;
        moved.units[3].rotated = Some(true);
        moved.units[3].source = UnitSource::Joined;
        let closed = SideState {
            units: moved.units.iter().map(as_state).collect(),
            ..Default::default()
        };
        let projected = with_opening(&opening, &closed, "blue").unwrap();
        assert_eq!(projected.units[1].record_position, Some(units[1].position));
        assert!(projected.units[0].record_position.is_none());
        assert_eq!(projected.units[0].record_rotated, Some(false));
        assert_eq!(projected.units[1].record_rotated, Some(false));
        assert!(projected.units[2].record_rotated.is_none());
        assert_eq!(projected.units[3].source, UnitSource::Joined);
        assert!(projected.units[3].record_position.is_none());
        assert!(projected.units[3].record_rotated.is_none());
    }

    #[test]
    fn deployment_warns_for_historical_cloak_after_flank_movement() {
        let economy = crate::economy::Economy::embedded().unwrap();
        let mut opening = State {
            blue: SideState {
                supply: 100_000,
                unlocked_units: vec![25],
                shop: crate::r#match::Allowances {
                    buys_remaining: 9,
                    ..Default::default()
                },
                ..Default::default()
            },
            red: SideState::default(),
            random_state: [0; 4],
            reinforce_pool: crate::r#match::PoolDelta::default(),
            reinforce_offers: None,
        };
        let bought = Action::BuyUnit {
            unit: 25,
            position: Some(crate::layout::Position { x: -100, y: -160 }),
            rotated: false,
        };
        let moved = Action::MoveUnit {
            index: 0,
            position: crate::layout::Position { x: 310, y: 50 },
            rotated: false,
        };
        let cloak = Action::UpgradeTechnology {
            unit: 25,
            tech: 3925,
        };
        opening.blue = crate::transition::deployed(
            economy,
            &opening.blue,
            std::slice::from_ref(&bought),
            false,
            None,
        )
        .unwrap();
        let check = |blue| {
            deployment(
                economy,
                &opening,
                &TurnActions { blue, red: vec![] },
                5,
                1012,
                1,
            )
            .unwrap()
        };
        let warned = check(vec![moved.clone(), cloak.clone()]);
        assert_eq!(warned.warnings.len(), 1);
        assert!(warned.warnings[0].contains("round 5 blue"));
        let canonical = check(vec![cloak.clone(), moved.clone()]);
        assert!(canonical.warnings.is_empty());
        // Both histories still produce a layout and the same projected state.
        assert_eq!(warned.layout, canonical.layout);
        let returned = Action::MoveUnit {
            index: 0,
            position: crate::layout::Position { x: -100, y: -160 },
            rotated: false,
        };
        assert!(check(vec![moved, cloak, returned]).warnings.is_empty());
    }

    #[test]
    fn aerial_mode_history_warning_survives_canonical_order_and_stays_scoped() {
        let economy = crate::economy::Economy::embedded().unwrap();
        let mut opening = State {
            blue: SideState {
                supply: 100_000,
                unlocked_units: vec![30],
                officers: vec![20021],
                next_index: crate::r#match::NextIndex {
                    unit: 1,
                    ..Default::default()
                },
                shop: crate::r#match::Allowances {
                    buys_remaining: 9,
                    ..Default::default()
                },
                units: vec![
                    serde_json::from_value(json!({"name": "void_eye", "index": 0,
                        "position": {"x": -100, "y": -160}}))
                    .unwrap(),
                ],
                ..Default::default()
            },
            red: SideState::default(),
            random_state: [0; 4],
            reinforce_pool: crate::r#match::PoolDelta::default(),
            reinforce_offers: None,
        };
        let bought = Action::BuyUnit {
            unit: 30,
            position: None,
            rotated: false,
        };
        let moved = Action::MoveUnit {
            index: 1,
            position: crate::layout::Position { x: 310, y: 50 },
            rotated: false,
        };
        let research = Action::UpgradeTechnology {
            unit: 30,
            tech: 4430,
        };
        let check = |opening: &State, blue| {
            deployment(
                economy,
                opening,
                &TurnActions { blue, red: vec![] },
                5,
                1012,
                1,
            )
            .unwrap()
        };
        let historical = check(
            &opening,
            vec![bought.clone(), moved.clone(), research.clone()],
        );
        let canonical = check(
            &opening,
            vec![research.clone(), bought.clone(), moved.clone()],
        );
        assert_eq!(historical.layout, canonical.layout);
        assert_eq!(canonical.warnings.len(), 1);
        assert!(canonical.warnings[0].contains("round 5 blue: Aerial Mode"));
        assert!(canonical.warnings[0].contains("[1]"));
        assert!(
            check(&opening, vec![research.clone(), bought.clone()])
                .warnings
                .is_empty()
        );
        opening.blue.officers.clear();
        assert!(
            check(
                &opening,
                vec![research.clone(), bought.clone(), moved.clone()]
            )
            .warnings
            .is_empty()
        );
        opening.blue.officers.push(20021);
        opening.blue.techs.push(4430);
        assert!(check(&opening, vec![bought, moved]).warnings.is_empty());
    }

    #[test]
    fn landed_wraith_extra_weapon_history_warning_stays_scoped() {
        let economy = crate::economy::Economy::embedded().unwrap();
        let opening = State {
            blue: SideState {
                supply: 100_000,
                unlocked_units: vec![18],
                officers: vec![20021],
                techs: vec![4418, 110_181],
                units: vec![
                    serde_json::from_value(json!({"name": "wraith", "index": 0,
                        "position": {"x": -100, "y": -160}}))
                    .unwrap(),
                ],
                ..Default::default()
            },
            ..Default::default()
        };
        let check = |opening: &State, blue| {
            deployment(
                economy,
                opening,
                &TurnActions { blue, red: vec![] },
                9,
                1012,
                1,
            )
            .unwrap()
        };
        let warned = check(&opening, vec![]);
        assert_eq!(warned.warnings.len(), 1);
        assert!(warned.warnings[0].contains("round 9 blue: Wraith formations [0]"));
        for absent in [20021, 4418, 110_181] {
            let mut other = opening.clone();
            other.blue.officers.retain(|id| *id != absent);
            other.blue.techs.retain(|id| *id != absent);
            assert!(check(&other, vec![]).warnings.is_empty());
        }
        let mut just_researched = opening.clone();
        just_researched.blue.techs.retain(|id| *id != 4418);
        assert!(
            check(
                &just_researched,
                vec![Action::UpgradeTechnology {
                    unit: 18,
                    tech: 4418
                }],
            )
            .warnings
            .is_empty()
        );
        let mut both_researched = opening.clone();
        both_researched.blue.techs.clear();
        let land_cruiser = Action::UpgradeTechnology {
            unit: 18,
            tech: 4418,
        };
        let artillery = Action::UpgradeTechnology {
            unit: 18,
            tech: 110_181,
        };
        assert_eq!(
            check(
                &both_researched,
                vec![land_cruiser.clone(), artillery.clone()]
            )
            .warnings
            .len(),
            1
        );
        assert!(
            check(&both_researched, vec![artillery, land_cruiser])
                .warnings
                .is_empty()
        );
        let mut no_wraith = opening;
        no_wraith.blue.units.clear();
        assert!(check(&no_wraith, vec![]).warnings.is_empty());
    }

    #[test]
    fn the_advance_team_joins_round_one_and_is_legacy_later() {
        let mut side = side();
        assign_sources(&mut side, 5, 1).unwrap();
        assert!(
            side.units
                .iter()
                .all(|unit| unit.source == UnitSource::Joined)
        );
        assert!(side.recovered.is_empty());

        assign_sources(&mut side, 6, 2).unwrap();
        assert_eq!(
            side.units
                .iter()
                .map(|unit| unit.source)
                .collect::<Vec<_>>(),
            [
                UnitSource::Legacy,
                UnitSource::Legacy,
                UnitSource::Legacy,
                UnitSource::Joined
            ]
        );
    }

    #[test]
    fn deliveries_and_recoveries_keep_their_allocator_boundary() {
        let mut side = side();
        side.officers = vec![20029]; // Marksman Specialist delivers in round 2.
        assign_sources(&mut side, 6, 2).unwrap();
        assert_eq!(
            side.units
                .iter()
                .map(|unit| unit.source)
                .collect::<Vec<_>>(),
            [
                UnitSource::Legacy,
                UnitSource::Legacy,
                UnitSource::Delivered,
                UnitSource::Joined
            ]
        );
        assert!(side.recovered.is_empty());
        side.units.retain(|unit| unit.index != 5);
        assign_sources(&mut side, 6, 2).unwrap();
        assert_eq!(side.recovered, [5]);
    }

    #[test]
    fn standalone_state_accepts_public_header_loadout_and_shop() {
        let base_side = "random_state: [1, 2, 3, 4]\n  reactor_core: 4500\n  supply: 200\n  unlocked_units: [fang]\n  next_index: {unit: 1, contraption: 0}\n  units: []\n";
        let yaml = format!(
            "kind: state\nbuild: another-version\nreinforce: false\nmax_round: 13\nmap_id: 1012\nseed: 42\nround: 3\nrandom_state: [5, 6, 7, 8]\nblue:\n  tech_loadout: full\n  shop: {{buys_remaining: 2, unlocks_remaining: 1, contraptions_remaining: 8}}\n  {base_side}red:\n  tech_loadout: {{}}\n  {base_side}"
        );
        let layout = super::read_state(yaml.as_bytes()).unwrap();
        assert_eq!(layout.game_build, "another-version");
        assert_eq!(layout.map_id, Some(1012));
        assert_eq!(layout.seed, Some(42));
        assert_eq!(layout.round, 3);

        let first_card = crate::names::card::name::<serde_yaml::Error>(
            crate::reinforcement::initial_members(42).unwrap()[0],
        )
        .unwrap();
        let expanded_state = yaml.replace("reinforce: false", "reinforce: true").replace(
            "round: 3\n",
            &format!("round: 3\nreinforce_pool: [{first_card}]\n"),
        );
        assert_eq!(
            super::read_state(expanded_state.as_bytes()).unwrap().round,
            3
        );
        let seedless_expanded = expanded_state.replace("seed: 42\n", "");
        assert_eq!(
            super::read_state(seedless_expanded.as_bytes())
                .unwrap()
                .round,
            3
        );

        for invalid in [
            yaml.replace("max_round: 13", "max_round: 0"),
            yaml.replace("tech_loadout: full", "tech_loadout: bad"),
            yaml.replace("buys_remaining: 2", "buys_remaining: -1"),
            yaml.replace(
                "round: 3\n",
                &format!("round: 3\nreinforce_pool: [{first_card}]\n"),
            ),
            expanded_state.replace(
                &format!("[{first_card}]"),
                &format!("[{first_card}, {first_card}]"),
            ),
        ] {
            assert!(super::read_state(invalid.as_bytes()).is_err(), "{invalid}");
        }
    }
}
