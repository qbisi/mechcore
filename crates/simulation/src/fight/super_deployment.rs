//! `SuperDeploymentSystem`: the units a side deployed into an ambush zone,
//! which open the fight travelling and arrive together.
//!
//! A travelling unit is on the field from the first tick, where its layout
//! put it: it counts alive, it can be found and struck, and it can die. It is
//! not updated (`FightCoreSystem.TeamUpdate` skips `FightMech.Update`) and it
//! has no RVO agent (`MotionController.EnterFight` skips
//! `RVOControllerFixed.Active`). `SuperDeploymentController.EnterTravel`
//! starts it on part of its life, and the side's controller heals it once a
//! second until the travel time is up. Then `FinishTranvel` activates each
//! unit's movement where it stands and takes it out of travel.
//! `docs/rules/super_deployment.md` is the rule.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::data::{Channel, Entry};
use crate::{Error, Result};

use super::{
    Actor, NATIVE_LOGIC_DELTA_Q32, Q32_ONE, Simulation,
    rvo::{fpoint_greater_or_equal, q32_div, q32_mul},
};

const DEFAULT_SUPER_DEPLOYMENT: &str = include_str!("../../../../config/super_deployment.yaml");

/// `Config`'s two numbers.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuperDeploymentConfig {
    #[allow(dead_code, reason = "the schema tag is checked on load")]
    schema: Schema,
    /// `superDeploymentTravelTime`, whole seconds.
    travel_time: i64,
    /// `superDeploymentLifeRate`, Q32.32.
    life_rate: i64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
enum Schema {
    #[serde(rename = "mechcore.super_deployment")]
    SuperDeployment,
}

impl SuperDeploymentConfig {
    fn load() -> Result<Self> {
        serde_yaml::from_str(DEFAULT_SUPER_DEPLOYMENT)
            .map_err(|error| Error::new(format!("cannot read the super deployment table: {error}")))
    }
}

/// One side's `SuperDeploymentController`.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Travel {
    /// `fightMeches`: the side's units still travelling, in the order the
    /// fight activated them.
    units: Vec<u64>,
    /// `travelTime`, Q32.32 seconds.
    travelled_q32: i64,
    /// `recoveryTime`, Q32.32 seconds.
    recovery_q32: i64,
    /// `TravelTime()`: the travel time scaled by the side's
    /// `superDeploymentTimeChangeRate`.
    duration_q32: i64,
    /// `superDeploymentLifeRate`.
    life_rate_q32: i64,
    /// What each unit's effect providers write, which
    /// `FightEffectSystem.ActiveEffect` enables only as the unit leaves its
    /// travel (`ExitTravel`): its armour.
    withheld: BTreeMap<u64, Vec<Entry>>,
}

/// `SuperDeploymentController.EnterTravel` for every travelling unit: each
/// side's controller, and each unit on the share of its life it starts with.
pub(in crate::fight) fn enter_travel(
    actors: &mut BTreeMap<u64, Actor>,
    travel_time_rates: &BTreeMap<u32, i64>,
) -> Result<BTreeMap<u32, Travel>> {
    let config = SuperDeploymentConfig::load()?;
    let mut travels = BTreeMap::<u32, Travel>::new();
    for (&unit_id, actor) in actors.iter_mut() {
        if !actor.placement.travelling {
            continue;
        }
        actor.travelling = true;
        actor.searched_attack = false;
        let withheld = actor
            .stats
            .overlays
            .channel(Channel::Unit)
            .take(crate::modifier::ARMOR_SOURCE);
        let max_life_q32 = actor.stats.max_life() << 32;
        actor.life = q32_mul(max_life_q32, config.life_rate) >> 32;
        travels
            .entry(actor.placement.team)
            .or_insert_with(|| Travel {
                units: Vec::new(),
                travelled_q32: 0,
                recovery_q32: 0,
                // `TravelTime()`: the time scaled by one plus the side's rate.
                duration_q32: q32_mul(
                    config.travel_time << 32,
                    Q32_ONE.saturating_add(
                        travel_time_rates
                            .get(&actor.placement.team)
                            .copied()
                            .unwrap_or(0),
                    ),
                ),
                life_rate_q32: config.life_rate,
                withheld: BTreeMap::new(),
            });
        let travel = travels
            .get_mut(&actor.placement.team)
            .expect("the side's travel was just made");
        travel.units.push(unit_id);
        if !withheld.is_empty() {
            travel.withheld.insert(unit_id, withheld);
        }
    }
    Ok(travels)
}

impl Simulation {
    /// `SuperDeploymentSystem.Update`: each side's controller, while it holds
    /// a unit still travelling. It counts the travel and the recovery time,
    /// heals every travelling unit once a second, and brings them all in once
    /// the travel time is up.
    pub(in crate::fight) fn step_super_deployment(&mut self) {
        let teams = self.travels.keys().copied().collect::<Vec<_>>();
        for team in teams {
            // `OnMechDead` takes a unit that died out of the list.
            let actors = &self.actors;
            let travel = self
                .travels
                .get_mut(&team)
                .expect("a side's travel is kept");
            travel.units.retain(|unit_id| actors[unit_id].alive());
            if travel.units.is_empty() {
                continue;
            }
            travel.travelled_q32 = travel.travelled_q32.saturating_add(NATIVE_LOGIC_DELTA_Q32);
            travel.recovery_q32 = travel.recovery_q32.saturating_add(NATIVE_LOGIC_DELTA_Q32);
            if fpoint_greater_or_equal(travel.recovery_q32, Q32_ONE) {
                travel.recovery_q32 -= Q32_ONE;
                let (units, life_rate, travel_time) = (
                    travel.units.clone(),
                    travel.life_rate_q32,
                    travel.duration_q32,
                );
                for unit_id in units {
                    let actor = self
                        .actors
                        .get_mut(&unit_id)
                        .expect("a travelling unit is an actor");
                    let max_life = actor.stats.max_life();
                    let healed =
                        q32_div(q32_mul(max_life << 32, Q32_ONE - life_rate), travel_time) >> 32;
                    actor.life = actor.life.saturating_add(healed).min(max_life);
                }
            }
            let travel = &self.travels[&team];
            if fpoint_greater_or_equal(travel.travelled_q32, travel.duration_q32) {
                self.finish_travel(team);
            }
        }
    }

    /// `FinishTranvel`: each unit's movement activated where it stands, and
    /// the unit taken out of travel (`ExitTravel`), its effects enabled
    /// (`FightEffectSystem.ActiveEffect`), in the list's order.
    fn finish_travel(&mut self, team: u32) {
        let travel = self
            .travels
            .get_mut(&team)
            .expect("a side's travel is kept");
        let units = std::mem::take(&mut travel.units);
        let mut withheld = std::mem::take(&mut travel.withheld);
        for unit_id in units {
            let actor = self
                .actors
                .get_mut(&unit_id)
                .expect("a travelling unit is an actor");
            for entry in withheld.remove(&unit_id).unwrap_or_default() {
                actor.stats.overlays.channel(Channel::Unit).write(entry);
            }
            actor.travelling = false;
            actor.motion.rvo_new_agent = true;
            actor.rvo_max_speed_q32 = actor.stats.move_speed_q32();
            actor.motion.next_max_speed_q32 = actor.rvo_max_speed_q32;
            actor.motion.rvo_tree_x_q32 = actor.x_q32;
            actor.motion.rvo_tree_z_q32 = actor.z_q32;
        }
    }
}
