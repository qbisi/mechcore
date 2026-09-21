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

/// Projects every round of a battle both ways and compiles each layout.
///
/// A round is projected from the position it opens with, and from the
/// position its decisions deploy onto, which is the layout `doc project`
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
    let compiled = |state: &State, round: i32, which: &str| {
        project(state, round, stated.map_id, stated.seed)
            .and_then(crate::compile_layout)
            .map_err(|error| format!("round {round}'s {which} position: {error}"))
    };
    let mut layouts = 0;
    for turn in &stated.turns {
        compiled(&turn.state, turn.round, "opening")?;
        let declined = deal
            .rounds
            .iter()
            .find(|dealt| dealt.round == turn.round)
            .map(|dealt| dealt.declined);
        let deployed = |state, actions, red| {
            crate::transition::deployed(economy, state, actions, red, declined)
                .map_err(|unsettled| format!("round {} does not deploy: {unsettled}", turn.round))
        };
        let state = State {
            reinforce_offers: turn.state.reinforce_offers.clone(),
            blue: deployed(&turn.state.blue, &turn.actions.blue, false)?,
            red: deployed(&turn.state.red, &turn.actions.red, true)?,
        };
        compiled(&state, turn.round, "deployed")?;
        layouts += 2;
    }
    Ok(layouts)
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
