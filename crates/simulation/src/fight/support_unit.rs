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

use super::math::fpcs_sin_fastest;
use super::*;
use crate::layout::Summon;

/// `FPoint.Deg2Rad`, Q32.32.
const DEG_TO_RAD: i64 = 0x0477_D1A9;

/// A quarter turn, Q32.32 radians: `SinFastest` of an angle a quarter turn
/// on is its cosine.
const QUARTER_TURN: i64 = 0x1_921F_B544;

/// `SupportUnitCreator.APPEAR_DURATION`, one second, in ticks.
const APPEAR_TICKS: u64 = 20;

/// `SupportUnitSystem`'s creators and `SummonSystem`'s summons still
/// appearing.
#[derive(Default)]
pub(in crate::fight) struct SupportUnitSystem {
    /// `TeamSupportUnitManager.creators`, `AddCreator`'s: the production
    /// lines units carry, which never finish and never hold the fight.
    pub(in crate::fight) lines: Vec<Creator>,
    /// `TeamSupportUnitManager.temporaryCreator`, `AddTemporaryCreator`'s:
    /// the battle skills' creators still creating or alive, which hold the
    /// fight (`IsStepFinish`).
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
    /// The unit whose production line it is, `SupportUnitData.GetParent`:
    /// its makes stand at `offsets` from where that unit stands and faces.
    owner: Option<u64>,
    /// `GetPositionDatas`, Q32.32 metres right and forward of the owner.
    offsets: Vec<(i64, i64)>,
    /// `GetBatchMaxCount`: how many batches it makes in all, none for no
    /// bound.
    max_batch: u32,
    /// `createBatchCount`.
    batches: u32,
    /// `GetMaxAliveCount`: how many of its makes may stand at once, none for
    /// no bound.
    max_alive: u32,
    /// Its makes, `AddMech`'s: `aliveMechCount` counts those not dead.
    made: Vec<u64>,
    /// `intervalTimeCounter`.
    counter: u32,
    /// `createCount`, which a summon's death never takes back.
    created: u32,
    /// `lifeTime`: the updates it has run.
    updates: u64,
}

/// Which of a `TeamSupportUnitManager`'s lists a creator is in.
#[derive(Clone, Copy)]
enum List {
    Lines,
    Temporary,
}

impl SupportUnitSystem {
    fn list(&self, list: List) -> &Vec<Creator> {
        match list {
            List::Lines => &self.lines,
            List::Temporary => &self.creators,
        }
    }

    fn list_mut(&mut self, list: List) -> &mut Vec<Creator> {
        match list {
            List::Lines => &mut self.lines,
            List::Temporary => &mut self.creators,
        }
    }
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
            owner: None,
            offsets: Vec::new(),
            max_batch: 0,
            batches: 0,
            max_alive: 0,
            made: Vec::new(),
            created: 0,
            updates: 0,
        }
    }

    /// A production line's creator, as `SupportUnitEffectProvider` hands its
    /// owner's side one when the fight starts: it makes for as long as the
    /// fight lasts, bound by its batches and by how many of its makes live.
    pub(in crate::fight) fn production(
        owner: &Actor,
        production: &crate::layout::Production,
    ) -> Self {
        let line = &production.line;
        let summon = Summon {
            rules: production.rules.clone(),
            count: u32::MAX,
            per_time: line.per_time,
            interval_ticks: u32::try_from(seconds_q32_to_steps(line.interval_q32))
                .unwrap_or(u32::MAX),
            random_range_q32: 0,
            drop_damage: false,
            updates: u64::MAX,
            corrections: production.corrections.clone(),
        };
        Self {
            team: owner.placement.team,
            x_q32: owner.x_q32,
            z_q32: owner.z_q32,
            counter: summon.interval_ticks,
            summon,
            owner: Some(owner.placement.unit_id),
            offsets: line.offsets.clone(),
            max_batch: line.max_batch,
            batches: 0,
            max_alive: line.max_alive,
            made: Vec::new(),
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

    /// `TeamSupportUnitManager.Update`, side by side: the production lines,
    /// then the battle skills' creators, each list the latest first; every
    /// creator runs one update, and one whose time is up is removed.
    pub(in crate::fight) fn step_support_units(
        &mut self,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let tick = step + 1;
        for team in [0_u32, 1] {
            self.step_creators(List::Lines, team, tick, events)?;
            self.step_creators(List::Temporary, team, tick, events)?;
        }
        Ok(())
    }

    /// `TeamSupportUnitManager.UpdateCreators` over one list of a side's.
    fn step_creators(
        &mut self,
        list: List,
        team: u32,
        tick: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let mut index = self.support.list(list).len();
        while index > 0 {
            index -= 1;
            if self.support.list(list)[index].team != team {
                continue;
            }
            let alive = {
                let creator = &self.support.list(list)[index];
                let appearing = |id: u64| {
                    self.support
                        .appearing
                        .iter()
                        .any(|appearing| appearing.actor.placement.unit_id == id)
                };
                creator
                    .made
                    .iter()
                    .filter(|&&id| appearing(id) || self.actors.get(&id).is_some_and(Actor::alive))
                    .count()
            };
            let batch = {
                let creator = &mut self.support.list_mut(list)[index];
                // `IsBatchMax` stops it for good, before its life counts.
                if creator.max_batch > 0 && creator.batches >= creator.max_batch {
                    continue;
                }
                creator.updates += 1;
                let mut batch = 0;
                let full = creator.max_alive > 0
                    && alive >= usize::try_from(creator.max_alive).unwrap_or(usize::MAX);
                if !full && creator.created < creator.summon.count {
                    creator.counter += 1;
                    if creator.counter >= creator.summon.interval_ticks {
                        creator.counter = 0;
                        batch = creator
                            .summon
                            .per_time
                            .min(creator.summon.count - creator.created);
                        creator.created += batch;
                        creator.batches += 1;
                    }
                }
                batch
            };
            let creator = self.support.list(list)[index].clone();
            for member in 0..batch {
                let unit_id = self.create_summon(&creator, member, tick, events)?;
                self.support.list_mut(list)[index].made.push(unit_id);
            }
            if creator.updates >= creator.summon.updates {
                self.support.list_mut(list).remove(index);
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
        member: u32,
        tick: u64,
        events: &mut Vec<Event>,
    ) -> Result<u64> {
        let team = creator.team;
        let (mut x_q32, mut z_q32, facing) = match creator.owner {
            Some(owner) => {
                let (x, z, facing) = self.production_position(creator, owner, member)?;
                (x, z, Some(facing))
            }
            None => (creator.x_q32, creator.z_q32, None),
        };
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
            energy_shield: None,
            carried_shield: None,
            production: None,
            start_buffs: Vec::new(),
            ignored_buffs: Vec::new(),
            important: false,
            travelling: false,
        };
        let mut actor = Actor::at_generated_position(placement, rules, x_q32, z_q32);
        if let Some(facing) = facing {
            actor.face(facing);
        }
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
        Ok(unit_id)
    }

    /// Where a production line's make stands, and which way it faces:
    /// `SummonSystem.CreateMech` turns its offset, `UnitPositionDatas.
    /// GetPosition`'s next, by `FQuaternion.AngleAxis` of its owner's facing
    /// about the vertical, adds it to where the owner stands, and faces it as
    /// the owner faces.
    fn production_position(
        &self,
        creator: &Creator,
        owner: u64,
        member: u32,
    ) -> Result<(i64, i64, i64)> {
        let Some(owner_actor) = self.actors.get(&owner).filter(|actor| actor.alive()) else {
            return Err(Error::new(format!(
                "unit {owner}'s production line makes a unit after it died, which is not \
                 measured"
            )));
        };
        let index = usize::try_from(member).unwrap_or(usize::MAX) % creator.offsets.len().max(1);
        let Some(&(right, forward)) = creator.offsets.get(index) else {
            return Err(Error::new("a production line holds no offset"));
        };
        let facing = owner_actor.body_rotation_q32;
        let (x, z) = turn_about_vertical(facing, right, forward);
        Ok((
            owner_actor.x_q32.saturating_add(x),
            owner_actor.z_q32.saturating_add(z),
            facing,
        ))
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

/// `FQuaternion.AngleAxis(angle, FVector3.up) * (x, 0, z)`, in the build's
/// fixed point: the half angle's `SinFastest` is the quaternion's `y`, a
/// quarter turn on its `w`, and `FQuaternion.Transform` expands the product
/// term by term.
fn turn_about_vertical(angle_q32: i64, x: i64, z: i64) -> (i64, i64) {
    let half = q32_div(q32_mul(angle_q32, DEG_TO_RAD), 2 << 32);
    let qy = fpcs_sin_fastest(half);
    let w = fpcs_sin_fastest(half.saturating_add(QUARTER_TURN));
    let y2 = qy.saturating_mul(2);
    let yy = q32_mul(qy, y2);
    let wy = q32_mul(w, y2);
    let one = 1_i64 << 32;
    (
        q32_mul(x, one - yy) + q32_mul(z, wy),
        q32_mul(x, -wy) + q32_mul(z, one - yy),
    )
}

/// A draw in hundredths of a metre as `FPoint` raw metres, `draw / 100` in
/// `FPoint` division, which rounds to the nearest: a scatter of 0.34 is a
/// raw unit above the truncated quotient, and one of -5.56 a raw unit below.
fn hundredths(draw: i32) -> i64 {
    let scaled = i64::from(draw) << 32;
    let half = if scaled < 0 { -50 } else { 50 };
    (scaled + half) / 100
}
