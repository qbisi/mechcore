//! `InterceptSystem`: an interceptor takes enemy projectiles out of the air.
//!
//! An interceptor is a building of its side, and `InterceptEffectBase` is what
//! it does. Each projectile that can be intercepted keeps the interceptors it
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
//! is removed as intercepted on the spot. `docs/rules/contraptions.md` states
//! the rule.

use super::*;
use crate::layout::{Interception, InterceptorBuilding};

/// `InterceptEffectBase.InterceptState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) enum InterceptState {
    Idle,
    Preparing,
    Resetting,
}

/// One interceptor's `InterceptEffectBase`.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Interceptor {
    pub(in crate::fight) building_id: u64,
    pub(in crate::fight) team: u32,
    x_q32: i64,
    z_q32: i64,
    interception: Interception,
    enabled: bool,
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
    pub(in crate::fight) fn new(building_id: u64, placed: &InterceptorBuilding) -> Self {
        Self {
            building_id,
            team: placed.team,
            x_q32: space_to_q32(placed.x),
            z_q32: space_to_q32(placed.z),
            interception: placed.interception,
            // `DoAdd` enables it idle, and `OnBaseEnterFight` fills its attack.
            enabled: true,
            in_reach: Vec::new(),
            target: None,
            state: InterceptState::Idle,
            hits: false,
            sum_time: 0,
            attack: placed.interception.attack,
            sum_cooling: 0,
            cooling: false,
        }
    }

    /// `IsInSourceRange`: at least the reach's minimum away, and under its
    /// maximum, measured in three dimensions from where it stands.
    fn reaches(&self, x_q32: i64, y_q32: i64, z_q32: i64) -> bool {
        let distance = self.distance_q32(x_q32, y_q32, z_q32);
        distance < self.interception.range_max_q32 && distance >= self.interception.range_min_q32
    }

    fn distance_q32(&self, x_q32: i64, y_q32: i64, z_q32: i64) -> i64 {
        native_q32_magnitude_3d(
            x_q32.saturating_sub(self.x_q32),
            y_q32,
            z_q32.saturating_sub(self.z_q32),
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
    /// `InterceptSystem`'s update: every interceptor, side by side in the
    /// order they were released.
    pub(in crate::fight) fn step_interceptors(&mut self, events: &mut Vec<Event>) -> Result<()> {
        for index in 0..self.interceptors.len() {
            self.update_interceptor(index, events)?;
        }
        Ok(())
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
                self.set_intercept_target(index, None);
                self.interceptors[index].state = InterceptState::Idle;
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
            let distance =
                interceptor.distance_q32(projectile.x_q32, projectile.y_q32, projectile.z_q32);
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
        let (building_id, previous) = (interceptor.building_id, interceptor.target);
        if previous == target {
            return;
        }
        if let Some(projectile) = previous.and_then(|id| self.projectile_mut(id)) {
            projectile.locked_by.retain(|&source| source != building_id);
        }
        if let Some(projectile) = target.and_then(|id| self.projectile_mut(id)) {
            projectile.locked_by.push(building_id);
        }
        self.interceptors[index].target = target;
    }

    /// `EnterPrepare`: whether the attack will hit is drawn now, from the
    /// side's stream, `GRRandom.IsProbabilityPass` drawing `Next(1000)` even
    /// when the probability is a certainty.
    fn enter_prepare(&mut self, index: usize) -> Result<()> {
        let (team, probability) = {
            let interceptor = &self.interceptors[index];
            (interceptor.team, interceptor.interception.probability)
        };
        let draw = self.side_random(team)?.next_between_inclusive(0, 999);
        let interceptor = &mut self.interceptors[index];
        interceptor.hits = draw < probability;
        interceptor.cooling = false;
        interceptor.state = InterceptState::Preparing;
        Ok(())
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
    /// the reach of, and joins every opposing one it is in the reach of.
    pub(in crate::fight) fn track_interceptors(&mut self, projectile: &mut Projectile) {
        if !projectile.interceptible {
            return;
        }
        let (x, y, z) = (projectile.x_q32, projectile.y_q32, projectile.z_q32);
        for source in std::mem::take(&mut projectile.sources) {
            let keeps = self.interceptor(source).is_some_and(|interceptor| {
                interceptor.team != projectile.team && interceptor.reaches(x, y, z)
            });
            if !keeps {
                self.leave_interceptor(source, projectile);
            }
        }
        for interceptor in self
            .interceptors
            .iter_mut()
            .filter(|interceptor| interceptor.enabled && interceptor.team != projectile.team)
        {
            if interceptor.reaches(x, y, z) {
                projectile.sources.push(interceptor.building_id);
                if !interceptor.in_reach.contains(&projectile.id) {
                    interceptor.in_reach.push(projectile.id);
                }
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
        let Some(index) = self
            .interceptors
            .iter()
            .position(|interceptor| interceptor.building_id == source)
        else {
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
    /// it intercepts nothing more and lets go of its lock.
    pub(in crate::fight) fn lose_interceptor(&mut self, building_id: u64) {
        let Some(index) = self
            .interceptors
            .iter()
            .position(|interceptor| interceptor.building_id == building_id)
        else {
            return;
        };
        self.set_intercept_target(index, None);
        let interceptor = &mut self.interceptors[index];
        interceptor.enabled = false;
        interceptor.in_reach.clear();
        interceptor.state = InterceptState::Idle;
    }

    /// Whether this building is an interceptor that still stands.
    pub(in crate::fight) fn standing_interceptor(&self, team: u32) -> Option<u64> {
        self.interceptors
            .iter()
            .find(|interceptor| interceptor.team == team && interceptor.enabled)
            .map(|interceptor| interceptor.building_id)
    }

    /// Whether this building is an interceptor, standing or not.
    pub(in crate::fight) fn is_interceptor(&self, building_id: u64) -> bool {
        self.interceptor(building_id).is_some()
    }

    fn interceptor(&self, building_id: u64) -> Option<&Interceptor> {
        self.interceptors
            .iter()
            .find(|interceptor| interceptor.building_id == building_id)
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
