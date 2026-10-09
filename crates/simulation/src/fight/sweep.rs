//! `FightSweepSkill` and its `SweepAttackPerformer`: a strip swept across
//! the target, one stretch of it struck every few updates, each unit struck
//! as often as its radius allows.

use std::collections::VecDeque;

use super::rvo::FixedVec2;
use super::skill::Performer;
use super::*;
use crate::modifier::SweepIntensify;

/// One stretch of the strip, `LineRange`: from `start` to `end`, the sweep's
/// width wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct SweepStretch {
    start: FixedVec2,
    end: FixedVec2,
}

/// A sweep under way, `SweepAttackPerformer`'s state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::fight) struct Sweep {
    /// `m_currentAttackArea`, the stretches still to strike; laid out on the
    /// performer's first update (`FirstPrepare`).
    stretches: VecDeque<SweepStretch>,
    prepared: bool,
    /// `m_prevArea`, the stretch struck last.
    previous: Option<SweepStretch>,
    /// `m_damagedTargets`: how often each unit has been struck, in the order
    /// first struck.
    struck: Vec<(FightActorRef, u32)>,
    delay_q32: i64,
    delta_frame: u32,
    damage_frame: u32,
    /// What the skill aimed at as the sweep started (`m_currentAttackTarget`,
    /// else `m_currentLockTarget`): its domain is the only one struck.
    aimed: Option<FightActorRef>,
    targets_air: bool,
    attack_count: i32,
    /// The update it last ran on: the attack controller updates the
    /// attacking phase it enters on the update it enters it.
    updated_step: Option<u64>,
}

impl Actor {
    /// What its technology changes of its sweep skill, while the skill holds
    /// it.
    pub(in crate::fight) fn sweep_intensify(&self) -> Option<SweepIntensify> {
        self.placement
            .effects
            .sweep
            .filter(|_| self.sweep_intensified)
    }
}

/// The sweep path's numbers, read off the unit's attack.
struct SweepShape {
    perpendicular: bool,
    reverse: bool,
    fixed_direction: bool,
    length: i64,
    width_q32: i64,
    damage_times: u32,
    damage_interval_q32: i64,
    damage_delay_q32: i64,
    hit_caps: Vec<(i64, u32)>,
}

/// A decimal as `FPoint` truncates it: the extractor writes the shortest
/// decimal whose truncation is the build's raw value.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the extractor writes values whose Q32.32 truncation is the build's raw value"
)]
fn truncated_q32(value: f64) -> i64 {
    (value * 4_294_967_296.0) as i64
}

/// The unit's sweep as its technologies leave it: `FightSweepSkill`'s
/// `ChangeLength` and `ChangeWidth` add their metres, none below nothing, and
/// the rest set how the strip lies and runs.
fn shape_of(attack: &AttackConfig, intensify: Option<SweepIntensify>) -> Option<SweepShape> {
    let AttackPath::Sweep {
        perpendicular,
        length,
        width,
        damage_times,
        damage_interval,
        damage_delay,
        hit_caps,
        ..
    } = &attack.path
    else {
        return None;
    };
    let changed = |base: u32, change: Option<i32>| {
        let total = i64::from(base) + i64::from(change.unwrap_or(0));
        if total < 1 { 0 } else { total }
    };
    Some(SweepShape {
        perpendicular: intensify.map_or(*perpendicular, |sweep| sweep.perpendicular),
        reverse: intensify.is_some_and(|sweep| sweep.reverse),
        fixed_direction: intensify.is_some_and(|sweep| sweep.fixed_direction),
        length: changed(*length, intensify.map(|sweep| sweep.length_value)),
        width_q32: changed(*width, intensify.map(|sweep| sweep.width_value)) << 32,
        damage_times: (*damage_times).max(1),
        damage_interval_q32: truncated_q32(*damage_interval),
        damage_delay_q32: truncated_q32(*damage_delay),
        hit_caps: hit_caps
            .iter()
            .map(|cap| (truncated_q32(cap.radius), cap.hits))
            .collect(),
    })
}

/// `SweepSkillData.GetMaxDamageTimesByRadius`: the cap of the first radius
/// in the list at least as large as the unit's, and no cap past the list.
fn hit_cap(hit_caps: &[(i64, u32)], radius_q32: i64) -> u32 {
    hit_caps
        .iter()
        .find(|(radius, _)| radius_q32 <= *radius)
        .map_or(9999, |(_, hits)| *hits)
}

/// `LineRange.Overlaps(CircleRange)`: a circle on either end of the stretch,
/// or one whose centre lies nearer the stretch than its radius and half the
/// width (`FightUtility.CalculateIsDistanceFromPointToLineBiggerThanValue2D`).
fn overlaps(stretch: SweepStretch, width_q32: i64, center: FixedVec2, radius_q32: i64) -> bool {
    if center == stretch.start || center == stretch.end {
        return true;
    }
    let from_start = center.sub(stretch.start);
    let along = stretch.end.sub(stretch.start);
    let length_sq = along.dot(along);
    let distance_sq = if length_sq == 0 {
        from_start.sqr_magnitude()
    } else {
        let dot = from_start.dot(along);
        if dot < 1 {
            from_start.sqr_magnitude()
        } else if dot < length_sq {
            let cross =
                q32_mul(along.x, from_start.y).saturating_sub(q32_mul(along.y, from_start.x));
            q32_div(q32_mul(cross, cross), length_sq)
        } else {
            center.sub(stretch.end).sqr_magnitude()
        }
    };
    let reach = radius_q32.saturating_add(width_q32 / 2);
    rvo::fpoint_less_than(distance_sq, q32_mul(reach, reach))
}

impl Sweep {
    /// `m_damagedTargets` counting each of these once more.
    fn count_struck(&mut self, targets: &[FightActorRef]) {
        for &target in targets {
            match self.struck.iter_mut().find(|(struck, _)| *struck == target) {
                Some((_, count)) => *count += 1,
                None => self.struck.push((target, 1)),
            }
        }
    }

    /// Whether every stretch has been struck.
    pub(in crate::fight) fn over(&self) -> bool {
        self.prepared && self.stretches.is_empty()
    }

    /// `SweepAttackPerformer.Start`.
    pub(in crate::fight) fn starting(
        attack: &AttackConfig,
        intensify: Option<SweepIntensify>,
        aimed: Option<FightActorRef>,
        attack_count: i32,
    ) -> Option<Self> {
        let shape = shape_of(attack, intensify)?;
        let damage_frame = q32_div(shape.damage_interval_q32, NATIVE_LOGIC_DELTA_Q32) >> 32;
        Some(Self {
            stretches: VecDeque::new(),
            prepared: false,
            previous: None,
            struck: Vec::new(),
            delay_q32: shape.damage_delay_q32,
            delta_frame: 0,
            damage_frame: u32::try_from(damage_frame).unwrap_or(1),
            aimed,
            targets_air: false,
            attack_count,
            updated_step: None,
        })
    }
}

impl Simulation {
    fn sweep_point(&self, target: FightActorRef) -> Option<FixedVec2> {
        let view = self.fight_actor(target)?;
        Some(FixedVec2 {
            x: view.x_q32,
            y: view.z_q32,
        })
    }

    /// The centre of the first shield of its side that holds the skill's
    /// lock, unless the sweep crosses shields.
    fn lock_shield(&self, actor_id: u64) -> Option<(i64, i64)> {
        let actor = &self.actors[&actor_id];
        if actor.rules.attack.crosses_shields {
            return None;
        }
        let lock = actor.skills.main.lock_target?;
        let team = self.fight_actor(lock)?.team;
        self.shield
            .standing
            .iter()
            .filter(|shield| shield.team == team)
            .find(|shield| self.shield_holds(shield.id, lock))
            .map(|shield| (shield.x_q32, shield.z_q32))
    }

    /// `FightSweepSkill.CreateAttackArea`: the strip `length` long across
    /// the target, `damage_times` stretches of it, laid out from the end the
    /// attack count picks.
    fn sweep_stretches(
        &self,
        actor_id: u64,
        shape: &SweepShape,
        attack_count: i32,
    ) -> VecDeque<SweepStretch> {
        let actor = &self.actors[&actor_id];
        let skill = &actor.skills.main;
        // Its construction target where it fires at one, its lock otherwise.
        let center_of = skill
            .attack_target()
            .filter(|target| matches!(target, FightActorRef::Building(_)))
            .or(skill.lock_target)
            .or_else(|| skill.attack_target());
        let Some(mut center) = center_of.and_then(|target| self.sweep_point(target)) else {
            return VecDeque::new();
        };
        let owner = FixedVec2 {
            x: actor.x_q32,
            y: actor.z_q32,
        };
        // `FightSweepSkill.CheckIsLockTargetInEnergyShield`: a lock inside a
        // shield of its side, for a sweep that does not cross shields, has
        // the strip centred on the shield where that is nearer the unit.
        if let Some(shield) = self.lock_shield(actor_id) {
            let shield_center = FixedVec2 {
                x: shield.0,
                y: shield.1,
            };
            if shield_center.sub(owner).sqr_magnitude() < center.sub(owner).sqr_magnitude() {
                center = shield_center;
            }
        }
        let mut direction = owner.sub(center);
        if shape.perpendicular {
            direction = FixedVec2 {
                x: direction.y,
                y: direction.x.saturating_neg(),
            };
        }
        if shape.reverse {
            direction = FixedVec2::ZERO.sub(direction);
        }
        // `isEvenAttack`: an even attack sweeps the other way, unless the
        // direction is kept.
        if !shape.fixed_direction && attack_count.rem_euclid(2) == 0 {
            direction = FixedVec2::ZERO.sub(direction);
        }
        let span = direction.normalized().mul(shape.length << 32);
        let half = span.div(2_i64 << 32);
        let start = center.add(half);
        let end = center.sub(half);
        let step = FixedVec2::ZERO
            .sub(span)
            .div(i64::from(shape.damage_times) << 32);
        let mut stretches = VecDeque::new();
        let mut from = start;
        for _ in 1..shape.damage_times {
            let to = from.add(step);
            stretches.push_back(SweepStretch {
                start: from,
                end: to,
            });
            from = to;
        }
        stretches.push_back(SweepStretch { start: from, end });
        stretches
    }

    /// `SweepSkillIntensifyEffectProvider.DisableEffect` and `EnableEffect`
    /// (`TryApply`): off, the unit's sweep skill is reset to its own length,
    /// width, perpendicular and reverse and its row's direction change
    /// (`FightSweepSkill.ResetLength`, `ResetWidth`, `ResetPerpendicular`,
    /// `ResetReverse`); on, the technology's changes are applied again
    /// (`AppliedChange`). The skill holds them, so a sweep under way reads
    /// them from its next update.
    pub(in crate::fight) fn switch_sweep(&mut self, actor_id: u64, on: bool) {
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .sweep_intensified = on;
    }

    /// `SweepAttackPerformer.TryPerformEffect`, once an update while the
    /// sweep lasts: the strip laid out on the first, then a stretch struck
    /// every `damage_frame` updates once the delay is out. Answers whether
    /// the sweep is over.
    pub(in crate::fight) fn update_sweep(
        &mut self,
        actor_id: u64,
        events: &mut Vec<Event>,
    ) -> Result<bool> {
        let actor = &self.actors[&actor_id];
        let Some(shape) = shape_of(&actor.rules.attack, actor.sweep_intensify()) else {
            return Ok(true);
        };
        let Performer::Sweep(sweep) = &self.actors[&actor_id].skills.main.performer else {
            return Ok(true);
        };
        if sweep.updated_step == Some(self.step_now) || sweep.over() {
            return Ok(sweep.over());
        }
        let mut sweep = (**sweep).clone();
        sweep.updated_step = Some(self.step_now);
        if !sweep.prepared {
            sweep.stretches = self.sweep_stretches(actor_id, &shape, sweep.attack_count);
            sweep.previous = None;
            sweep.targets_air = sweep
                .aimed
                .and_then(|aimed| self.fight_actor(aimed))
                .is_some_and(|view| view.domain == UnitDomain::Air);
            sweep.prepared = true;
        }
        if rvo::fpoint_less_than(0, sweep.delay_q32) {
            sweep.delay_q32 = sweep.delay_q32.saturating_sub(NATIVE_LOGIC_DELTA_Q32);
        } else {
            sweep.delta_frame += 1;
            if sweep.delta_frame >= sweep.damage_frame
                && let Some(stretch) = sweep.stretches.pop_front()
            {
                self.strike_stretch(actor_id, &shape, &mut sweep, stretch, events)?;
                sweep.delta_frame = 0;
            }
        }
        let over = sweep.stretches.is_empty();
        // A sweep over is `m_isComplete`: the skill's checks resume on its
        // next update, which ends it.
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skills
            .main
            .performer = Performer::Sweep(Box::new(sweep));
        Ok(over)
    }

    /// `SweepAttackPerformer.Perform`: the stretch and the one before it,
    /// struck at every enemy of the aimed domain it reaches but those already
    /// struck as often as their radius allows or no longer on this stretch.
    fn strike_stretch(
        &mut self,
        actor_id: u64,
        shape: &SweepShape,
        sweep: &mut Sweep,
        stretch: SweepStretch,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let area = sweep.previous.map_or(stretch, |previous| SweepStretch {
            start: previous.start,
            end: stretch.end,
        });
        let circle = |simulation: &Self, target: FightActorRef| {
            let view = simulation.fight_actor(target)?;
            Some((
                FixedVec2 {
                    x: view.x_q32,
                    y: view.z_q32,
                },
                space_to_q32(view.radius),
            ))
        };
        let excluded = sweep
            .struck
            .iter()
            .filter(|(target, count)| {
                !circle(self, *target).is_some_and(|(center, radius)| {
                    *count < hit_cap(&shape.hit_caps, radius)
                        && overlaps(stretch, shape.width_q32, center, radius)
                })
            })
            .map(|(target, _)| *target)
            .collect::<Vec<_>>();
        let team = self.actors[&actor_id].placement.team;
        let domain = if sweep.targets_air {
            UnitDomain::Air
        } else {
            UnitDomain::Ground
        };
        let crosses = self.actors[&actor_id].rules.attack.crosses_shields;
        let owner = FightActorRef::Unit(actor_id);
        // `FightCalculator.IsFightRangeInEnergyShield`: the area's end, its
        // middle or its start, at the aimed domain's height, inside a shield
        // of the other side. A sweep that does not cross shields strikes that
        // shield in place of every unit, unless the unit stands inside it.
        let height = unit_height(domain);
        let middle = area.start.add(area.end).div(2_i64 << 32);
        let shield_struck = if crosses {
            None
        } else {
            [area.end, middle, area.start].iter().find_map(|point| {
                self.shield
                    .standing
                    .iter()
                    .find(|shield| shield.team != team && shield.contains(point.x, height, point.y))
                    .map(|shield| shield.id)
            })
        }
        .filter(|&shield| !self.shield_holds(shield, owner));
        if let Some(shield) = shield_struck {
            let damage = self.main_attack_damage(actor_id);
            let aimed = sweep.aimed.unwrap_or(owner);
            self.beam_at_shield(
                (SkillRef::main(FightActorRef::Unit(actor_id)), 0),
                aimed,
                shield,
                damage,
                events,
            )?;
            sweep.previous = Some(stretch);
            sweep.count_struck(&excluded);
            return Ok(());
        }
        let hits = self
            .target_search_order()
            .into_iter()
            .filter(|(candidate_team, _)| *candidate_team != team)
            .flat_map(|(_, candidates)| candidates)
            .filter(|candidate| {
                !excluded.contains(candidate)
                    && self.fight_actor(*candidate).is_some_and(|view| {
                        view.alive
                            && view.visibility != Visibility::Hide
                            && view.team != team
                            && view.domain == domain
                    })
                    && circle(self, *candidate).is_some_and(|(center, radius)| {
                        overlaps(area, shape.width_q32, center, radius)
                    })
                    // `FightSkill.IsActorProtectedByEnergyShield`: a unit a
                    // shield of its side covers, and the owner is outside of,
                    // is passed over.
                    && (crosses || self.blow_shield(actor_id, *candidate).is_none())
            })
            .collect::<Vec<_>>();
        for target in &hits {
            if self.fight_actor_is_alive(*target) {
                self.direct_effect(actor_id, *target, 0, events)?;
            }
        }
        sweep.previous = Some(stretch);
        // `DamageEffect.PerformInRange` adds what it struck to the list of
        // those passed over, and `Perform` counts every one of them.
        let mut counted = excluded;
        counted.extend(hits);
        sweep.count_struck(&counted);
        Ok(())
    }
}
