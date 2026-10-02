//! `SupportUnitSystem` and `SummonSystem`: a support skill's summons.
//!
//! A support skill's sub-effect lands as any skill's does and hands its side's
//! `TeamSupportUnitManager` a `SupportUnitCreator`. The creator updates with
//! `SupportUnitSystem`, after `InterceptSystem`, and creates its summons in
//! batches: the first on its first update, the landing tick, and the next each
//! `createInterval` later, until it has made `maxCount`. Each is its side's
//! unit from then on, at level 1 and in no formation, carrying what its side's
//! officers, technologies and Energy Tower skills write onto its type, and
//! standing where the skill landed or, when the skill summons several,
//! scattered around it by two draws of its side's stream. It then appears: for a second it is out of every
//! tree, neither updated nor counted, but already an obstacle to the units
//! moving around it. At the start of the tick a second on, before any module
//! updates, `SummonSystem.AddMech` lets it in, and an air-dropped one first
//! deals its whole life to everything its edge covers, of either side, and
//! loses as much as it dealt. `docs/rules/battle_skill.md` states the rule.

use super::*;
use crate::layout::Summon;

/// `SupportUnitCreator.APPEAR_DURATION`, one second, in ticks.
const APPEAR_TICKS: u64 = 20;

/// `SupportUnitSystem`'s creators and `SummonSystem`'s summons still
/// appearing.
#[derive(Default)]
pub(in crate::fight) struct SupportUnitSystem {
    /// Each side's `SupportUnitCreator`s still creating or alive.
    pub(in crate::fight) creators: Vec<Creator>,
    /// The summons created and not yet let into the fight, in the order they
    /// were created.
    pub(in crate::fight) appearing: Vec<Appearing>,
}

/// One `SupportUnitCreator` still creating or still alive.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Creator {
    team: u32,
    x_q32: i64,
    z_q32: i64,
    summon: Summon,
    /// `intervalTimeCounter`.
    counter: u32,
    /// `createCount`, which a summon's death never takes back.
    created: u32,
    /// `lifeTime`: the updates it has run.
    updates: u64,
}

/// A summon created and not yet let into the fight.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Appearing {
    actor: Actor,
    /// The tick at whose start it joins the fight.
    joins_on: u64,
    drop_damage: bool,
}

impl Creator {
    pub(in crate::fight) fn new(team: u32, x: i64, z: i64, summon: Summon) -> Self {
        Self {
            team,
            x_q32: space_to_q32(x),
            z_q32: space_to_q32(z),
            // Set so that the first update creates.
            counter: summon.interval_ticks,
            summon,
            created: 0,
            updates: 0,
        }
    }
}

impl Simulation {
    /// The summons still appearing, in the order they were made.
    pub(in crate::fight) fn appearing_actors(&self) -> impl Iterator<Item = &Actor> {
        self.support
            .appearing
            .iter()
            .map(|appearing| &appearing.actor)
    }

    /// `SummonSystem.HaveProcessingMech`: whether a summon of the side is still
    /// appearing.
    pub(in crate::fight) fn appearing_on(&self, team: u32) -> bool {
        self.support
            .appearing
            .iter()
            .any(|appearing| appearing.actor.placement.team == team)
    }

    /// `TeamSupportUnitManager.Update`, side by side: every creator, the
    /// latest first, runs one update, and one whose time is up is removed.
    pub(in crate::fight) fn step_support_units(
        &mut self,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let tick = step + 1;
        for team in [0_u32, 1] {
            let mut index = self.support.creators.len();
            while index > 0 {
                index -= 1;
                if self.support.creators[index].team != team {
                    continue;
                }
                let batch = {
                    let creator = &mut self.support.creators[index];
                    creator.updates += 1;
                    let mut batch = 0;
                    if creator.created < creator.summon.count {
                        creator.counter += 1;
                        if creator.counter >= creator.summon.interval_ticks {
                            creator.counter = 0;
                            batch = creator
                                .summon
                                .per_time
                                .min(creator.summon.count - creator.created);
                            creator.created += batch;
                        }
                    }
                    batch
                };
                let creator = self.support.creators[index].clone();
                for _ in 0..batch {
                    self.create_summon(&creator, tick, events)?;
                }
                if creator.updates >= creator.summon.updates {
                    self.support.creators.remove(index);
                }
            }
        }
        Ok(())
    }

    /// `SummonSystem.CreateMech` for one summon: scattered by two draws of
    /// its side's stream when the skill summons several, then
    /// `DoCreateMech`, whose `FightMech` draws its skills' first intervals,
    /// and `CreateMechDelay`, which keeps it out of the fight for a second.
    fn create_summon(
        &mut self,
        creator: &Creator,
        tick: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let team = creator.team;
        let (mut x_q32, mut z_q32) = (creator.x_q32, creator.z_q32);
        if creator.summon.random_range_q32 > 0 {
            let span = i32::try_from((creator.summon.random_range_q32 >> 32) * 100)
                .map_err(|_| Error::new("a summon's scatter exceeds i32"))?;
            for axis in [&mut x_q32, &mut z_q32] {
                let draw = self.side_random(team)?.next_in_range(span);
                *axis = axis.saturating_add(hundredths(draw));
            }
        }
        let rules = creator.summon.rules.clone();
        let unit_id = self.ids.next_unit;
        self.ids.next_unit += 1;
        let formation_id = self.ids.next_formation;
        self.ids.next_formation += 1;
        let placement = Placement {
            team,
            unit_id,
            formation_id,
            formation_index: -1,
            type_name: rules.type_name.clone(),
            world_x: q32_to_space_rounded(x_q32),
            world_z: q32_to_space_rounded(z_q32),
            rotation: if team == 0 { 0 } else { 180_000 },
            rotated: false,
            level: 1,
            exp: 0,
            corrections: creator.summon.corrections.clone(),
            lifesteal: None,
            auto_recovery: None,
            start_buffs: Vec::new(),
            travelling: false,
        };
        let mut actor = Actor::at_generated_position(placement, rules, x_q32, z_q32);
        actor.summoned = true;
        // `CreateMechDelay` locks the agent with nothing handed to it: it
        // reaches its first solve with no speed to take unless entering a
        // state hands it one, as `StopMove` does at once and `Move` only on
        // the update before a solve.
        actor.motion.next_max_speed_q32 = 0;
        actor.target_query_alive = false;
        let position = QVec3 {
            x: x_q32,
            y: space_to_q32(unit_height(actor.rules.domain)),
            z: z_q32,
        };
        events.push(event(
            Some(ObjectRef::new(ObjectKind::Unit, unit_id)),
            None,
            None,
            None,
            EventPayload::UnitCreated {
                team_id: team,
                formation_id,
                unit_type_id: actor.rules.unit_type_id,
                position,
            },
        ));
        self.support.appearing.push(Appearing {
            actor,
            joins_on: tick + APPEAR_TICKS,
            drop_damage: creator.summon.drop_damage,
        });
        Ok(())
    }

    /// `SummonSystem.AddMechDelay` for every summon whose second is up, in
    /// the order they were created: an air drop's damage, then
    /// `FightTeam.ActiveMech`, which puts it in its side's trees.
    pub(in crate::fight) fn join_summons(
        &mut self,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let tick = step + 1;
        while self
            .support
            .appearing
            .first()
            .is_some_and(|appearing| appearing.joins_on <= tick)
        {
            let Appearing {
                actor, drop_damage, ..
            } = self.support.appearing.remove(0);
            let unit_id = actor.placement.unit_id;
            let team = actor.placement.team;
            self.actors.insert(unit_id, actor);
            self.draw_first_intervals(FightActorRef::Unit(unit_id))?;
            if drop_damage && self.actors[&unit_id].rules.domain == UnitDomain::Ground {
                self.drop_damage(unit_id, events)?;
            }
            let actor = &self.actors[&unit_id];
            let (x_q32, z_q32, radius) = (actor.x_q32, actor.z_q32, actor.rules.collision_radius());
            for trees in [&mut self.target_quadtrees, &mut self.mech_quadtrees] {
                trees
                    .entry(team)
                    .or_insert_with(TargetActorQuadtree::new)
                    .insert(FightActorRef::Unit(unit_id), x_q32, z_q32, radius);
            }
        }
        Ok(())
    }

    /// `SupportUnitCreator.PerformAirDropDamage`: `SupportUnitDamageProvider`
    /// deals the summon's life to everything of either side whose edge its
    /// own radius covers, with no owner, and the summon then loses what they
    /// lost, as `FightCalculator.CalculateHitActorDamage` takes it: no rate
    /// on damage taken, and never more than it has.
    fn drop_damage(&mut self, unit_id: u64, events: &mut Vec<Event>) -> Result<()> {
        let actor = &self.actors[&unit_id];
        let team = actor.placement.team;
        let hit = DamageHit {
            // `PerformAirDropDamage` turns `IsInterceptByAdvancedEnergyShield`
            // off.
            crosses_shields: true,
            ..DamageHit::unowned(
                team,
                actor.life,
                (actor.x_q32, actor.z_q32),
                0,
                actor.rules.collision_radius(),
            )
        };
        let struck = self.perform_damage(hit, events)?;
        let total = struck.lost;
        self.record_ends(struck.ends, events);
        if total <= 0 {
            return Ok(());
        }
        let actor = self
            .actors
            .get_mut(&unit_id)
            .expect("the summon was just let in");
        let lost = total.min(actor.life);
        actor.life -= lost;
        if actor.life == 0 {
            return Err(Error::new(format!(
                "summoned unit {unit_id} dies of its own air drop, and whom that death counts \
                 for is not measured"
            )));
        }
        self.count_self_hit(unit_id, total)?;
        events.push(event(
            None,
            None,
            Some(team),
            Some(ObjectRef::new(ObjectKind::Unit, unit_id)),
            EventPayload::Damage {
                amount: i32::try_from(lost).map_err(|_| Error::new("damage exceeds i32"))?,
                skill_slot: None,
            },
        ));
        Ok(())
    }
}

/// A draw in hundredths of a metre as `FPoint` raw metres, `draw / 100` in
/// `FPoint` division, which rounds to the nearest: a scatter of 0.34 is a
/// raw unit above the truncated quotient, and one of -5.56 a raw unit below.
fn hundredths(draw: i32) -> i64 {
    let scaled = i64::from(draw) << 32;
    let half = if scaled < 0 { -50 } else { 50 };
    (scaled + half) / 100
}
