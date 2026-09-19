//! Projects a match position onto the layout a fight simulates.
//!
//! `docs/spec/document/state.md` defines the projection: what a fight cannot observe is
//! dropped, most of what it can is copied, and three fields are translated.
//! A supply, a shop, a reinforcement offer and the two allocators do not
//! survive; formations, constructions, contraptions, retained shields, terrain
//! and the tower levels do.
//!
//! The three that are neither dropped nor copied are the Research Center's
//! blueprint chains, which a layout says by naming the Officer each one hands
//! out, the Energy Tower's skills, of which a layout keeps only the two a fight
//! can see, and the skill panel, of which a layout keeps only what this round
//! released.

use crate::DocumentKind;
use crate::battle::{Release, SideState, SkillTarget, State};
use crate::catalog::battle_skill_type_from_id;
use crate::economy::Economy;
use crate::layout::{
    BattleSkillDefinition, FIGHT_VISIBLE_ENERGY_TOWER_SKILLS, Layout, Side, Sides, Techs,
};

/// Projects one round's position onto a layout.
///
/// # Errors
///
/// Returns an error when a released skill has no layout type or aims at a
/// target a layout cannot state.
pub fn project(
    economy: &Economy,
    state: &State,
    round: i32,
    map_id: i32,
    seed: i32,
) -> Result<Layout, String> {
    Ok(Layout {
        kind: DocumentKind::Layout,
        map_id: Some(map_id),
        seed: Some(seed),
        round,
        sides: Sides {
            blue: project_side(economy, &state.sides.blue, "blue")?,
            red: project_side(economy, &state.sides.red, "red")?,
        },
    })
}

/// Projects one side of a position.
///
/// # Errors
///
/// Returns an error when a released skill has no layout type or aims at a
/// target a layout cannot state.
pub fn project_side(
    economy: &Economy,
    state: &SideState,
    side_name: &str,
) -> Result<Side, String> {
    let mut units = state.techs.units.clone();
    units.sort_unstable();

    Ok(Side {
        techs: Techs {
            officers: officers(economy, &state.techs.officers, &state.blueprints),
            units,
        },
        energy_tower_skills: energy_tower_skills(&state.energy_tower_skills),
        tower_strengthen_levels: tower_strengthen_levels(&state.tower_strengthen_levels),
        formations: state
            .formations
            .iter()
            .map(|formation| formation.formation.clone())
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
/// A state keeps the Research Center's two enhancement chains in `blueprints`
/// and leaves their Officers out of its own list. A layout has no blueprint
/// list, so each chain arrives here as the Officer it hands out. Everything
/// else is copied, duplicates included: an Officer card that may be taken again
/// stacks.
#[must_use]
pub fn officers(economy: &Economy, held: &[i32], blueprints: &[i32]) -> Vec<i32> {
    let mut officers = held.to_vec();
    officers.extend(
        blueprints
            .iter()
            .filter_map(|blueprint| economy.blueprint_officer(*blueprint)),
    );
    officers.sort_unstable();
    officers
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
    /// `tests/layouts/tuff-replay-round-7.yaml` was captured at the end of
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
                "../../tests/grbr/2259_20260901--201562374_[crower]VS[[TUFF]MARLFAUX].grbr",
            )
            .unwrap(),
        )
        .unwrap();
        let turn = battle
            .turns
            .iter()
            .find(|turn| turn.round == 7)
            .expect("the replay reaches round 7");
        let stepped = |state: &crate::battle::SideState, actions: &[crate::battle::Action], red| {
            let mut placement = crate::landing::placement(red);
            actions.iter().fold(state.clone(), |position, action| {
                crate::transition::step_placing(&economy, &position, action, &mut placement)
                    .unwrap()
            })
        };
        let deployed = crate::battle::State {
            reinforce_offers: turn.state.reinforce_offers.clone(),
            sides: crate::battle::StateSides {
                blue: stepped(&turn.state.sides.blue, &turn.actions.blue, false),
                red: stepped(&turn.state.sides.red, &turn.actions.red, true),
            },
        };
        let projected =
            project(&economy, &deployed, turn.round, battle.map_id, battle.seed).unwrap();

        let bytes = std::fs::read("../../tests/layouts/tuff-replay-round-7.yaml").unwrap();
        let captured = crate::layout::parse_yaml(&bytes).unwrap();
        assert_eq!(
            crate::layout::canonical_yaml(projected).unwrap(),
            crate::layout::canonical_yaml(captured).unwrap()
        );
    }

    #[test]
    fn every_recorded_position_projects_onto_a_layout_that_compiles() {
        let economy = Economy::embedded().unwrap();
        let mut projected = 0;
        for entry in std::fs::read_dir("../../tests/grbr").expect("tracked replay directory") {
            let path = entry.expect("directory entry").path();
            if path.extension().is_none_or(|extension| extension != "grbr") {
                continue;
            }
            let Ok(battle) = battle_from_grbr(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            for turn in &battle.turns {
                let layout = project(
                    &economy,
                    &turn.state,
                    turn.round,
                    battle.map_id,
                    battle.seed,
                )
                .unwrap_or_else(|error| {
                    panic!("{} round {}: {error}", path.display(), turn.round)
                });
                compile_layout(layout).unwrap_or_else(|error| {
                    panic!("{} round {} does not compile: {error}", path.display(), turn.round)
                });
                projected += 1;
            }
        }
        assert_eq!(projected, 334);
    }
}
