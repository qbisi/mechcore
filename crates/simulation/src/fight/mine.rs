//! `MineSystem`: a missile waits where it was released and fires once.
//!
//! A missile is `FightLandMine`, not a building: nothing targets it and it
//! takes no part in movement. `MineSystem` updates before `FightCoreSystem`,
//! so on every tick, before any unit has moved, `TeamMineManager.Update` asks
//! each missile of each side whether an enemy has come into its trigger
//! range: the nearest object of the other side by its edge, a unit of either
//! domain or a building that is not a tower, under the range by the missile's
//! own two-dimensional distance. One that has fires a projectile at it from
//! `mineFlyHeight` above where it stands and is spent. The projectile follows
//! its target, carries the missile's own damage and splash, and its hit writes
//! the missile's buff on every unit it strikes. `docs/rules/contraptions.md`
//! states the rule.

use super::*;
use crate::data::{Entry, Index};

/// `Config.mineFlyHeight`, which the export does not carry: the missiles of
/// `tests/missile/fights/` leave from 60 metres above where they stand.
const MINE_FLY_HEIGHT: i64 = 60_000;

/// What tags a missile's buff, so that its end takes it away.
const MISSILE_SOURCE: &str = "BuffSystem.LandMine";

/// One missile standing, `FightLandMine`.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Mine {
    team: u32,
    x: i64,
    z: i64,
    x_q32: i64,
    z_q32: i64,
    trigger_range_q32: i64,
    shot: MissileShot,
}

impl Mine {
    pub(in crate::fight) const fn team(&self) -> u32 {
        self.team
    }

    pub(in crate::fight) fn new(placed: &MissileMine) -> Self {
        Self {
            team: placed.team,
            x: placed.x,
            z: placed.z,
            x_q32: space_to_q32(placed.x),
            z_q32: space_to_q32(placed.z),
            trigger_range_q32: placed.trigger_range_q32,
            shot: placed.shot.clone(),
        }
    }
}

impl Simulation {
    /// `MineSystem`'s update: each side's `TeamMineManager.Update`, which
    /// walks the side's missiles from the last released to the first, each
    /// fired at most once.
    pub(in crate::fight) fn step_mines(
        &mut self,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let teams = self
            .mines
            .iter()
            .map(Mine::team)
            .collect::<std::collections::BTreeSet<_>>();
        for team in teams {
            let mut index = self.mines.len();
            while index > 0 {
                index -= 1;
                if self.mines[index].team != team {
                    continue;
                }
                if let Some(target) = self.mine_trigger(&self.mines[index], target_search_order) {
                    let mine = self.mines.remove(index);
                    self.fire_mine(&mine, target, events)?;
                }
            }
        }
        Ok(())
    }

    /// `TeamMineManager.TryActiveMine`: the nearest object of the other side
    /// whose edge is under the trigger range, the first of equals; a tower
    /// never sets a missile off.
    fn mine_trigger(
        &self,
        mine: &Mine,
        target_search_order: &BTreeMap<u32, Vec<FightActorRef>>,
    ) -> Option<FightActorRef> {
        let mut chosen = None;
        let mut nearest = i64::MAX;
        for (_, candidates) in target_search_order
            .iter()
            .filter(|(team, _)| **team != mine.team)
        {
            for &candidate in candidates {
                if self.is_tower(candidate) || !self.fight_actor_is_alive(candidate) {
                    continue;
                }
                let Some(view) = self.fight_actor(candidate) else {
                    continue;
                };
                let distance = native_q32_magnitude(
                    view.x_q32.saturating_sub(mine.x_q32),
                    view.z_q32.saturating_sub(mine.z_q32),
                )
                .saturating_sub(space_to_q32(view.radius));
                if distance < mine.trigger_range_q32 && distance < nearest {
                    nearest = distance;
                    chosen = Some(candidate);
                }
            }
        }
        chosen
    }

    /// `MineSystem.ActiveMine`: a projectile the missile's own, from
    /// `mineFlyHeight` above it, locked on what set it off.
    fn fire_mine(
        &mut self,
        mine: &Mine,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let view = self
            .fight_actor(target)
            .ok_or_else(|| Error::new("a missile's target is not on the board"))?;
        let (target_kind, target_id, target_y) = match target {
            FightActorRef::Unit(id) => (
                ObjectKind::Unit,
                id,
                unit_height(self.actors[&id].rules.domain),
            ),
            FightActorRef::Building(id) => (ObjectKind::Building, id, 0),
        };
        let projectile_id = self.ids.objects.allocate_object(ObjectKind::Projectile)?.id;
        let projectile = Projectile {
            id: projectile_id,
            team: mine.team,
            shooter: Shooter::Missile(mine.shot.clone()),
            skill_slot: 0,
            target_kind,
            target: target_id,
            x: mine.x,
            y: MINE_FLY_HEIGHT,
            z: mine.z,
            x_q32: mine.x_q32,
            y_q32: space_to_q32(MINE_FLY_HEIGHT),
            z_q32: mine.z_q32,
            cached_target_x: q32_to_space_rounded(view.x_q32),
            cached_target_y: target_y,
            cached_target_z: q32_to_space_rounded(view.z_q32),
            cached_target_x_q32: view.x_q32,
            cached_target_y_q32: space_to_q32(target_y),
            cached_target_z_q32: view.z_q32,
            cached_target_radius: view.radius,
            speed: mine.shot.speed,
            life: mine.shot.life,
            max_life: mine.shot.life,
            interceptible: mine.shot.interceptible,
            sources: Vec::new(),
            locked_by: Vec::new(),
            lock_target: true,
            offset_x_q32: 0,
            offset_z_q32: 0,
            climb_to_q32: None,
            // `ProjectileController.inEnergyShields`, as for any projectile.
            spawn_shields: self.enemy_shields_at(
                mine.team,
                mine.x_q32,
                space_to_q32(MINE_FLY_HEIGHT),
                mine.z_q32,
            ),
            absorbed_by: None,
            move_range_q32: mine
                .trigger_range_q32
                .saturating_add(space_to_q32(view.radius)),
        };
        events.push(event(
            Some(projectile.object_ref()),
            None,
            None,
            Some(ObjectRef::new(target_kind, target_id)),
            EventPayload::ProjectileReleased {
                skill_slot: None,
                weapon_index: None,
            },
        ));
        self.projectiles.push(projectile);
        Ok(())
    }

    /// A missile's projectile landing: the missile's own damage and splash,
    /// credited to the projectile, and then its buff on every unit it struck
    /// that still stands, as `FightLandMine.DispatchHitDamageEvent` adds it.
    pub(in crate::fight) fn missile_hit(
        &mut self,
        projectile: &Projectile,
        shot: &MissileShot,
        aimed: FightActorRef,
        reach: Reach,
        events: &mut Vec<Event>,
    ) -> Result<super::damage::Struck> {
        // A missile's hits cross no shield (`FightLandMine` answers
        // `CanCrossAdvancedEnergyShield` no): the shield it flew into takes
        // it, and one without a splash is taken by the shield covering its
        // target, as a unit's shot is. No owner stands inside a shield.
        let mut shield = projectile.absorbed_by;
        if shield.is_none() && shot.splash_radius == 0 {
            shield = self.shield_around(aimed);
        }
        let hit = DamageHit {
            shield,
            splash_radius: shot.splash_radius,
            ..DamageHit::of_projectile(projectile, aimed, shot.damage, reach)
        };
        let struck = self.perform_damage(hit, events)?;
        let row = super::tower::BuffRow {
            clears_when_technologies_disabled: false,
            buff_id: shot.buff.id,
            max_life_rate: 0,
            stacking: None,
            summons: None,
            divide: shot.buff.divide,
            additive: shot.buff.additive,
            ticks: shot.buff.ticks,
            source: MISSILE_SOURCE,
            entries: vec![Entry {
                index: Index::MoveSpeed,
                source: MISSILE_SOURCE,
                correction: super::tower::rate(shot.buff.move_speed_rate),
            }],
            disables_technology: false,
            debuff: shot.buff.debuff,
            invincible: false,
            life_change: None,
            current_life_rate: 0,
        };
        for &target in &struck.targets {
            if let FightActorRef::Unit(id) = target
                && self.actors[&id].alive()
                && self.buff_reaches(id, &row)
            {
                self.write_buff(id, None, projectile.team, &row, events)?;
            }
        }
        Ok(struck)
    }
}
