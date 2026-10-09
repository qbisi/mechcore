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

use super::math::{fpcs_pow_fastest, fpcs_sin_fastest, q32_div};
use super::*;
use crate::layout::Summon;

/// `FPoint.Deg2Rad`, Q32.32.
const DEG_TO_RAD: i64 = 0x0477_D1A9;

/// A quarter turn, Q32.32 radians: `SinFastest` of an angle a quarter turn
/// on is its cosine.
const QUARTER_TURN: i64 = 0x1_921F_B544;

/// `IBEC_DeadSummon.DEAD_FACTOR`, 1.5: the power of the ratio of the radii a
/// dying unit's summons count by.
const DEAD_FACTOR: i64 = 0x1_8000_0000;

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
    /// What each side's buffs make a dying unit summon, by side and type id.
    pub(in crate::fight) death_summons: BTreeMap<(u32, u32, i64), crate::layout::DeathSummon>,
    /// The units that died this tick, in the order they died:
    /// `DeadEffectSystem.deadActors`, whose `OnDead` waits for that module's
    /// update.
    pub(in crate::fight) dying: Vec<u64>,
    /// The `unit_created` of what each unit that died this tick summoned: a
    /// technology's, to precede its `unit_died`, and its buffs', to follow
    /// it.
    pub(in crate::fight) summoned_events: BTreeMap<u64, (Vec<Event>, Vec<Event>)>,
}

impl SupportUnitSystem {
    /// The production lines units carry, and what the side's buffs make a
    /// dying unit summon; nothing created yet.
    pub(in crate::fight) fn new(
        lines: Vec<Creator>,
        layout: &crate::layout::CompiledLayout,
    ) -> Self {
        Self {
            lines,
            creators: Vec::new(),
            appearing: Vec::new(),
            death_summons: layout.death_summons.clone(),
            dying: Vec::new(),
            summoned_events: BTreeMap::new(),
        }
    }
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
    /// How many ticks a make takes to appear before it joins the fight.
    appear_ticks: u64,
    /// Whether a make takes its owner's level.
    parent_level: bool,
    /// Whether an offset turns with the owner's body rather than its root.
    body_frame: bool,
    /// Whether its makes come out of their owner, taking their places as
    /// they join.
    comes_out: bool,
    /// Whether its owner's support skill lets each batch out, and whether
    /// it holds the line locked.
    gate: Gate,
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

/// What a support skill makes of its production line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    /// No skill gates the line: an item's or a battle skill's.
    None,
    /// A support skill gates it, and it runs.
    Open,
    /// `isLocked`: its support skill locked it while a batch was due and the
    /// skill could not start; it does not count until unlocked.
    Locked,
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
    /// Where one that comes out of its owner stands as it joins.
    coming_out: Option<ComingOut>,
}

/// `SummonSystem.DelayedMechInfo` with `isPositionAdjust`: the owner, and
/// the make's offset from it, its row's whole metres and the make's draws,
/// Q32.32 metres right and forward of the owner's facing.
#[derive(Debug, Clone, Copy)]
struct ComingOut {
    owner: u64,
    right_q32: i64,
    forward_q32: i64,
}

impl Creator {
    /// The creator `MoveAbilitySummonSystem` hands a unit's side each time
    /// the unit begins to surface (`TeamSupportUnitManager.
    /// AddTemporaryCreator`): its line's, which `SupportUnitCreator.
    /// IsFinished` ends after its first update, as its source's
    /// `GetLifeTime` is -1.
    pub(in crate::fight) fn surfacing(
        owner: &Actor,
        production: &crate::layout::Production,
    ) -> Self {
        let mut creator = Self::production(owner, production);
        creator.summon.updates = 1;
        creator
    }

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
            appear_ticks: APPEAR_TICKS,
            parent_level: false,
            body_frame: false,
            comes_out: false,
            gate: Gate::None,
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
            // A line of no positions makes about its owner, scattered within
            // the owner's radius (`SpecialSupportUnitData.GetRandomRange`,
            // `IBuffTarget.GetRadius`).
            random_range_q32: if line.offsets.is_empty() {
                space_to_q32(owner.rules.collision_radius())
            } else {
                0
            },
            drop_damage: false,
            updates: u64::MAX,
            corrections: production.corrections.clone(),
            technology_disable: production.technology_disable.clone(),
            reactive_armor: production.reactive_armor,
        };
        Self {
            team: owner.placement.team,
            x_q32: owner.x_q32,
            z_q32: owner.z_q32,
            counter: summon.interval_ticks,
            summon,
            owner: Some(owner.placement.unit_id),
            offsets: line.offsets.clone(),
            appear_ticks: seconds_q32_to_steps(line.appear_q32),
            parent_level: line.parent_level,
            body_frame: line.body_frame,
            comes_out: line.arrival == crate::modifier::Arrival::ComesOut,
            gate: if line.gated { Gate::Open } else { Gate::None },
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

    /// The tree a solve built holds every appearing summon's agent where it
    /// stands, made before that solve.
    pub(in crate::fight) fn appearing_agents_built(&mut self) {
        for appearing in &mut self.support.appearing {
            appearing.actor.motion.rvo_new_agent = false;
        }
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
            // `SupportUnitCreator.Update` does nothing while locked.
            if self.support.list(list)[index].gate == Gate::Locked {
                continue;
            }
            let alive = self.creator_alive(&self.support.list(list)[index]);
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

    /// `aliveMechCount`: a creator's makes still appearing or alive.
    fn creator_alive(&self, creator: &Creator) -> usize {
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
    }

    /// `SupportSkillStartAttackChecker.Check` of a unit's support skill: no
    /// preemptive skill runs, its line's next update makes a batch
    /// (`SupportUnitCreator.PreCalculate`), and the main skill is at rest,
    /// which unlocks the line and starts the skill; a batch due while the main
    /// skill is not at rest locks the line instead.
    pub(in crate::fight) fn support_gate(&mut self, actor_id: u64) -> bool {
        let actor = &self.actors[&actor_id];
        if actor.skills.running_preemptive.is_some() || actor.skills.preemptive_active {
            return false;
        }
        let at_rest = self.main_skill_at_rest(actor_id);
        let Some(index) = self
            .support
            .lines
            .iter()
            .position(|creator| creator.owner == Some(actor_id) && creator.gate != Gate::None)
        else {
            return false;
        };
        let creator = &self.support.lines[index];
        let full = creator.max_alive > 0
            && self.creator_alive(creator)
                >= usize::try_from(creator.max_alive).unwrap_or(usize::MAX);
        let due = (creator.max_batch == 0 || creator.batches < creator.max_batch)
            && !full
            && creator.created < creator.summon.count
            && creator.counter + 1 >= creator.summon.interval_ticks;
        if !due {
            return false;
        }
        self.support.lines[index].gate = if at_rest { Gate::Open } else { Gate::Locked };
        at_rest
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
        let (x_q32, z_q32, facing, coming_out) = self.made_at(creator, member)?;
        let rules = creator.summon.rules.clone();
        // `CreateSummonMechInfo.level`: the owner's, for a make of
        // `DynamicMechLevel.Parent`.
        let level = match creator.owner {
            Some(owner) if creator.parent_level => self.actors[&owner].placement.level,
            _ => 1,
        };
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
            level,
            exp: 0,
            experience_rate: crate::data::ExperienceRate::default(),
            unit_experience_rate: crate::data::ExperienceRate::default(),
            corrections: creator.summon.corrections.clone(),
            lifesteal: None,
            auto_recovery: None,
            energy_shield: None,
            stealth: None,
            siege_mode: None,
            wreckage: None,
            sweep: None,
            distance_intensify: false,
            secondary_damage: None,
            dead_line: None,
            mech_group: None,
            move_ability_attack: None,
            reactive_armor: creator.summon.reactive_armor,
            move_ability_range_item: None,
            main_fire: None,
            interception: None,
            carried_shield: None,
            production: None,
            buff_sources: Vec::new(),
            ignored_buffs: Vec::new(),
            important: false,
            ignores_control_beam: false,
            travelling: false,
            extra_weapons: Vec::new(),
            technology_disable: creator.summon.technology_disable.clone(),
            dead_summon: None,
            surfacing: None,
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
        // Its agent is made with it, so the first tree built after reads its
        // position as zero, as any new agent's.
        actor.motion.rvo_new_agent = true;
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
        // `SummonSystem.DoCreateMech` makes its agent with it
        // (`MotionController.ActiveMoveFunction`); one that comes out has
        // none until it joins.
        if coming_out.is_none() {
            self.rvo.added_units.push(unit_id);
        }
        // `SummonSystem.DoCreateMech`: a summon that takes no time to appear
        // joins at once (`AddMech`) rather than by `CreateMechDelay`, its
        // skills' first intervals drawn before the next make's scatter.
        if creator.appear_ticks == 0 {
            self.let_in(actor, creator.summon.drop_damage, events)?;
            // It joins after this tick's updates, as a dying unit's summon
            // does: its clock reads its interval until its first.
            self.actors
                .get_mut(&unit_id)
                .expect("the summon has just joined")
                .skills
                .hold_attack_clocks();
        } else {
            self.support.appearing.push(Appearing {
                actor,
                joins_on: tick + creator.appear_ticks,
                drop_damage: creator.summon.drop_damage,
                coming_out,
            });
        }
        Ok(unit_id)
    }

    /// Where a make is made, which way it faces, and, for one that comes out
    /// of its owner, where it will stand.
    fn made_at(
        &mut self,
        creator: &Creator,
        member: u32,
    ) -> Result<(i64, i64, Option<i64>, Option<ComingOut>)> {
        match creator.owner {
            // `SummonSystem.CreateMechDelaySetPos` makes it where the owner
            // stands (`SupportUnitData.GetPosition`).
            Some(owner) if creator.comes_out => {
                let owner_actor = &self.actors[&owner];
                let (x, z, facing) = (
                    owner_actor.x_q32,
                    owner_actor.z_q32,
                    owner_actor.body_rotation_q32,
                );
                let coming_out = self.come_out(creator, owner, member)?;
                Ok((x, z, Some(facing), Some(coming_out)))
            }
            Some(owner) => {
                let (mut x, mut z, facing) = self.production_position(creator, owner, member)?;
                self.scatter(creator, &mut x, &mut z)?;
                Ok((x, z, Some(facing), None))
            }
            None => {
                let (mut x, mut z) = (creator.x_q32, creator.z_q32);
                self.scatter(creator, &mut x, &mut z)?;
                Ok((x, z, None, None))
            }
        }
    }

    /// A make that comes out of its owner: its offset, the row's next
    /// position in whole metres (`Vector2Int`), and two draws of its side's
    /// stream of up to a metre each way, a hundredth a step, x then z, made
    /// as it is (`SummonSystem.CreateMechDelaySetPos`).
    fn come_out(&mut self, creator: &Creator, owner: u64, member: u32) -> Result<ComingOut> {
        let index = usize::try_from(member).unwrap_or(usize::MAX) % creator.offsets.len().max(1);
        let (right, forward) = creator.offsets.get(index).copied().unwrap_or((0, 0));
        let mut drawn = [0_i64; 2];
        for axis in &mut drawn {
            *axis = hundredths(self.side_random(creator.team)?.next_in_range(100));
        }
        Ok(ComingOut {
            owner,
            right_q32: ((right >> 32) << 32).saturating_add(drawn[0]),
            forward_q32: ((forward >> 32) << 32).saturating_add(drawn[1]),
        })
    }

    /// Where a summon stands once scattered: a line of one position that may
    /// keep two alive by up to a metre (`SummonSystem.CreateMech` with
    /// `positionDatas`), and a creator with a random range within its whole
    /// metres, each two draws of its side's stream, x first.
    fn scatter(&mut self, creator: &Creator, x_q32: &mut i64, z_q32: &mut i64) -> Result<()> {
        let team = creator.team;
        // A line of one position that may keep two alive scatters each make
        // by up to a metre, a draw a hundredth of a metre each way, x first
        // (`SummonSystem.CreateMech` with `positionDatas`).
        if creator.owner.is_some() && creator.offsets.len() == 1 && creator.max_alive >= 2 {
            for axis in [&mut *x_q32, &mut *z_q32] {
                let draw = self.side_random(team)?.next_in_range(100);
                *axis = axis.saturating_add(hundredths(draw));
            }
        }
        if creator.summon.random_range_q32 > 0 {
            let span = i32::try_from((creator.summon.random_range_q32 >> 32) * 100)
                .map_err(|_| Error::new("a summon's scatter exceeds i32"))?;
            for axis in [&mut *x_q32, &mut *z_q32] {
                let draw = self.side_random(team)?.next_in_range(span);
                *axis = axis.saturating_add(hundredths(draw));
            }
        }
        Ok(())
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
                "unit {owner}'s production line makes a unit on the tick it died, before its \
                 `OnDead` takes the line away, which is not measured"
            )));
        };
        // A line of no positions makes where its owner stands
        // (`SpecialSupportUnitData.GetPosition`).
        let index = usize::try_from(member).unwrap_or(usize::MAX) % creator.offsets.len().max(1);
        let (right, forward) = creator.offsets.get(index).copied().unwrap_or((0, 0));
        let facing = if creator.body_frame {
            owner_actor
                .turret_rotation()
                .unwrap_or(owner_actor.body_rotation_q32)
        } else {
            owner_actor.body_rotation_q32
        };
        let (x, z) = turn_about_vertical(facing, Q32_ONE, right, forward);
        Ok((
            owner_actor.x_q32.saturating_add(x),
            owner_actor.z_q32.saturating_add(z),
            facing,
        ))
    }

    /// A dead unit's production lines go as `DeadEffectSystem` calls its
    /// `OnDead`: `FightEffectSystem.DeactiveEffect` takes its effects off, and
    /// its support skill's line leaves its side's creators
    /// (`SupportUnitSystem.RemoveSkillOwner`). A Tarantula's Spider Mine line
    /// makes nothing after it dies; the mines it made stay.
    pub(in crate::fight) fn drop_dead_owners_lines(&mut self) {
        let actors = &self.actors;
        self.support.lines.retain(|creator| {
            creator
                .owner
                .is_none_or(|owner| actors.get(&owner).is_some_and(Actor::alive))
        });
    }

    /// `SummonSystem.RemoveMech`, which a summon's `OnMechDead` raises as
    /// `DeadEffectSystem` calls its `OnDead`: a summon that died this tick is
    /// destroyed (`FightController.DestroyMech`), and
    /// `FightEffectSystem.ClearEffect` takes away what its side's officers,
    /// technologies and Energy Tower skills wrote onto it. A shot it fired
    /// that lands later strikes without them. A deployed unit is no summon,
    /// and keeps them.
    pub(in crate::fight) fn clear_dead_summons(&mut self) -> Result<()> {
        for actor in self.actors.values_mut() {
            if !actor.summoned || actor.alive() || actor.placement.corrections.is_empty() {
                continue;
            }
            for (channel, entry) in std::mem::take(&mut actor.placement.corrections) {
                actor.stats.overlays.channel(channel).withdraw(entry.source);
            }
            // Its technologies' providers are gone with the rest: a disable
            // that leaves it later writes nothing back.
            actor.placement.technology_disable.corrections.clear();
            actor.stats.refresh(&actor.rules)?;
        }
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
                mut actor,
                drop_damage,
                coming_out,
                ..
            } = self.support.appearing.remove(0);
            if let Some(coming_out) = coming_out {
                self.take_place(&mut actor, coming_out)?;
            }
            self.let_in(actor, drop_damage, events)?;
        }
        Ok(())
    }

    /// `SummonSystem.AddMechDelay` with `isPositionAdjust`: a make that comes
    /// out of its owner gets its agent (`MotionController.ActiveMoveFunction`)
    /// and stands at its offset turned by `FQuaternion.AngleAxis` of the
    /// owner's facing about the vertical, from where the owner stands, facing
    /// as it faces (`FightMech.UpdatePositionAndRotation`).
    fn take_place(&mut self, actor: &mut Actor, coming_out: ComingOut) -> Result<()> {
        let owner = self
            .actors
            .get(&coming_out.owner)
            .ok_or_else(|| Error::new("a make comes out of a unit that has left the fight"))?;
        let facing = owner.body_rotation_q32;
        let (x, z) = turn_about_vertical(
            facing,
            Q32_ONE,
            coming_out.right_q32,
            coming_out.forward_q32,
        );
        actor.set_position(owner.x_q32.saturating_add(x), owner.z_q32.saturating_add(z));
        actor.face(facing);
        actor.motion.rvo_new_agent = true;
        self.rvo.added_units.push(actor.placement.unit_id);
        Ok(())
    }

    /// `SummonSystem.AddMech`: the summon joins its side, an air-dropped one
    /// deals its drop, and `FightTeam.ActiveMech` puts it in its side's
    /// trees.
    fn let_in(&mut self, actor: Actor, drop_damage: bool, events: &mut Vec<Event>) -> Result<()> {
        let unit_id = actor.placement.unit_id;
        self.join(actor)?;
        if drop_damage && self.actors[&unit_id].rules.domain == UnitDomain::Ground {
            self.drop_damage(unit_id, events)?;
        }
        self.plant(unit_id);
        Ok(())
    }

    /// The summon in the fight and its side's update order, its skills'
    /// first intervals drawn.
    fn join(&mut self, actor: Actor) -> Result<()> {
        let unit_id = actor.placement.unit_id;
        self.actors.insert(unit_id, actor);
        self.unit_update_order.push(unit_id);
        // `FightEffectSystem` activates its effects as it joins.
        self.activate_reactive_armor(unit_id);
        self.draw_owner_first_intervals(FightActorRef::Unit(unit_id))
    }

    /// The summon in its side's trees.
    fn plant(&mut self, unit_id: u64) {
        let actor = &self.actors[&unit_id];
        let team = actor.placement.team;
        let (x_q32, z_q32, radius) = (actor.x_q32, actor.z_q32, actor.rules.collision_radius());
        for trees in [&mut self.target_quadtrees, &mut self.mech_quadtrees] {
            trees
                .entry(team)
                .or_insert_with(TargetActorQuadtree::new)
                .insert(FightActorRef::Unit(unit_id), x_q32, z_q32, radius);
        }
    }

    /// The units made this tick numbered as a recording numbers the units
    /// a snapshot finds new: by side, then where they stand, `z` and then
    /// `x`, each taking the next unit and formation in turn. They were made,
    /// and are recorded as made, in their own order.
    fn number_as_recorded(&mut self, made: &[u64]) -> BTreeMap<u64, (u64, u64)> {
        let mut sorted = made.to_vec();
        sorted.sort_by_key(|id| {
            let actor = &self.actors[id];
            (actor.placement.team, actor.z_q32, actor.x_q32)
        });
        let renamed = sorted
            .iter()
            .zip(made)
            .map(|(&old, &new)| (old, (new, self.actors[&new].placement.formation_id)))
            .collect::<BTreeMap<_, _>>();
        let mut moved = renamed
            .keys()
            .map(|id| self.actors.remove(id).expect("a summon just made"))
            .collect::<Vec<_>>();
        for actor in &mut moved {
            let (unit_id, formation_id) = renamed[&actor.placement.unit_id];
            actor.placement.unit_id = unit_id;
            actor.placement.formation_id = formation_id;
        }
        for actor in moved {
            self.actors.insert(actor.placement.unit_id, actor);
        }
        for id in &mut self.unit_update_order {
            if let Some(&(unit_id, _)) = renamed.get(id) {
                *id = unit_id;
            }
        }
        renamed
    }
}

/// What a recording names in events made before their units were numbered
/// as it numbers them: each unit made and its formation, by the name it was
/// made under.
fn rename_created(renamed: &BTreeMap<u64, (u64, u64)>, events: &mut [Event]) {
    {
        for event in events.iter_mut() {
            let Some(subject) = event
                .subject
                .filter(|subject| subject.kind == ObjectKind::Unit)
            else {
                continue;
            };
            let Some(&(unit_id, formation)) = renamed.get(&subject.id) else {
                continue;
            };
            if let EventPayload::UnitCreated { formation_id, .. } = &mut event.payload {
                event.subject = Some(ObjectRef::new(ObjectKind::Unit, unit_id));
                *formation_id = formation;
            }
        }
    }
}

impl Simulation {
    /// `BuffManager.OnMechDead` of every unit that died this tick running a
    /// buff that summons, as `DeadEffectSystem` calls its `OnDead`, and each
    /// such buff's `IBEC_DeadSummon.OnMechDead`: a unit a buff itself summoned
    /// (`MechCreateType.ParasiticalSummon`) summons nothing, nor one of
    /// another domain than the summon. The buff's source summons
    /// `Max(1, (dead radius / summon radius) ^ 1.5)` units of its type
    /// (`DEAD_FACTOR`), the whole part, where the dead unit stood, scattered
    /// within its radius, each joining its side at once. A Marksman killed
    /// under Replicate leaves 7 Crawlers, four to the 1.5 being 7.99999998.
    ///
    /// A unit whose technology summons where it dies (`DeadSummonTech`)
    /// summons too, parasitic or not, as `DeadSummonController.
    /// PerformDeadEffect` does: Mechanical Division leaves five Crawlers
    /// where its Steel Ball died.
    pub(in crate::fight) fn summon_from_the_dead(&mut self) -> Result<()> {
        let mut made = Vec::new();
        let mut created = Vec::new();
        for dead_id in std::mem::take(&mut self.support.dying) {
            let dead = &self.actors[&dead_id];
            let technology = dead.placement.dead_summon;
            let summons = if dead.parasitic {
                Vec::new()
            } else {
                dead.buffs
                    .iter()
                    .filter_map(super::tower::RunningBuff::dead_summon)
                    .collect::<Vec<_>>()
            };
            if technology.is_none() && summons.is_empty() {
                continue;
            }
            let (mut before, mut after) = (Vec::new(), Vec::new());
            if let Some(summon) = technology {
                made.extend(self.summon_by_technology(dead_id, summon, &mut before)?);
            }
            for (summon, source) in summons {
                made.extend(self.summon_where_dead(dead_id, summon, source, &mut after)?);
            }
            created.push((dead_id, (before, after)));
        }
        let renamed = self.number_as_recorded(&made);
        for (_, (before, after)) in &mut created {
            rename_created(&renamed, before);
            rename_created(&renamed, after);
        }
        // `FightTeam.ActiveMech` put each in its side's trees as it was made.
        for unit_id in &made {
            self.plant(
                renamed
                    .get(unit_id)
                    .map_or(*unit_id, |&(renamed, _)| renamed),
            );
        }
        self.support.summoned_events.extend(created);
        Ok(())
    }

    /// One `IBEC_DeadSummon.OnMechDead`: `SummonSystem.CreateMech` of the
    /// parent's side, each summon at `CardLevel.Level1` and in a formation of
    /// its own, scattered by two draws of that side's stream.
    fn summon_where_dead(
        &mut self,
        dead_id: u64,
        summon: crate::modifier::DeadSummon,
        source: Option<ObjectRef>,
        events: &mut Vec<Event>,
    ) -> Result<Vec<u64>> {
        let Some(parent) = source
            .filter(|source| source.kind == ObjectKind::Unit)
            .and_then(|source| self.actors.get(&source.id))
        else {
            return Err(Error::new(format!(
                "unit {dead_id} dies under a buff that summons and that no unit added"
            )));
        };
        let type_id = match summon {
            crate::modifier::DeadSummon::SourceType => parent.rules.unit_type_id,
            crate::modifier::DeadSummon::Unit(id) => u32::try_from(id)
                .map_err(|_| Error::new("a buff summons a unit of a negative type"))?,
        };
        let team = parent.placement.team;
        let Some(made) = self.support.death_summons.get(&(team, type_id, 1)).cloned() else {
            return Err(Error::new(format!(
                "unit {dead_id} dies under a buff that has team {team} summon unit {type_id}, \
                 which that side's layout did not prepare"
            )));
        };
        let dead = &self.actors[&dead_id];
        if made.rules.domain != dead.rules.domain {
            return Ok(Vec::new());
        }
        let radius_q32 = space_to_q32(dead.rules.collision_radius());
        let ratio = q32_div(radius_q32, space_to_q32(made.rules.collision_radius()));
        let count = (fpcs_pow_fastest(ratio, DEAD_FACTOR) >> 32).max(1);
        self.summon_around(dead_id, (team, &made), count, None, events)
    }

    /// One `DeadSummonController.PerformDeadEffect`: `SummonSystem.CreateMech`
    /// of the dead unit's side, as many of the technology's unit type as it
    /// says, at the first level or the dead unit's, facing as the dead unit
    /// faced, scattered within its radius.
    fn summon_by_technology(
        &mut self,
        dead_id: u64,
        summon: crate::layout::DeadSummonOnDeath,
        events: &mut Vec<Event>,
    ) -> Result<Vec<u64>> {
        let dead = &self.actors[&dead_id];
        let team = dead.placement.team;
        let facing = dead.body_rotation_q32;
        let (type_id, count) = (summon.unit_type_id, summon.count);
        let key = (team, type_id, summon.level);
        let Some(made) = self.support.death_summons.get(&key).cloned() else {
            return Err(Error::new(format!(
                "unit {dead_id} dies with a technology that has team {team} summon unit \
                 {type_id}, which that side's layout did not prepare"
            )));
        };
        self.summon_around(
            dead_id,
            (team, &made),
            i64::from(count),
            Some(facing),
            events,
        )
    }

    /// `SummonSystem.CreateMech` of `count` summons about where a unit died,
    /// each moved within the whole metres of its radius by two draws of its
    /// side's stream, x then z, and joining its side at once. One a buff
    /// summons is parasitic (`MechCreateType.ParasiticalSummon`) and faces its
    /// side's way; one a technology summons faces as the dead unit faced.
    fn summon_around(
        &mut self,
        dead_id: u64,
        (team, made): (u32, &crate::layout::DeathSummon),
        count: i64,
        facing: Option<i64>,
        events: &mut Vec<Event>,
    ) -> Result<Vec<u64>> {
        let dead = &self.actors[&dead_id];
        let radius_q32 = space_to_q32(dead.rules.collision_radius());
        let (centre_x_q32, centre_z_q32) = (dead.x_q32, dead.z_q32);
        let span = i32::try_from((radius_q32 >> 32) * 100)
            .map_err(|_| Error::new("a dead unit's radius exceeds i32"))?;
        let mut joined = Vec::new();
        for _ in 0..count {
            let mut position = (centre_x_q32, centre_z_q32);
            for axis in [&mut position.0, &mut position.1] {
                let draw = self.side_random(team)?.next_in_range(span);
                *axis = axis.saturating_add(hundredths(draw));
            }
            let mut actor = self.make_parasite(made, position, events);
            if let Some(facing) = facing {
                actor.parasitic = false;
                actor.face(facing);
            }
            let unit_id = actor.placement.unit_id;
            joined.push(unit_id);
            self.join(actor)?;
            // It joins after this tick's updates: its clock reads its
            // interval until its first.
            self.actors
                .get_mut(&unit_id)
                .expect("the summon has just joined")
                .skills
                .hold_attack_clocks();
        }
        Ok(joined)
    }

    /// `FightController.CreateMech` of a parasitic summon: its side's
    /// placement for its type, a unit and a formation of its own.
    fn make_parasite(
        &mut self,
        made: &crate::layout::DeathSummon,
        (x_q32, z_q32): (i64, i64),
        events: &mut Vec<Event>,
    ) -> Actor {
        let unit_id = self.ids.next_unit;
        self.ids.next_unit += 1;
        let formation_id = self.ids.next_formation;
        self.ids.next_formation += 1;
        let placement = Placement {
            unit_id,
            formation_id,
            world_x: q32_to_space_rounded(x_q32),
            world_z: q32_to_space_rounded(z_q32),
            ..made.placement.clone()
        };
        let mut actor = Actor::at_generated_position(placement, made.rules.clone(), x_q32, z_q32);
        actor.summoned = true;
        actor.parasitic = true;
        self.rvo.added_units.push(unit_id);
        // Its agent is made with it, so the first tree built after reads its
        // position as zero, as any new agent's, and it reaches its first
        // solve with no speed until `Move`, on the update before a solve,
        // hands it one: a Rhino's Crawlers, made on a tick that solves, and a
        // Marksman's, made on the tick before one, first move two solves on.
        actor.motion.rvo_new_agent = true;
        actor.motion.next_max_speed_q32 = 0;
        actor.target_query_alive = false;
        events.push(event(
            Some(ObjectRef::new(ObjectKind::Unit, unit_id)),
            None,
            None,
            None,
            EventPayload::UnitCreated {
                team_id: actor.placement.team,
                formation_id,
                unit_type_id: actor.rules.unit_type_id,
                position: QVec3 {
                    x: x_q32,
                    y: space_to_q32(unit_height(actor.rules.domain)),
                    z: z_q32,
                },
            },
        ));
        actor
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
            provider: Provider::SupportUnit,
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

/// `FQuaternion.AngleAxis(angle, axis) * (x, 0, z)` about `FVector3.up`
/// (`axis_y` one) or `down` (minus one), in the build's fixed point: the
/// half angle's `SinFastest` times the axis is the quaternion's `y`, a
/// quarter turn on its `w`, and `FQuaternion.Transform` expands the product
/// term by term.
pub(in crate::fight) fn turn_about_vertical(
    angle_q32: i64,
    axis_y: i64,
    x: i64,
    z: i64,
) -> (i64, i64) {
    let half = q32_div(q32_mul(angle_q32, DEG_TO_RAD), 2 << 32);
    let qy = q32_mul(axis_y, fpcs_sin_fastest(half));
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
