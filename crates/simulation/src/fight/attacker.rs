//! What a skill asks of whoever owns it: `ISkillOwner` and `IAttacker`.
//!
//! The build runs one skill machine for every owner. `FightMech.Update` and
//! `FightConstruction.Update` both call `SkillManager.Update` and then
//! `SkillManager.UpdateWeaponRotateion`, and the machine's states
//! (`SkillIdleState`, `SkillAttackState`, `SkillReloadingState`, the
//! checkers) never ask what kind of owner they serve. What differs between a
//! unit and a construction reaches them only through the two interfaces: where
//! the owner stands and how big it is (`GetMainTransform`, `GetRadius`), how
//! far it reaches (`GetAttackRange`, `GetMinAttackRange`), what its attack
//! angle is measured against (`GetAttackAngle`), how fast its weapons turn
//! (`GetRotateSpeed`), and whether it searches at all
//! (`IsMechSearchTargetEnabled`).
//!
//! [`Attacker`] is that answer, read once from the owner. Everything the skill
//! decides from it is written once, here and in `skill/`; a question that
//! needs an owner's kind is a question these interfaces do not ask.

use super::*;

/// What an owner's attack angle is measured against.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) enum Facing<'a> {
    /// The owner's own rotation: a unit without a body, whose weapon has no
    /// transform of its own, falls back to the `FightMech` transform in
    /// `SkillAttackAngleChecker`.
    Root(i64),
    /// Every weapon's rotation: a unit with a body, and a construction.
    Weapons(&'a [i64]),
}

impl Facing<'_> {
    /// Whether a bearing is within half an attack angle of the root, or of
    /// every weapon; an owner with no weapon faces nothing.
    pub(in crate::fight) fn faces(self, bearing_q32: i64, half_angle_mdeg: i64) -> bool {
        let half_angle_q32 = mdeg_to_degrees_q32(half_angle_mdeg);
        match self {
            Self::Root(rotation) => rotation_distance_q32(rotation, bearing_q32) <= half_angle_q32,
            Self::Weapons(rotations) => {
                !rotations.is_empty()
                    && rotations.iter().all(|rotation| {
                        rotation_distance_q32(*rotation, bearing_q32) <= half_angle_q32
                    })
            }
        }
    }
}

/// A skill's owner as the skill sees it.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct Attacker<'a> {
    pub(in crate::fight) owner: FightActorRef,
    pub(in crate::fight) team: u32,
    /// Where it stands, in space units and in Q32.32.
    pub(in crate::fight) x: i64,
    pub(in crate::fight) z: i64,
    pub(in crate::fight) x_q32: i64,
    pub(in crate::fight) z_q32: i64,
    /// Where the tick's searches measure it from, and the rotation they score
    /// against: what the target trees held at the tick's start.
    pub(in crate::fight) query_x_q32: i64,
    pub(in crate::fight) query_z_q32: i64,
    pub(in crate::fight) query_rotation_q32: i64,
    /// `ISkillOwner.GetRadius`: reach is measured edge to edge from it.
    pub(in crate::fight) radius: i64,
    /// How high its shots leave from.
    pub(in crate::fight) y: i64,
    /// The skill row.
    pub(in crate::fight) attack: &'a AttackConfig,
    /// `IAttacker.GetAttackRange`, with whatever corrects it.
    pub(in crate::fight) attack_range: i64,
    /// What one blow deals, with whatever corrects it.
    pub(in crate::fight) attack_damage: i64,
    /// The attack interval, in native time units, with whatever corrects it.
    pub(in crate::fight) attack_interval: u64,
    pub(in crate::fight) facing: Facing<'a>,
    /// Whether its weapons turn on a transform of their own, which the attack
    /// angle is then measured from: a unit with a body, and a construction.
    /// A unit without one falls back to its root.
    pub(in crate::fight) has_body: bool,
    /// `ISkillOwner.GetRotateSpeed`, as one update's turn in Q32.32 degrees.
    pub(in crate::fight) turn_q32: i64,
    /// Whether its skill searches at all: a construction without a
    /// `ConstructionSearchTargetController`, which its row's
    /// `IsEnableSearchTarget` decides, never finds a target.
    pub(in crate::fight) searches: bool,
}

impl Attacker<'_> {
    /// Whether a target of this radius at this point is within reach, edge to
    /// edge: `IAttacker.IsActorInAttackRange`.
    pub(in crate::fight) fn reaches(&self, x_q32: i64, z_q32: i64, radius: i64) -> bool {
        let edge_distance_q32 = native_q32_magnitude(
            x_q32.saturating_sub(self.x_q32),
            z_q32.saturating_sub(self.z_q32),
        )
        .saturating_sub(space_to_q32(self.radius))
        .saturating_sub(space_to_q32(radius))
        .max(0);
        edge_distance_q32 >= space_to_q32(self.attack.min_range())
            && edge_distance_q32 <= space_to_q32(self.attack_range)
    }

    /// Whether a bearing is within the attack angle of what the angle is
    /// measured against: `SkillAttackAngleChecker.IsAttackTargetInAttackAngle`.
    pub(in crate::fight) fn faces(&self, bearing_q32: i64) -> bool {
        self.facing
            .faces(bearing_q32, self.attack.attack_half_angle_mdeg())
    }

    /// The bearing from the owner to a point.
    pub(in crate::fight) fn bearing_q32(&self, x_q32: i64, z_q32: i64) -> i64 {
        direction_degrees_q32_raw(
            x_q32.saturating_sub(self.x_q32),
            z_q32.saturating_sub(self.z_q32),
        )
    }

    /// Where its shot leaves from and what it carries.
    pub(in crate::fight) fn launch(&self) -> Launch {
        Launch {
            owner: self.owner,
            team: self.team,
            x: self.x,
            z: self.z,
            x_q32: self.x_q32,
            y: self.y,
            z_q32: self.z_q32,
            speed: self.attack.projectile_speed(),
            life: self.attack.projectile_life(),
            lock_target: self.attack.lock_target,
        }
    }
}

impl Simulation {
    /// The owner of a skill, as its skill sees it.
    pub(in crate::fight) fn attacker(&self, owner: FightActorRef) -> Option<Attacker<'_>> {
        match owner {
            FightActorRef::Unit(id) => {
                let actor = self.actors.get(&id)?;
                Some(Attacker {
                    owner,
                    team: actor.placement.team,
                    x: actor.x,
                    z: actor.z,
                    x_q32: actor.x_q32,
                    z_q32: actor.z_q32,
                    query_x_q32: actor.target_query_x_q32,
                    query_z_q32: actor.target_query_z_q32,
                    query_rotation_q32: actor.target_query_source_rotation_q32,
                    radius: actor.rules.collision_radius(),
                    y: unit_height(actor.rules.domain),
                    attack: &actor.rules.attack,
                    attack_range: actor.stats.attack_range(),
                    attack_damage: actor.stats.attack_damage(),
                    attack_interval: actor.stats.attack_interval(),
                    facing: if actor.rules.has_body {
                        Facing::Weapons(&actor.skill.weapon_rotations_q32)
                    } else {
                        Facing::Root(actor.body_rotation_q32)
                    },
                    has_body: actor.rules.has_body,
                    turn_q32: actor.turn_q32(),
                    searches: true,
                })
            }
            FightActorRef::Building(id) => {
                let construction = self.constructions.get(&id)?;
                // A construction does not move, and its weapon turns only
                // after its own skill has searched, so where it stood and
                // pointed at the tick's start is where it stands and points.
                let rotation = construction
                    .skill
                    .weapon_rotations_q32
                    .first()
                    .copied()
                    .unwrap_or(0);
                Some(Attacker {
                    owner,
                    team: construction.team,
                    x: construction.x,
                    z: construction.z,
                    x_q32: construction.x_q32,
                    z_q32: construction.z_q32,
                    query_x_q32: construction.x_q32,
                    query_z_q32: construction.z_q32,
                    query_rotation_q32: rotation,
                    radius: construction.radius,
                    y: 0,
                    attack: &construction.attack,
                    attack_range: construction.attack.range(),
                    attack_damage: construction.attack.base_damage,
                    attack_interval: construction.attack.interval_time_units(),
                    facing: Facing::Weapons(&construction.skill.weapon_rotations_q32),
                    has_body: true,
                    turn_q32: construction.turn_q32,
                    searches: construction.searches,
                })
            }
        }
    }

    /// The skill an owner runs.
    pub(in crate::fight) fn skill(&self, owner: FightActorRef) -> &Skill {
        match owner {
            FightActorRef::Unit(id) => &self.actors[&id].skill,
            FightActorRef::Building(id) => &self.constructions[&id].skill,
        }
    }

    pub(in crate::fight) fn skill_mut(&mut self, owner: FightActorRef) -> &mut Skill {
        match owner {
            FightActorRef::Unit(id) => {
                &mut self
                    .actors
                    .get_mut(&id)
                    .expect("actor identity is stable")
                    .skill
            }
            FightActorRef::Building(id) => {
                &mut self
                    .constructions
                    .get_mut(&id)
                    .expect("construction identity is stable")
                    .skill
            }
        }
    }

    /// `ISkillData`'s quick switch: whether the skill takes the next target
    /// in the middle of its attack.
    pub(in crate::fight) fn quick_switch_target(&self, owner: FightActorRef) -> bool {
        self.attacker(owner)
            .expect("skill owner identity is stable")
            .attack
            .quick_switch_target
    }

    /// The unit whose motion a skill's decisions reach, if the owner has
    /// motion that runs.
    ///
    /// `FightConstruction` builds a `MotionController` but its `Update` never
    /// runs one (`ConstructionSearchTargetController`, `SkillManager`,
    /// `BuffManager`, and nothing else): a construction's motion never leaves
    /// idle, and nothing it would publish moves anything. The skill machine
    /// itself asks no motion anything; what reaches the motion here is the
    /// kernel's, where a unit's motion stops with its attack.
    pub(in crate::fight) fn moving_mut(&mut self, owner: FightActorRef) -> Option<&mut Actor> {
        match owner {
            FightActorRef::Unit(id) => self.actors.get_mut(&id),
            FightActorRef::Building(_) => None,
        }
    }

    /// The state of the owner's motion: a construction's reads idle.
    pub(in crate::fight) fn motion_state(&self, owner: FightActorRef) -> MotionState {
        match owner {
            FightActorRef::Unit(id) => self.actors[&id].motion.state,
            FightActorRef::Building(_) => MotionState::Idle,
        }
    }

    /// Whether the owner's motion holds its fire while it turns to face.
    pub(in crate::fight) fn attack_hold_fire(&self, owner: FightActorRef) -> bool {
        match owner {
            FightActorRef::Unit(id) => self.actors[&id].motion.attack_hold_fire,
            FightActorRef::Building(_) => false,
        }
    }

    /// Whether a target is alive, targetable and within the owner's reach:
    /// `SkillAttackRangeChecker`.
    pub(in crate::fight) fn target_in_attack_range(
        &self,
        owner: FightActorRef,
        target: FightActorRef,
    ) -> bool {
        let (Some(attacker), Some(view)) = (self.attacker(owner), self.fight_actor(target)) else {
            return false;
        };
        view.alive && view.targetable && attacker.reaches(view.x_q32, view.z_q32, view.radius)
    }

    /// Whether a target is within the owner's attack angle:
    /// `SkillAttackAngleChecker`.
    pub(in crate::fight) fn target_in_attack_angle(
        &self,
        owner: FightActorRef,
        target: FightActorRef,
    ) -> bool {
        let (Some(attacker), Some(view)) = (self.attacker(owner), self.fight_actor(target)) else {
            return false;
        };
        view.alive && attacker.faces(attacker.bearing_q32(view.x_q32, view.z_q32))
    }

    /// Both: `FightSkill.IsAttackTargetInAttackArea`.
    pub(in crate::fight) fn target_in_attack_area(
        &self,
        owner: FightActorRef,
        target: FightActorRef,
    ) -> bool {
        self.target_in_attack_range(owner, target) && self.target_in_attack_angle(owner, target)
    }

    /// `SkillManager.UpdateWeaponRotateion`: every weapon turns towards a
    /// bearing at the owner's rotate speed.
    pub(in crate::fight) fn turn_weapons_towards(
        &mut self,
        owner: FightActorRef,
        bearing_q32: i64,
    ) {
        let turn_q32 = self
            .attacker(owner)
            .expect("skill owner identity is stable")
            .turn_q32;
        self.skill_mut(owner)
            .turn_weapons_towards(bearing_q32, turn_q32);
    }

    /// Draws an attack interval for an owner's skill from its side's stream:
    /// the description plus a stagger, never under one tick. A skill without a
    /// stagger leaves the stream to the next owner.
    pub(in crate::fight) fn draw_attack_interval(&mut self, owner: FightActorRef) -> Result<u64> {
        let attacker = self
            .attacker(owner)
            .ok_or_else(|| Error::new("attack interval owner is absent"))?;
        let team = attacker.team;
        let interval_steps = native_time_units_to_steps(attacker.attack_interval);
        let offset_steps = native_time_units_to_steps(attacker.attack.interval_offset_time_units());
        let random = self
            .team_random
            .get_mut(&team)
            .ok_or_else(|| Error::new("attack interval team random stream is absent"))?;
        Ok(draw_interval(random, interval_steps, offset_steps))
    }
}

impl Simulation {
    /// Every skill's first interval, drawn at deployment from its side's
    /// stream: one draw per skill the owner runs, units in identity order and
    /// then constructions, and the first draw kept as the owner's current
    /// interval. Nothing schedules an attack yet, so the draw is kept rather
    /// than consumed and discarded.
    ///
    /// A side's stream is seeded `(round + team) × 4444`. The Anti-Armor
    /// Turret's shots in `tests/turret/` are 49 48 52 50 50 ticks apart, which
    /// is blue's stream past the Marksman's draw and one more: the
    /// construction draws after every unit.
    pub(in crate::fight) fn deploy_attack_intervals(&mut self, round: u32) -> Result<()> {
        let owners = self
            .actors
            .keys()
            .map(|&id| FightActorRef::Unit(id))
            .chain(
                self.constructions
                    .keys()
                    .map(|&id| FightActorRef::Building(id)),
            )
            .collect::<Vec<_>>();
        for owner in owners {
            let attacker = self
                .attacker(owner)
                .expect("skill owner identity is stable");
            let team = attacker.team;
            let skills = if attacker.attack.weapons.mode == WeaponMode::Group {
                attacker.attack.weapons.count()
            } else {
                1
            };
            self.team_random.entry(team).or_insert_with(|| {
                GrRandom::new(u64::from(
                    round
                        .cast_signed()
                        .wrapping_add(team.cast_signed())
                        .wrapping_mul(4_444)
                        .cast_unsigned(),
                ))
            });
            for index in 0..skills {
                let interval = self.draw_attack_interval(owner)?;
                if index == 0 {
                    self.skill_mut(owner).current_attack_interval = interval;
                }
            }
        }
        Ok(())
    }
}

/// An interval of `interval_steps` staggered by a draw in
/// `[-offset_steps, offset_steps]`, never under one tick.
pub(in crate::fight) fn draw_interval(
    random: &mut GrRandom,
    interval_steps: u64,
    offset_steps: u64,
) -> u64 {
    let sample = if offset_steps == 0 {
        0
    } else {
        i64::from(random.next_in_range(i32::try_from(offset_steps).unwrap_or(i32::MAX)))
    };
    i64::try_from(interval_steps)
        .unwrap_or(i64::MAX)
        .saturating_add(sample)
        .max(1)
        .cast_unsigned()
}
