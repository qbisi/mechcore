//! `InterceptSystem`: an interceptor takes enemy projectiles out of the air.
//!
//! `InterceptEffectBase` is what an interceptor does, and an
//! `InterceptCtr_Group` of them is what a side's `TeamInterceptSourceManager`
//! keeps for one owner: an interceptor building's one, or the interceptors a
//! unit's interception technology makes it, which stand where the unit
//! stands. Each projectile that can be intercepted keeps the interceptors it
//! is in reach of up to date as it moves (`ProjectileController.
//! GetInInterceptSources`), and each interceptor keeps the projectiles in its
//! reach in the order they came into it. `InterceptSystem` updates after
//! `ProjectileSystem`, so an interceptor sees the tick's moves.
//!
//! An idle interceptor locks the nearest projectile in its reach that the
//! attack already locked on it cannot finish, and prepares: it draws once
//! from its side's stream whether the attack will hit. When it has prepared,
//! a hit takes its attack off the projectile's life, and the attack falls. It
//! then resets and is idle again. An interceptor idle with nothing to lock
//! gives its attack back a little at a time. A projectile whose life is gone
//! is removed as intercepted on the spot. A preemptive unit's interceptor
//! locks its unit's main skill while it prepares and resets, and lets it go
//! idle as it does. `docs/rules/contraptions.md` states the rule for a
//! building, and `docs/rules/technology_effects.md` for a unit.

use super::*;
use crate::layout::{Interception, InterceptorBuilding};
use crate::modifier::UnitInterception;

/// `InterceptEffectBase.InterceptState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum InterceptState {
    Idle,
    Preparing,
    Resetting,
}

/// Whose an interceptor is: an `InterceptEffect_FightInterceptor` stands
/// where its building does, and an `InterceptEffect_FightMech_Preemptive` or
/// `_NoPreemptive` where its unit does, on the unit's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum InterceptorOwner {
    Building {
        building_id: u64,
        team: u32,
        x_q32: i64,
        z_q32: i64,
    },
    Unit {
        actor_id: u64,
        preemptive: bool,
    },
}

/// One interceptor's `InterceptEffectBase`.
#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each is a field of `InterceptEffectBase`"
)]
pub(in crate::fight) struct Interceptor {
    /// What names it in a projectile's lists.
    key: u64,
    pub(in crate::fight) owner: InterceptorOwner,
    interception: Interception,
    /// `isEnable`.
    enabled: bool,
    /// Whether its group has left its side's `interceptControllerRecords`
    /// (`TeamInterceptSourceManager.DoRemove`): it updates no more and no
    /// projectile joins it, and what it holds stays as it was.
    removed: bool,
    /// The projectiles in reach, in the order they came into it.
    in_reach: Vec<u64>,
    target: Option<u64>,
    state: InterceptState,
    hits: bool,
    sum_time: u32,
    attack: i64,
    sum_cooling: u32,
    cooling: bool,
}

impl Interceptor {
    /// `InterceptCtr_Group.Init` and `DoAdd`: enabled and idle, its attack
    /// whole.
    fn new(key: u64, owner: InterceptorOwner, interception: Interception) -> Self {
        Self {
            key,
            owner,
            interception,
            enabled: true,
            removed: false,
            in_reach: Vec::new(),
            target: None,
            state: InterceptState::Idle,
            hits: false,
            sum_time: 0,
            attack: interception.attack,
            sum_cooling: 0,
            cooling: false,
        }
    }

    /// An interceptor building's, as the fight is built.
    pub(in crate::fight) fn building(
        key: u64,
        building_id: u64,
        placed: &InterceptorBuilding,
    ) -> Self {
        Self::new(
            key,
            InterceptorOwner::Building {
                building_id,
                team: placed.team,
                x_q32: space_to_q32(placed.x),
                z_q32: space_to_q32(placed.z),
            },
            placed.interception,
        )
    }

    /// `DropAtk`: a hit costs the attack its decline, down to its floor.
    fn drop_attack(&mut self) {
        self.attack = self
            .attack
            .saturating_sub(self.interception.decline)
            .max(self.interception.lower);
    }

    /// `RecorveAtk`: a rise gives the attack back, up to its whole.
    fn recover_attack(&mut self) {
        self.attack = self
            .attack
            .saturating_add(self.interception.rise)
            .min(self.interception.attack);
    }

    /// `UpdateForIdleCooling`: idle with nothing to lock, it cools for its
    /// cooling time and then rises once every rise interval.
    fn idle_cooling(&mut self) {
        self.sum_cooling = self.sum_cooling.saturating_add(1);
        if self.cooling {
            if self.sum_cooling >= self.interception.rise_ticks {
                self.sum_cooling = 0;
                self.recover_attack();
            }
        } else if self.sum_cooling >= self.interception.cooling_ticks {
            self.sum_cooling = 0;
            self.cooling = true;
        }
    }
}

impl Simulation {
    /// `InterceptSystem`'s update: every side's groups in the order its
    /// `interceptControllerRecords` keeps them, each group's interceptors in
    /// turn. Each side draws from its own stream and locks only the other
    /// side's projectiles, so the sides' order changes nothing.
    pub(in crate::fight) fn step_interceptors(&mut self, events: &mut Vec<Event>) -> Result<()> {
        for index in 0..self.interceptors.len() {
            if !self.interceptors[index].removed {
                self.update_interceptor(index, events)?;
            }
        }
        Ok(())
    }

    /// `InterceptMissileEffectProvider.DoActive`: a unit whose technologies
    /// make it an interceptor adds its group to its side's
    /// (`TeamInterceptSourceManager.GetInterceptSource`), one interceptor
    /// for each of its weapons, each enabled and idle with its attack whole
    /// (`InterceptCtr_Group.Init`, `DoAdd`).
    pub(in crate::fight) fn activate_interception(&mut self, actor_id: u64) {
        let Some(UnitInterception {
            interception,
            weapons,
            preemptive,
        }) = self.actors[&actor_id].placement.effects.interception
        else {
            return;
        };
        for _ in 0..weapons {
            let key = self.next_interceptor_key();
            self.interceptors.push(Interceptor::new(
                key,
                InterceptorOwner::Unit {
                    actor_id,
                    preemptive,
                },
                interception,
            ));
        }
    }

    /// The units that start the fight on the ground activate their
    /// interception as `FightEffectSystem.OnEnterFight` activates their
    /// effects. A unit that travels in activates it as it arrives.
    pub(in crate::fight) fn activate_interceptions(&mut self) {
        let units = self
            .actors
            .iter()
            .filter(|(_, actor)| {
                !actor.travelling && actor.placement.effects.interception.is_some()
            })
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        for actor_id in units {
            self.activate_interception(actor_id);
        }
    }

    /// `InterceptMissileEffectProvider.DisableEffect` and `EnableEffect`:
    /// each interceptor of the unit's group (`TeamInterceptSourceManager.
    /// GetInterceptSource`) disabled or enabled (`InterceptEffectBase.
    /// DoDisable`, `DoEnable`), which either way lets its target go and
    /// returns it to idle (`SetTarget`, `OnEnterIdle`): a disabled one does
    /// not update (`InterceptEffectBase.Update`).
    pub(in crate::fight) fn switch_unit_interception(&mut self, actor_id: u64, on: bool) {
        for index in 0..self.interceptors.len() {
            let interceptor = &mut self.interceptors[index];
            if interceptor.removed
                || !matches!(interceptor.owner, InterceptorOwner::Unit { actor_id: id, .. } if id == actor_id)
            {
                continue;
            }
            interceptor.enabled = on;
            self.enter_intercept_idle(index);
        }
    }

    /// `InterceptMissileEffectProvider.DoDeactive`, as `DeadEffectSystem`
    /// calls a dead unit's `OnDead` and `FightEffectSystem.DeactiveEffect`
    /// takes its effects off: its group leaves its side's.
    pub(in crate::fight) fn deactivate_dead_interceptions(&mut self) {
        let dead = self
            .interceptors
            .iter()
            .filter_map(|interceptor| match interceptor.owner {
                InterceptorOwner::Unit { actor_id, .. }
                    if !interceptor.removed && !self.actors[&actor_id].alive() =>
                {
                    Some(actor_id)
                }
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        for actor_id in dead {
            self.remove_interceptors(|owner| {
                matches!(owner, InterceptorOwner::Unit { actor_id: id, .. } if id == actor_id)
            });
        }
    }

    /// `InterceptSystem.OnChangeTeam`: a turned unit's group leaves its old
    /// side's records for the end of its new side's.
    pub(in crate::fight) fn interceptors_change_side(&mut self, actor_id: u64) {
        let (turned, kept): (Vec<_>, Vec<_>) = std::mem::take(&mut self.interceptors)
            .into_iter()
            .partition(|interceptor| {
                !interceptor.removed
                    && matches!(interceptor.owner, InterceptorOwner::Unit { actor_id: id, .. } if id == actor_id)
            });
        self.interceptors = kept;
        self.interceptors.extend(turned);
    }

    /// `TeamInterceptSourceManager.DoRemove`: the owner's group leaves its
    /// side's records, and `InterceptCtr_Group.DoRemove` tells its
    /// interceptors no more than that. What each holds stays: its lock
    /// still counts against what the others lock.
    fn remove_interceptors(&mut self, owned: impl Fn(InterceptorOwner) -> bool) {
        for interceptor in &mut self.interceptors {
            if owned(interceptor.owner) {
                interceptor.removed = true;
            }
        }
    }

    fn next_interceptor_key(&self) -> u64 {
        self.interceptors
            .iter()
            .map(|interceptor| interceptor.key)
            .max()
            .map_or(1, |key| key + 1)
    }

    /// `GetTeamController`: a building's side, or its unit's as it stands.
    fn interceptor_team(&self, index: usize) -> u32 {
        match self.interceptors[index].owner {
            InterceptorOwner::Building { team, .. } => team,
            InterceptorOwner::Unit { actor_id, .. } => self.actors[&actor_id].placement.team,
        }
    }

    /// `GetPos`: a building's centre on the ground, or where its unit's
    /// transform stands.
    fn interceptor_position(&self, index: usize) -> (i64, i64, i64) {
        match self.interceptors[index].owner {
            InterceptorOwner::Building { x_q32, z_q32, .. } => (x_q32, 0, z_q32),
            InterceptorOwner::Unit { actor_id, .. } => {
                let actor = &self.actors[&actor_id];
                (
                    actor.x_q32,
                    space_to_q32(unit_height(actor.domain)),
                    actor.z_q32,
                )
            }
        }
    }

    /// `IsInSourceRange`: at least the reach's minimum away, and under its
    /// maximum, measured in three dimensions from where it stands.
    fn interceptor_reaches(&self, index: usize, x_q32: i64, y_q32: i64, z_q32: i64) -> bool {
        let distance = self.interceptor_distance(index, x_q32, y_q32, z_q32);
        let interception = &self.interceptors[index].interception;
        distance < interception.range_max_q32 && distance >= interception.range_min_q32
    }

    fn interceptor_distance(&self, index: usize, x_q32: i64, y_q32: i64, z_q32: i64) -> i64 {
        let (x, y, z) = self.interceptor_position(index);
        native_q32_magnitude_3d(
            x_q32.saturating_sub(x),
            y_q32.saturating_sub(y),
            z_q32.saturating_sub(z),
        )
    }

    /// `InterceptEffectBase.Update`.
    fn update_interceptor(&mut self, index: usize, events: &mut Vec<Event>) -> Result<()> {
        let interceptor = &mut self.interceptors[index];
        if !interceptor.enabled {
            return Ok(());
        }
        if interceptor.target.is_some() {
            if interceptor.state == InterceptState::Preparing {
                interceptor.sum_time = interceptor.sum_time.saturating_add(1);
                if interceptor.sum_time >= interceptor.interception.prepare_ticks {
                    interceptor.sum_time = 0;
                    self.try_intercept(index, events)?;
                    self.interceptors[index].state = InterceptState::Resetting;
                }
            }
        } else if interceptor.state == InterceptState::Idle {
            interceptor.sum_time = 0;
            let target = self.intercept_target(index);
            self.set_intercept_target(index, target);
            if target.is_none() {
                self.interceptors[index].idle_cooling();
                return Ok(());
            }
            self.enter_prepare(index)?;
        }
        let interceptor = &mut self.interceptors[index];
        if interceptor.state == InterceptState::Resetting {
            interceptor.sum_time = interceptor.sum_time.saturating_add(1);
            let reset = interceptor
                .interception
                .interval_ticks
                .saturating_sub(interceptor.interception.prepare_ticks);
            if interceptor.sum_time >= reset {
                interceptor.sum_time = 0;
                self.enter_intercept_idle(index);
            }
        }
        Ok(())
    }

    /// `GetTarget`: the nearest projectile in reach whose life the attack
    /// already locked on it does not cover; the first of equals.
    fn intercept_target(&self, index: usize) -> Option<u64> {
        let interceptor = &self.interceptors[index];
        let mut chosen = None;
        let mut nearest = i64::MAX;
        for &projectile_id in &interceptor.in_reach {
            let Some(projectile) = self.projectile(projectile_id) else {
                continue;
            };
            let locked = projectile
                .locked_by
                .iter()
                .filter_map(|&source| self.interceptor(source))
                .map(|source| source.attack)
                .sum::<i64>();
            if projectile.life < locked {
                continue;
            }
            let distance = self.interceptor_distance(
                index,
                projectile.x_q32,
                projectile.y_q32,
                projectile.z_q32,
            );
            if nearest > distance {
                nearest = distance;
                chosen = Some(projectile_id);
            }
        }
        chosen
    }

    /// `SetTarget`: the lock moves from the old projectile to the new.
    fn set_intercept_target(&mut self, index: usize, target: Option<u64>) {
        let interceptor = &self.interceptors[index];
        let (key, previous) = (interceptor.key, interceptor.target);
        if previous == target {
            return;
        }
        if let Some(projectile) = previous.and_then(|id| self.projectile_mut(id)) {
            projectile.locked_by.retain(|&source| source != key);
        }
        if let Some(projectile) = target.and_then(|id| self.projectile_mut(id)) {
            projectile.locked_by.push(key);
        }
        self.interceptors[index].target = target;
    }

    /// `EnterPrepare`: whether the attack will hit is drawn now, from the
    /// side's stream, `GRRandom.IsProbabilityPass` drawing `Next(1000)` even
    /// when the probability is a certainty. A preemptive unit's interceptor
    /// hears it (`OnEnterPrepare`) and locks its unit's main skill
    /// (`InterceptEffect_FightMech_Preemptive.ChangeMechToLockState`).
    fn enter_prepare(&mut self, index: usize) -> Result<()> {
        let team = self.interceptor_team(index);
        let probability = self.interceptors[index].interception.probability;
        let draw = self.side_random(team)?.next_between_inclusive(0, 999);
        let interceptor = &mut self.interceptors[index];
        interceptor.hits = draw < probability;
        interceptor.cooling = false;
        interceptor.state = InterceptState::Preparing;
        if let InterceptorOwner::Unit {
            actor_id,
            preemptive: true,
        } = interceptor.owner
        {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            super::skill::lock(&mut actor.skills.main);
        }
        Ok(())
    }

    /// `EnterIdle`: the lock goes, and an interceptor that was not idle
    /// becomes so. A preemptive unit's interceptor hears it (`OnEnterIdle`)
    /// and lets its unit's main skill idle
    /// (`InterceptEffect_FightMech_Preemptive.ChangeMechToUnlockState`).
    fn enter_intercept_idle(&mut self, index: usize) {
        self.set_intercept_target(index, None);
        let interceptor = &mut self.interceptors[index];
        if interceptor.state == InterceptState::Idle {
            return;
        }
        interceptor.state = InterceptState::Idle;
        if let InterceptorOwner::Unit {
            actor_id,
            preemptive: true,
        } = interceptor.owner
        {
            let actor = self
                .actors
                .get_mut(&actor_id)
                .expect("actor identity is stable");
            super::skill::unlock(&mut actor.skills.main);
        }
    }

    /// `TryAttack`: a hit takes the attack off the projectile's life, which
    /// removes it once the life is gone, and then the attack falls.
    fn try_intercept(&mut self, index: usize, events: &mut Vec<Event>) -> Result<()> {
        let interceptor = &self.interceptors[index];
        if !interceptor.hits {
            return Ok(());
        }
        let Some(target) = interceptor.target else {
            return Ok(());
        };
        let attack = interceptor.attack;
        let position = self
            .projectiles
            .iter()
            .position(|projectile| projectile.id == target)
            .ok_or_else(|| Error::new("an interceptor's lock is not in the air"))?;
        let projectile = &mut self.projectiles[position];
        projectile.life = projectile.life.saturating_sub(attack).max(0);
        if projectile.life <= 0 {
            let projectile = self.projectiles.remove(position);
            events.push(event(
                Some(projectile.object_ref()),
                projectile.shooter.actor().map(FightActorRef::object_ref),
                projectile.shooter.actor().map(|_| projectile.team),
                Some(ObjectRef::new(projectile.target_kind, projectile.target)),
                EventPayload::ProjectileRemoved {
                    position: QVec3 {
                        x: projectile.x_q32,
                        y: projectile.y_q32,
                        z: projectile.z_q32,
                    },
                    intercepted: true,
                    absorbed_by: None,
                },
            ));
            self.leave_interceptors(&projectile);
        }
        self.interceptors[index].drop_attack();
        Ok(())
    }

    /// `ProjectileController.GetInInterceptSources`, after a projectile that
    /// can be intercepted has moved: it leaves the interceptors it has left
    /// the reach of, or that stand on its side now, and joins every one in
    /// the records of the other side that it is in the reach of
    /// (`TeamInterceptSourceManager.GetCurrentTeamInterceptSources`, which
    /// takes a unit's group while the unit stands on that side).
    pub(in crate::fight) fn track_interceptors(&mut self, projectile: &mut Projectile) {
        if !projectile.interceptible {
            return;
        }
        let (x, y, z) = (projectile.x_q32, projectile.y_q32, projectile.z_q32);
        for source in std::mem::take(&mut projectile.sources) {
            let keeps = self.interceptor_index(source).is_some_and(|index| {
                self.interceptor_team(index) != projectile.team
                    && self.interceptor_reaches(index, x, y, z)
            });
            if !keeps {
                self.leave_interceptor(source, projectile);
            }
        }
        for index in 0..self.interceptors.len() {
            if self.interceptors[index].removed
                || self.interceptor_team(index) == projectile.team
                || !self.interceptor_reaches(index, x, y, z)
            {
                continue;
            }
            let interceptor = &mut self.interceptors[index];
            projectile.sources.push(interceptor.key);
            if !interceptor.in_reach.contains(&projectile.id) {
                interceptor.in_reach.push(projectile.id);
            }
        }
    }

    /// `ClearInterceptSources`, as a projectile is released for any reason.
    pub(in crate::fight) fn leave_interceptors(&mut self, projectile: &Projectile) {
        let mut released = projectile.clone();
        for source in std::mem::take(&mut released.sources) {
            self.leave_interceptor(source, &mut released);
        }
    }

    /// `RemoveProjectileController`: an interceptor whose lock leaves it
    /// drops the lock and resets.
    fn leave_interceptor(&mut self, source: u64, projectile: &mut Projectile) {
        let Some(index) = self.interceptor_index(source) else {
            return;
        };
        let interceptor = &mut self.interceptors[index];
        if interceptor.target == Some(projectile.id) {
            interceptor.sum_time = 0;
            interceptor.target = None;
            projectile.locked_by.retain(|&locker| locker != source);
            interceptor.state = InterceptState::Resetting;
        }
        interceptor.in_reach.retain(|&id| id != projectile.id);
    }

    /// `InterceptSystem.DoRemoveFightInterceptor`, when its building falls:
    /// its group leaves its side's.
    pub(in crate::fight) fn lose_interceptor(&mut self, building_id: u64) {
        self.remove_interceptors(|owner| {
            matches!(owner, InterceptorOwner::Building { building_id: id, .. } if id == building_id)
        });
    }

    /// The interceptor a projectile's list names by its key.
    fn interceptor_index(&self, key: u64) -> Option<usize> {
        self.interceptors
            .iter()
            .position(|interceptor| interceptor.key == key)
    }

    fn interceptor(&self, key: u64) -> Option<&Interceptor> {
        self.interceptor_index(key)
            .map(|index| &self.interceptors[index])
    }

    fn projectile(&self, id: u64) -> Option<&Projectile> {
        self.projectiles
            .iter()
            .find(|projectile| projectile.id == id)
    }

    fn projectile_mut(&mut self, id: u64) -> Option<&mut Projectile> {
        self.projectiles
            .iter_mut()
            .find(|projectile| projectile.id == id)
    }
}
