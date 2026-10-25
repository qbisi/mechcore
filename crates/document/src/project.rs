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
use crate::r#match::{Action, Release, SideState, SkillTarget, State, TurnActions};

/// A deployment state, its projected layout, and any projection warnings.
pub struct Deployment {
    pub state: State,
    pub layout: Layout,
    pub warnings: Vec<String>,
}

/// Projects an ordered deployment, warning when historical Cloak acquisition
/// follows flank movement and cannot be represented by the resulting layout.
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
    declined: Option<i32>,
) -> Result<Deployment, String> {
    let mut warnings = Vec::new();
    let mut deploy = |opened: &SideState, actions: &[Action], red| {
        let mut position = opened.clone();
        let mut affected = std::collections::BTreeSet::new();
        for action in actions {
            let next = crate::transition::deployed(
                economy,
                &position,
                std::slice::from_ref(action),
                red,
                declined,
            )
            .map_err(|error| error.to_string())?;
            if matches!(action, Action::UpgradeTechnology { tech: 3925, .. }) {
                affected.extend(
                    position
                        .units
                        .iter()
                        .filter(|entry| {
                            entry.unit.type_name == "phantom_ray"
                                && entry.unit.travelling == Some(true)
                        })
                        .map(|entry| entry.unit.index),
                );
            }
            affected.retain(|index| {
                next.units
                    .iter()
                    .any(|entry| entry.unit.index == *index && entry.unit.travelling == Some(true))
            });
            position = next;
        }
        if !affected.is_empty() {
            let side = if red { "red" } else { "blue" };
            warnings.push(format!(
                "round {round} {side}: Stealth Cloak activated after flank deployment of Phantom Ray formations {affected:?}. The layout activates technologies before flank deployment and cannot preserve that native initialization order. Format the match to place research before movement."
            ));
        }
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
    Ok(Layout {
        kind: DocumentKind::Layout,
        game_build: crate::economy::game_build().to_owned(),
        map_id: Some(map_id),
        seed: Some(seed),
        round,
        blue: with_sources(project_side(&state.blue, "blue")?, &opening.blue, round)?,
        red: with_sources(project_side(&state.red, "red")?, &opening.red, round)?,
    })
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

fn with_sources(mut side: Side, opening: &SideState, round: i32) -> Result<Side, String> {
    assign_sources(&mut side, opening.next_index.unit, round)?;
    for unit in &mut side.units {
        let recorded = if unit.source == UnitSource::Legacy {
            opening
                .units
                .iter()
                .find(|entry| entry.unit.index == unit.index)
        } else {
            None
        };
        unit.record_position = recorded
            .map(|entry| entry.unit.position)
            .filter(|position| *position != unit.position);
        unit.record_rotated = recorded
            .map(|entry| entry.unit.rotated.unwrap_or(false))
            .filter(|rotated| *rotated != unit.rotated.unwrap_or(false));
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
    deal: &crate::reinforcement::Verified,
) -> Result<usize, String> {
    let compiled = |opening: &State, state: &State, round: i32, which: &str| {
        project(opening, state, round, stated.map_id, stated.seed)
            .and_then(crate::compile_layout)
            .map_err(|error| format!("round {round}'s {which} position: {error}"))
    };
    let mut layouts = 0;
    for turn in &stated.turns {
        compiled(&turn.state, &turn.state, turn.round, "opening")?;
        let declined = deal.declined(turn);
        let deployed = |state, actions, red| {
            crate::transition::deployed(economy, state, actions, red, declined)
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
    let mut techs = state.techs.clone();
    techs.sort_unstable();
    let mut officers = state.officers.clone();
    officers.sort_unstable();

    Ok(Side {
        officers,
        techs,
        blueprints: chain_blueprints(&state.blueprints),
        energy_tower_skills: energy_tower_skills(&state.energy_tower_skills),
        tower_strengthen_levels: tower_strengthen_levels(&state.tower_strengthen_levels),
        recovered: Vec::new(),
        units: state
            .units
            .iter()
            .map(|formation| formation.unit.clone())
            .collect(),
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
/// The list is copied, keyed the same way in both documents. An all-zero list
/// is what a layout omits, so it projects onto nothing.
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
    let mut released: Vec<(&Release, i32)> = state
        .battle_skills
        .iter()
        .filter_map(|slot| Some((slot.release.as_ref()?, slot.id)))
        .collect();
    released.sort_by_key(|(release, _)| release.order);
    released
        .into_iter()
        .filter(|(_, id)| {
            // A recovery is deployment's work: it takes one of the side's own
            // objects away and pays back what it cost before the fight, so the
            // fight sees nothing of it and a layout states nothing of it.
            !crate::ledger::RECOVERY_SKILLS.contains(id)
        })
        .map(|(release, id)| {
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
        let opening = SideState {
            units: units
                .iter()
                .map(|unit| crate::r#match::StateUnit {
                    unit: unit.clone(),
                    value: Some(100),
                    redeployed: false,
                })
                .collect(),
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
        let projected = with_sources(moved, &opening, 3).unwrap();
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
        let opening = State {
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
            level: None,
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
        let check = |blue| {
            deployment(
                economy,
                &opening,
                &TurnActions { blue, red: vec![] },
                5,
                1012,
                1,
                None,
            )
            .unwrap()
        };
        let warned = check(vec![bought.clone(), moved.clone(), cloak.clone()]);
        assert_eq!(warned.warnings.len(), 1);
        assert!(warned.warnings[0].contains("round 5 blue"));
        let canonical = check(vec![bought.clone(), cloak.clone(), moved.clone()]);
        assert!(canonical.warnings.is_empty());
        // Both histories still produce a layout and the same projected state.
        assert_eq!(warned.layout, canonical.layout);
        let returned = Action::MoveUnit {
            index: 0,
            position: crate::layout::Position { x: -100, y: -160 },
            rotated: false,
        };
        assert!(
            check(vec![bought, moved, cloak, returned])
                .warnings
                .is_empty()
        );
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
}
