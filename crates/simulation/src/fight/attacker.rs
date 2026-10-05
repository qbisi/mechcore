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
use crate::data::{Index, Overlay, switched_targets};

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
    /// The skill whose view of its owner this is.
    pub(in crate::fight) skill: SkillRef,
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
    /// Which domains it attacks: the row's, switched by its `DataSet`
    /// (`FightSkill.IsAirAttack`, `IsGroundAttack`).
    pub(in crate::fight) targets: AttackTargets,
    /// What its `ProjectileSpeedValue` adds to the row's projectile speed,
    /// millimetres a second.
    pub(in crate::fight) projectile_speed_add: i64,
    /// `IAttacker.GetAttackRange`, with whatever corrects it.
    pub(in crate::fight) attack_range: i64,
    /// What one blow deals, with whatever corrects it.
    pub(in crate::fight) attack_damage: i64,
    /// `FightSkill.GetSplashRange`: the description's splash with the
    /// skill's `SplashRangeValue` added.
    pub(in crate::fight) splash_radius: i64,
    /// The attack interval, in Q32.32 seconds, with whatever corrects it.
    pub(in crate::fight) attack_interval_q32: i64,
    pub(in crate::fight) facing: Facing<'a>,
    /// Whether its weapons turn on a transform of their own, which the attack
    /// angle is then measured from: a unit with a body, and a construction.
    /// A unit without one falls back to its root.
    pub(in crate::fight) has_body: bool,
    /// `ISkillOwner.GetRotateSpeed`, as one update's turn in Q32.32 degrees.
    pub(in crate::fight) turn_q32: i64,
    /// The rotation window `Selector.CalculateRotationData` hands
    /// `CalculateScore`, as how far it reaches to the left and to the right
    /// of the rotation it scores against: a candidate in range outside it
    /// takes the out-of-range penalty. A unit's window is the whole turn,
    /// `Angle0` to `Angle360`, which `CalculateScore` does not check; a
    /// construction's skill's is its attack angle either side of its weapon,
    /// at every rotation, as the recorded `CalculateScore` arguments show,
    /// and a weapon turning within an arc has the arc widened by the attack
    /// angle. The construction's own search, whose lock nothing aims or fires
    /// from, has none.
    pub(in crate::fight) rotation_window_q32: Option<(i64, i64)>,
    /// Whether its skill searches at all: a construction without a
    /// `ConstructionSearchTargetController`, which its row's
    /// `IsEnableSearchTarget` decides, never finds a target.
    pub(in crate::fight) searches: bool,
    /// What its search counts off a candidate's distance by the candidate's
    /// domain, millimetres: the `airTargetDistanceScoreOffset` and
    /// `groundTargetDistanceScoreOffset` that
    /// `SearchTargetController.SetTargetSelector` hands a `DistanceIntensify`
    /// selector from the skill's `AttackRangeValueAir` and
    /// `AttackRangeValueGround`, and none a `Normal` one gets.
    pub(in crate::fight) score_offsets: ScoreOffsets,
}

/// A selector's distance offsets by domain, millimetres.
#[derive(Debug, Clone, Copy, Default)]
pub(in crate::fight) struct ScoreOffsets {
    air: i64,
    ground: i64,
}

impl ScoreOffsets {
    /// What `Selector.Calculate` hands `DistanceScoreCalculator.Calculate`
    /// for a candidate, which counts it off the distance: the offset for its
    /// domain, less `ScoreRatingTargetSelector.invisibleActorDistanceScoreOffset`
    /// for one that is not visible, which is minus 40 metres.
    pub(in crate::fight) fn for_candidate(self, domain: UnitDomain, visible: bool) -> i64 {
        let offset = match domain {
            UnitDomain::Air => self.air,
            UnitDomain::Ground => self.ground,
        };
        if visible {
            offset
        } else {
            offset.saturating_sub(INVISIBLE_DISTANCE_SCORE_OFFSET)
        }
    }
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
            speed: self
                .attack
                .projectile_speed()
                .saturating_add(self.projectile_speed_add),
            life: self.attack.projectile_life(),
            interceptible: self.attack.projectile_interceptible(),
            lock_target: self.attack.lock_target,
            climb: self.attack.projectile_pre_flight_height(),
            range: self.attack_range,
        }
    }
}

impl Simulation {
    /// A side's `FightTeam.random`.
    pub(in crate::fight) fn side_random(&mut self, team: u32) -> Result<&mut GrRandom> {
        self.team_random
            .get_mut(&team)
            .ok_or_else(|| Error::new(format!("side {team} has no random stream")))
    }

    /// `FightActor.ChangeTeam`: a unit changing side leaves its old side's
    /// list and `FightTeamController.AddActor` appends it to its new one's,
    /// so it updates after every unit already there.
    pub(in crate::fight) fn joins_side_last(&mut self, unit_id: u64) {
        self.unit_update_order.retain(|&id| id != unit_id);
        self.unit_update_order.push(unit_id);
    }

    /// Every unit in the order its side updates it: the deployed ones in the
    /// fight's update order, then each summon and each unit turned in the
    /// order it joined, and anything else made since in identity order.
    pub(in crate::fight) fn units_in_update_order(&self) -> Vec<u64> {
        let deployed = self
            .unit_update_order
            .iter()
            .copied()
            .filter(|id| self.actors.contains_key(id));
        let made = self
            .actors
            .keys()
            .copied()
            .filter(|id| !self.unit_update_order.contains(id));
        deployed.chain(made).collect()
    }

    /// The owner of a skill, as its skill sees it.
    /// The unit itself as the attacker its own search scores for
    /// (`FightMech` as `IAttacker`): its root's transform where it stands as
    /// it updates, the whole turn its window, and its main skill's range.
    pub(in crate::fight) fn mech_attacker(&self, actor_id: u64) -> Option<Attacker<'_>> {
        let actor = self.actors.get(&actor_id)?;
        let mut source = self.attacker(FightActorRef::Unit(actor_id))?;
        source.query_x_q32 = actor.x_q32;
        source.query_z_q32 = actor.z_q32;
        source.query_rotation_q32 = actor.body_rotation_q32;
        source.rotation_window_q32 = None;
        // `MechSearchTargetController` keeps the selector it was made with.
        source.score_offsets = ScoreOffsets::default();
        Some(source)
    }

    /// `FightSkill.GetAttackRange` of a unit's main skill, which
    /// `FightMech.GetAttackRange` answers too: its `AttackRangeAirProperty`
    /// while it locks a unit that flies, and its `AttackRangeGroundProperty`
    /// otherwise.
    pub(in crate::fight) fn main_attack_range(&self, actor_id: u64) -> i64 {
        let actor = &self.actors[&actor_id];
        actor
            .stats
            .attack_range_against(self.main_lock_domain(actor))
    }

    /// [`Self::main_attack_range`] in Q32.32 metres.
    pub(in crate::fight) fn main_attack_range_q32(&self, actor_id: u64) -> i64 {
        let actor = &self.actors[&actor_id];
        actor
            .stats
            .attack_range_q32_against(self.main_lock_domain(actor))
    }

    /// `DamageCalculator.GetAttackDamage` of a unit's main skill: its
    /// `AirDamageProperty` while it attacks a unit that flies, and its
    /// `GroundDamageProperty` otherwise.
    pub(in crate::fight) fn main_attack_damage(&self, actor_id: u64) -> i64 {
        let actor = &self.actors[&actor_id];
        actor
            .stats
            .attack_damage_against(self.attack_domain(actor.skills.main.attack_target()))
    }

    /// Whether what a unit's main skill locks flies (`lockTarget.IsFly`).
    fn main_lock_domain(&self, actor: &Actor) -> UnitDomain {
        self.attack_domain(actor.skills.main.lock_target)
    }

    /// Whether a skill's target flies, none standing on the ground.
    pub(in crate::fight) fn attack_domain(&self, target: Option<FightActorRef>) -> UnitDomain {
        target.map_or(UnitDomain::Ground, |target| self.domain_of(target))
    }

    pub(in crate::fight) fn attacker(&self, owner: FightActorRef) -> Option<Attacker<'_>> {
        match owner {
            FightActorRef::Unit(id) => {
                let actor = self.actors.get(&id)?;
                Some(Attacker {
                    owner,
                    skill: SkillRef::main(owner),
                    team: actor.placement.team,
                    x: actor.x,
                    z: actor.z,
                    x_q32: actor.x_q32,
                    z_q32: actor.z_q32,
                    query_x_q32: actor.target_query_x_q32,
                    query_z_q32: actor.target_query_z_q32,
                    query_rotation_q32: actor
                        .default_search_frame(&actor.rules.attack, 0)
                        .map_or(actor.target_query_source_rotation_q32, |(rotation, _)| {
                            rotation
                        }),
                    radius: actor.rules.collision_radius(),
                    y: unit_height(actor.rules.domain),
                    attack: &actor.rules.attack,
                    targets: actor.stats.targets(actor.rules.attack.targets),
                    projectile_speed_add: actor.stats.projectile_speed_add(),
                    attack_range: self.main_attack_range(id),
                    attack_damage: self.main_attack_damage(id),
                    splash_radius: actor.stats.splash_radius(),
                    attack_interval_q32: actor.stats.attack_interval_q32(),
                    // A standalone weapon's skill faces with its own weapon:
                    // the unit's main skill is the first gun's.
                    facing: if actor.rules.has_body && actor.skills.main.standalone() {
                        Facing::Weapons(&actor.skills.main.weapon_rotations_q32[..1])
                    } else if actor.rules.has_body {
                        Facing::Weapons(&actor.skills.main.weapon_rotations_q32)
                    } else {
                        Facing::Root(actor.body_rotation_q32)
                    },
                    has_body: actor.rules.has_body,
                    turn_q32: actor.turn_q32(),
                    rotation_window_q32: actor
                        .default_search_frame(&actor.rules.attack, 0)
                        .and_then(|(_, window)| window),
                    searches: true,
                    // `SearchTargetSpecificProvider.DoEnable` turns the main
                    // skill's selector to `DistanceIntensify` after it has
                    // written the values the selector reads.
                    score_offsets: if actor.placement.distance_intensify {
                        ScoreOffsets {
                            air: actor.stats.score_offset_for(UnitDomain::Air),
                            ground: actor.stats.score_offset_for(UnitDomain::Ground),
                        }
                    } else {
                        ScoreOffsets::default()
                    },
                })
            }
            FightActorRef::Building(id) => {
                let construction = self.constructions.get(&id)?;
                // A construction does not move, and its weapon turns only
                // after its own skill has searched, so where it stood and
                // pointed at the tick's start is where it stands and points.
                let rotation = construction
                    .skills
                    .main
                    .weapon_rotations_q32
                    .first()
                    .copied()
                    .unwrap_or(0);
                Some(Attacker {
                    owner,
                    skill: SkillRef::main(owner),
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
                    targets: construction.attack.targets,
                    projectile_speed_add: 0,
                    attack_range: construction.attack.range(),
                    attack_damage: construction.attack_damage,
                    splash_radius: construction.attack.splash_radius(),
                    attack_interval_q32: time_units_to_seconds_q32(
                        construction.attack.interval_time_units(),
                    ),
                    facing: Facing::Weapons(&construction.skills.main.weapon_rotations_q32),
                    has_body: true,
                    turn_q32: construction.turn_q32,
                    rotation_window_q32: Some({
                        let half =
                            mdeg_to_degrees_q32(construction.attack.attack_half_angle_mdeg());
                        (half, half)
                    }),
                    searches: construction.searches,
                    score_offsets: ScoreOffsets::default(),
                })
            }
        }
    }

    /// One skill's owner as that skill sees it. An owner's main skill is what
    /// [`Self::attacker`] answers. An extra skill answers from its own row:
    /// its reach (`FightSkill.GetAttackRange`, the main skill's where its row
    /// uses the main skill's range), the damage its row states for the
    /// unit's level or its rate of the unit's base damage, its splash and its
    /// interval, and its own weapon, which
    /// turns on a transform of its own, as what its attack angle is measured
    /// against. No correction reaches an extra skill here but a damage one on
    /// a skill with a damage rate: the layout refuses a unit any other would
    /// reach.
    pub(in crate::fight) fn skill_attacker(&self, skill_ref: SkillRef) -> Option<Attacker<'_>> {
        let SkillSlot::Extra(index) = skill_ref.slot else {
            return self.attacker(skill_ref.owner);
        };
        let FightActorRef::Unit(id) = skill_ref.owner else {
            return None;
        };
        let actor = self.actors.get(&id)?;
        let extra = actor.skills.extras.get(index)?;
        let rules = &extra.rules;
        let mut attacker = self.attacker(skill_ref.owner)?;
        attacker.skill = skill_ref;
        attacker.attack = &rules.attack;
        // A skill with a damage rate holds the main skill's `DataSet`; any
        // other holds what reaches it alone.
        (attacker.targets, attacker.projectile_speed_add) = if rules.damage_rate > 0.0 {
            (
                actor.stats.targets(rules.attack.targets),
                actor.stats.projectile_speed_add(),
            )
        } else {
            let own = Overlay::of(&extra.skill_corrections);
            (
                switched_targets(rules.attack.targets, &own),
                own.value(Index::ProjectileSpeed),
            )
        };
        // Only the main skill's search is turned to `DistanceIntensify`.
        attacker.score_offsets = ScoreOffsets::default();
        // A row that uses the main skill's range reaches its own range
        // beyond it: the Secondary Armament's 2 metres past the main gun's,
        // 107 against 105 in the recorded searches. So does every skill of a
        // grouped row, whose `ParentSkill` is the main skill
        // (`FightSkillFactory.PrepareGroupedSkill`) and whose
        // `FightSkill.GetAttackRange` is its parent's with its own
        // `SkillDataFloat.AttackRange` added: Energy Diffraction's beams
        // reach 10 metres past the Melting Point's 85.
        let parented = rules.attack.weapons.mode == WeaponMode::Group;
        attacker.attack_range = if rules.use_main_skill_range || parented {
            attacker.attack_range.saturating_add(rules.attack.range())
        } else {
            rules.attack.range()
        };
        // `SkillData.GetDamage`: the entry for the unit's level, the last for
        // a level beyond the list, and none for a row that lists none: an
        // Incendiary Bomb's hit leaves its fire and nothing else, and a
        // Homing Missile deals its one entry at every level.
        let against = self.attack_domain(extra.skill.attack_target());
        attacker.attack_damage = if rules.damage_rate > 0.0 {
            // `DamageProperty.RefreshBaseDamage`: the unit's base damage at
            // its level (`FightMech.GetBaseDamage`) times the skill's rate,
            // truncated: a Rhino's Whirlwind deals 4983 of its 3560 at 1.4.
            let base = actor
                .rules
                .attack
                .base_damage
                .saturating_mul(actor.placement.level);
            // `DamageProperty.CalculateDamage` then corrects it by the
            // skill's `DataSet`, which holds what reaches the main skill, and
            // the buffs', its damage property the one for what it attacks.
            actor
                .stats
                .damage_from(
                    q32_mul(base << 32, crate::rules::metres_q32(rules.damage_rate)) >> 32,
                    against,
                )
                .ok()?
        } else {
            let own = rules.damage_by_level.last().map_or(0, |last| {
                usize::try_from(actor.placement.level - 1)
                    .ok()
                    .and_then(|level| rules.damage_by_level.get(level))
                    .unwrap_or(last)
                    .to_owned()
            });
            // `DamageProperty.CalculateDamage` corrects it by what its own
            // `DataSet` holds, an equipment's and an Energy Tower skill's,
            // and the buffs'.
            actor
                .stats
                .damage_with(own, &extra.skill_corrections, against)
                .ok()?
        };
        attacker.splash_radius = rules.attack.splash_radius();
        attacker.attack_interval_q32 =
            time_units_to_seconds_q32(rules.attack.interval_time_units());
        // A weapon without an arc has no transform of its own, and points
        // where what it is mounted on points: the turret it is mounted on, or
        // the unit. The Hound's bombs score from the unit's rotation, as its
        // main skill does.
        let mount_rotation = actor.mount_rotation_q32(rules.attack.weapons.mount);
        let rotation = if extra.arc().is_some() {
            attacker.facing = Facing::Weapons(&extra.skill.weapon_rotations_q32);
            attacker.has_body = true;
            extra.skill.weapon_rotations_q32[0]
        } else {
            attacker.facing = Facing::Root(mount_rotation);
            mount_rotation
        };
        // `SkillSearchTargetController.PrepareSearch` does nothing: an extra
        // skill's search scores where everything stands as it updates, from
        // its own weapon's rotation. A weapon held to an arc passes over what
        // lies outside the arc widened by the attack angle either side, the
        // arc about its rest as the unit stands now, whichever way the
        // weapon points: the window is stated about the weapon's rotation.
        attacker.query_x_q32 = actor.x_q32;
        attacker.query_z_q32 = actor.z_q32;
        attacker.query_rotation_q32 = rotation;
        attacker.rotation_window_q32 = extra.arc().and_then(|arc| {
            let (left, right) = arc.left.zip(arc.right)?;
            let parent = match (rules.attack.weapons.mount, actor.turret_rotation()) {
                (WeaponMount::MechBody, Some(turret)) => turret,
                _ => actor.body_rotation_q32,
            };
            let rest = parent.saturating_add(i64::from(arc.default) << 32);
            let off_rest =
                (rotation - rest + (180_i64 << 32)).rem_euclid(360_i64 << 32) - (180_i64 << 32);
            let angle = mdeg_to_degrees_q32(rules.attack.attack_half_angle_mdeg());
            Some((
                off_rest + (i64::from(left) << 32) + angle,
                (i64::from(right) << 32) + angle - off_rest,
            ))
        });
        Some(attacker)
    }

    /// `FightSkill.IsTowerAttackable`: a main skill may take a tower; an
    /// extra skill only where it deals damage of its own or a share of its
    /// unit's (`GetDamage`, `GetDamageRate`), which an explosion's rate of
    /// one does. Incendiary Bomb's shell, dealing nothing, passes towers over.
    pub(in crate::fight) fn tower_attackable(&self, skill_ref: SkillRef) -> bool {
        let SkillSlot::Extra(index) = skill_ref.slot else {
            return true;
        };
        let FightActorRef::Unit(id) = skill_ref.owner else {
            return true;
        };
        let rules = &self.actors[&id].skills.extras[index].rules;
        rules.explosion.is_some()
            || self
                .skill_attacker(skill_ref)
                .is_some_and(|attacker| attacker.attack_damage != 0)
    }

    /// A skill's index in its owner's `FightMech.GetSkills()`, which a
    /// recording names it by: the main skill, or each of its group's skills,
    /// comes first, and the extra skills after it in their order.
    pub(in crate::fight) fn skill_slot(&self, skill_ref: SkillRef) -> usize {
        match skill_ref.slot {
            SkillSlot::Main => 0,
            SkillSlot::Extra(index) => self.skills(skill_ref.owner).extra_first_slot(index),
        }
    }

    /// The skill a slot of an owner's `GetSkills()` names: one of the main
    /// skill's for a slot among its group's, an extra skill's after them.
    pub(in crate::fight) fn skill_at_slot(&self, owner: FightActorRef, slot: usize) -> SkillRef {
        SkillRef {
            owner,
            slot: self.skills(owner).at_slot(slot).0,
        }
    }

    /// An owner's skills.
    pub(in crate::fight) fn skills(&self, owner: FightActorRef) -> &SkillManager {
        match owner {
            FightActorRef::Unit(id) => &self.actors[&id].skills,
            FightActorRef::Building(id) => &self.constructions[&id].skills,
        }
    }

    /// One skill an owner runs.
    pub(in crate::fight) fn skill(&self, skill_ref: SkillRef) -> &Skill {
        self.skills(skill_ref.owner).get(skill_ref.slot)
    }

    pub(in crate::fight) fn skill_mut(&mut self, skill_ref: SkillRef) -> &mut Skill {
        match skill_ref.owner {
            FightActorRef::Unit(id) => self
                .actors
                .get_mut(&id)
                .expect("actor identity is stable")
                .skills
                .get_mut(skill_ref.slot),
            FightActorRef::Building(id) => self
                .constructions
                .get_mut(&id)
                .expect("construction identity is stable")
                .skills
                .get_mut(skill_ref.slot),
        }
    }

    /// The row a skill runs from: its owner's for the main skill, its
    /// technology's for an extra one.
    pub(in crate::fight) fn skill_rules(&self, skill_ref: SkillRef) -> &AttackConfig {
        self.skill_attacker(skill_ref)
            .expect("skill owner identity is stable")
            .attack
    }

    /// `ISkillData`'s quick switch: whether the skill takes the next target
    /// in the middle of its attack.
    pub(in crate::fight) fn quick_switch_target(&self, skill_ref: SkillRef) -> bool {
        self.skill_attacker(skill_ref)
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
    ///
    /// Only the main skill's decisions reach the motion: `ChangeLockTarget`
    /// hands the owner a lock only from its main searcher
    /// (`FightSkillBase.IsMainSearcher`), and an extra skill is not one.
    pub(in crate::fight) fn moving_mut(&mut self, skill_ref: SkillRef) -> Option<&mut Actor> {
        match (skill_ref.owner, skill_ref.slot) {
            (FightActorRef::Unit(id), SkillSlot::Main) => self.actors.get_mut(&id),
            _ => None,
        }
    }

    /// The state of the owner's motion: a construction's reads idle.
    pub(in crate::fight) fn motion_state(&self, owner: FightActorRef) -> MotionState {
        match owner {
            FightActorRef::Unit(id) => self.actors[&id].motion.state,
            FightActorRef::Building(_) => MotionState::Idle,
        }
    }

    /// Whether a target is alive, targetable and within the owner's reach:
    /// `SkillAttackRangeChecker`.
    pub(in crate::fight) fn target_in_attack_range(
        &self,
        skill_ref: SkillRef,
        target: FightActorRef,
    ) -> bool {
        let (Some(attacker), Some(view)) =
            (self.skill_attacker(skill_ref), self.fight_actor(target))
        else {
            return false;
        };
        // A skill firing at a shield reaches it once the point of the
        // shield's surface on its way to the lock is in range, from the
        // owner's edge: `SkillAttackRangeChecker.IsAttackTargetInAttackRange`.
        if let Some(shield) = self.skill(skill_ref).shield_target()
            && Some(target) == self.skill(skill_ref).lock_target
        {
            return view.alive
                && self
                    .shield_attack_point(shield, skill_ref, target)
                    .is_some_and(|(x_q32, z_q32)| attacker.reaches(x_q32, z_q32, 0));
        }
        view.alive
            && view.targetable
            && self.reaches_hidden(skill_ref.owner, view.visible)
            && attacker.reaches(view.x_q32, view.z_q32, view.radius)
    }

    /// The visibility half of `SkillAttackRangeChecker.IsActorInAttackRange`:
    /// a target that is not visible is out of range, except to a unit whose
    /// own `MechData` moves underground, which reaches one hidden the way it
    /// is (and not one in stealth, which no unit's skill makes).
    pub(in crate::fight) fn reaches_hidden(
        &self,
        owner: FightActorRef,
        target_visible: bool,
    ) -> bool {
        target_visible
            || matches!(owner, FightActorRef::Unit(id)
                if self.actors[&id].rules.underground.is_some())
    }

    /// Whether a target is within the owner's attack angle:
    /// `SkillAttackAngleChecker`.
    pub(in crate::fight) fn target_in_attack_angle(
        &self,
        skill_ref: SkillRef,
        target: FightActorRef,
    ) -> bool {
        let (Some(attacker), Some(view)) =
            (self.skill_attacker(skill_ref), self.fight_actor(target))
        else {
            return false;
        };
        view.alive && attacker.faces(attacker.bearing_q32(view.x_q32, view.z_q32))
    }

    /// Both: `FightSkill.IsAttackTargetInAttackArea`.
    pub(in crate::fight) fn target_in_attack_area(
        &self,
        skill_ref: SkillRef,
        target: FightActorRef,
    ) -> bool {
        self.target_in_attack_range(skill_ref, target)
            && self.target_in_attack_angle(skill_ref, target)
    }

    /// `SkillManager.UpdateWeaponRotateion`: every weapon turns towards a
    /// bearing at the owner's rotate speed.
    pub(in crate::fight) fn turn_weapons_towards(&mut self, skill_ref: SkillRef, bearing_q32: i64) {
        let turn_q32 = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable")
            .turn_q32;
        self.skill_mut(skill_ref)
            .turn_weapons_towards(bearing_q32, turn_q32);
    }

    /// Draws an attack interval for an owner's skill from its side's stream:
    /// the description plus a stagger, never under one tick. A skill without a
    /// stagger leaves the stream to the next owner.
    pub(in crate::fight) fn draw_attack_interval(&mut self, skill_ref: SkillRef) -> Result<u64> {
        let attacker = self
            .skill_attacker(skill_ref)
            .ok_or_else(|| Error::new("attack interval owner is absent"))?;
        let team = attacker.team;
        let interval_steps = seconds_q32_to_steps(attacker.attack_interval_q32);
        let offset_steps = native_time_units_to_steps(attacker.attack.interval_offset_time_units());
        let random = self
            .team_random
            .get_mut(&team)
            .ok_or_else(|| Error::new("attack interval team random stream is absent"))?;
        Ok(draw_interval(random, interval_steps, offset_steps))
    }
}

impl Simulation {
    /// `FightTeam.random`: every side's stream starts with the fight, from
    /// the round and the side, whether or not anything of the side draws
    /// from it before the fight has begun.
    pub(in crate::fight) fn start_side_streams(&mut self, round: u32) {
        for team in [0_u32, 1] {
            self.team_random.entry(team).or_insert_with(|| {
                GrRandom::new(u64::from(
                    round
                        .cast_signed()
                        .wrapping_add(team.cast_signed())
                        .wrapping_mul(4_444)
                        .cast_unsigned(),
                ))
            });
        }
    }

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
    pub(in crate::fight) fn deploy_attack_intervals(&mut self) -> Result<()> {
        let owners = self
            .units_in_update_order()
            .into_iter()
            .map(FightActorRef::Unit)
            .chain(
                self.constructions
                    .keys()
                    .map(|&id| FightActorRef::Building(id)),
            )
            .collect::<Vec<_>>();
        for owner in owners {
            self.draw_owner_first_intervals(owner)?;
        }
        Ok(())
    }

    /// Every skill of one owner draws its first interval in its
    /// `SkillManager`'s order, ascending skill ID: the extra skills an extra
    /// weapon technology adds draw before or after the main one.
    pub(in crate::fight) fn draw_owner_first_intervals(
        &mut self,
        owner: FightActorRef,
    ) -> Result<()> {
        let (before, after) = match owner {
            FightActorRef::Unit(id) => self.extra_skills_around_main(id),
            FightActorRef::Building(_) => (Vec::new(), Vec::new()),
        };
        let extra = |index| SkillRef {
            owner,
            slot: SkillSlot::Extra(index),
        };
        for index in before {
            self.draw_first_intervals(extra(index))?;
        }
        self.draw_first_intervals(SkillRef::main(owner))?;
        for index in after {
            self.draw_first_intervals(extra(index))?;
        }
        Ok(())
    }

    /// One owner's first intervals: one draw per skill it runs, the first
    /// kept as its current interval.
    pub(in crate::fight) fn draw_first_intervals(&mut self, skill_ref: SkillRef) -> Result<()> {
        let attacker = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable");
        // An extra skill is one `FightSkill`, whatever its row's weapons.
        let skills = if attacker.attack.weapons.mode == WeaponMode::Normal
            || skill_ref.slot != SkillSlot::Main
        {
            1
        } else {
            attacker.attack.weapons.count()
        };
        for index in 0..skills {
            let interval = self.draw_attack_interval(skill_ref)?;
            if index == 0 {
                self.skill_mut(skill_ref).current_attack_interval = interval;
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
