//! Projects a match position onto the layout a fight simulates.
//!
//! `docs/spec/document/state.md` defines the projection: what a fight cannot observe is
//! dropped, most of what it can is copied, and three fields are translated.
//! A supply, a shop, a reinforcement offer and the two allocators do not
//! survive; formations, constructions, contraptions, retained shields, terrain
//! and the tower levels do.
//!
//! The three that are neither dropped nor copied are the Research Center's
//! blueprints, of which a layout keeps only the two enhancement chains, the
//! Energy Tower's skills, of which it keeps only the two a fight can see, and
//! the skill panel, of which it keeps only what this round released.

use crate::DocumentKind;
use crate::battle::{Release, SideState, SkillTarget, State};
use crate::catalog::battle_skill_type_from_id;
use crate::layout::{BattleSkillDefinition, FIGHT_VISIBLE_ENERGY_TOWER_SKILLS, Layout, Side};

/// Projects one round's position onto a layout.
///
/// # Errors
///
/// Returns an error when a released skill has no layout type or aims at a
/// target a layout cannot state.
pub fn project(state: &State, round: i32, map_id: i32, seed: i32) -> Result<Layout, String> {
    Ok(Layout {
        kind: DocumentKind::Layout,
        game_build: crate::economy::game_build().to_owned(),
        map_id: Some(map_id),
        seed: Some(seed),
        round,
        blue: project_side(&state.blue, "blue")?,
        red: project_side(&state.red, "red")?,
    })
}

/// Projects one side of a position.
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
        units: state
            .units
            .iter()
            .map(|formation| formation.unit.clone())
            .collect(),
        constructions: state.constructions.clone(),
        contraptions: state.contraptions.clone(),
        airdrop_shields: state.airdrop_shields.clone(),
        terrains: state.terrains.clone(),
        battle_skills: project_releases(state, side_name)?,
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

/// Keeps the panel entries this round released, in release order.
fn project_releases(
    state: &SideState,
    side_name: &str,
) -> Result<Vec<BattleSkillDefinition>, String> {
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
            Ok(BattleSkillDefinition {
                type_name: type_name.to_owned(),
                positions: positions.clone(),
            })
        })
        .collect()
}

#[cfg(all(test, feature = "convert"))]
mod tests {
    use super::project;
    use crate::compile::compile_layout;
    use crate::convert::battle_from_grbr;
    use crate::economy::Economy;

    /// Every position a tracked replay holds must project onto a legal layout.
    ///
    /// This is the widest check the projection has without a deployment
    /// executor: it resolves every unit, construction and contraption type,
    /// and puts every position through the footprint, region and collision
    /// rules `docs/spec/document/layout.md` states.
    /// A layout captured live pins the projection, composed with the round's
    /// deployment.
    ///
    /// `replay/layout/tuff-replay-round-7.yaml` was captured at the end of
    /// round 7's deployment, so it is `project(step*(state, actions))` rather
    /// than `project(state)`: the round's decisions, stepped in order from the
    /// position it opened with, and every formation handed out landed where the
    /// board puts it. The whole captured layout has to come out, both sides,
    /// board included. Blue activates both enhancement chains during the
    /// round, so its Officer list is right only if the deployment runs first.
    #[test]
    fn a_captured_layout_is_the_projection_of_the_stepped_round() {
        let economy = Economy::embedded().unwrap();
        let battle = battle_from_grbr(
            &std::fs::read(
                "../../replay/grbr/2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].grbr",
            )
            .unwrap(),
        )
        .unwrap();
        let turn = battle
            .turns
            .iter()
            .find(|turn| turn.round == 7)
            .expect("the replay reaches round 7");
        let pool = crate::opening::predict(&economy, battle.seed, battle.map_id)
            .unwrap()
            .initialization
            .unit_round_pool;
        let declined = crate::reinforcement::decline_supply(&economy, pool, turn.round).unwrap();
        let stepped = |state: &crate::battle::SideState, actions: &[crate::battle::Action], red| {
            let mut placement = crate::landing::placement(red);
            actions.iter().fold(state.clone(), |position, action| {
                crate::transition::step_placing(
                    &economy,
                    &position,
                    action,
                    Some(declined),
                    &mut placement,
                )
                .unwrap()
            })
        };
        let deployed = crate::battle::State {
            reinforce_offers: turn.state.reinforce_offers.clone(),
            blue: stepped(&turn.state.blue, &turn.actions.blue, false),
            red: stepped(&turn.state.red, &turn.actions.red, true),
        };
        let projected = project(&deployed, turn.round, battle.map_id, battle.seed).unwrap();

        let bytes = std::fs::read("../../replay/layout/tuff-replay-round-7.yaml").unwrap();
        let captured = crate::layout::parse_yaml(&bytes).unwrap();
        assert_eq!(
            crate::layout::canonical_yaml(projected).unwrap(),
            crate::layout::canonical_yaml(captured).unwrap()
        );
    }

    /// Every round of the tracked set projects from the position its
    /// decisions reached, which is the layout `doc project` writes and a
    /// fight is run over.
    ///
    /// The sibling test below projects each round's opening position, which no
    /// release ever reaches: a state segment that carried one would be refused
    /// as a position a round opens with. This one runs the round first, so it
    /// is the only check that puts what a deployment did through the layout
    /// rules.
    #[test]
    fn every_deployed_position_projects_onto_a_layout_that_compiles() {
        let economy = Economy::embedded().unwrap();
        let mut projected = 0;
        for entry in std::fs::read_dir("../../replay/grbr").expect("tracked replay directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let Ok(battle) = battle_from_grbr(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            let pool = crate::opening::predict(&economy, battle.seed, battle.map_id)
                .unwrap()
                .initialization
                .unit_round_pool;
            for turn in &battle.turns {
                let declined =
                    crate::reinforcement::decline_supply(&economy, pool, turn.round).unwrap();
                let deployed = |state, actions, red| {
                    crate::transition::deployed(&economy, state, actions, red, Some(declined))
                        .unwrap_or_else(|unsettled| {
                            panic!("{} round {}: {unsettled}", path.display(), turn.round)
                        })
                };
                let state = crate::battle::State {
                    reinforce_offers: turn.state.reinforce_offers.clone(),
                    blue: deployed(&turn.state.blue, &turn.actions.blue, false),
                    red: deployed(&turn.state.red, &turn.actions.red, true),
                };
                let layout = project(&state, turn.round, battle.map_id, battle.seed)
                    .unwrap_or_else(|error| {
                        panic!("{} round {}: {error}", path.display(), turn.round)
                    });
                compile_layout(layout).unwrap_or_else(|error| {
                    panic!(
                        "{} round {} does not compile: {error}",
                        path.display(),
                        turn.round
                    )
                });
                projected += 1;
            }
        }
        assert_eq!(projected, 334);
    }

    #[test]
    fn every_recorded_position_projects_onto_a_layout_that_compiles() {
        let mut projected = 0;
        for entry in std::fs::read_dir("../../replay/grbr").expect("tracked replay directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let Ok(battle) = battle_from_grbr(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            for turn in &battle.turns {
                let layout = project(&turn.state, turn.round, battle.map_id, battle.seed)
                    .unwrap_or_else(|error| {
                        panic!("{} round {}: {error}", path.display(), turn.round)
                    });
                compile_layout(layout).unwrap_or_else(|error| {
                    panic!(
                        "{} round {} does not compile: {error}",
                        path.display(),
                        turn.round
                    )
                });
                projected += 1;
            }
        }
        assert_eq!(projected, 334);
    }
}
