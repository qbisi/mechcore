//! `AdvancedEnergyShieldSystem`: battlefield shields.
//!
//! A shield contraption is a `FightEnergyShield` of its side, a sphere standing
//! on the ground, active from the start and holding its row's energy. It is
//! no actor: nothing moves around it or targets it. A hit meant for a unit of
//! its side that it covers lands on it instead, for as much energy as it has
//! left, and one that empties it destroys it for the rest of the fight: the
//! hit that does is absorbed whole. `docs/rules/contraptions.md` states the
//! rule.
//!
//! A shield a unit carries, a Barrier's, is one too, with that unit as its
//! owner: it stands where its owner stands, moving as it moves and staying
//! where it last stood once it dies, and one that a hit empties is
//! deactivated rather than destroyed (`PerformHitAdvancedEndergyShieldEffect`
//! branches on the owner), staying on the board with no energy.

use super::*;
use super::{
    math::{
        fpcs_acos_fastest, fpcs_asin_fastest, fpcs_cos_fastest, fpcs_sin_fastest, fpcs_sqrt_fastest,
    },
    rvo,
};
use crate::layout::{ShieldKind, ShieldPlacement};
use mechcore_mcfr::{ShieldDestroyedReason, ShieldRoundPolicy, ShieldSourceKind, ShieldState};

/// One `FightEnergyShield` still standing.
#[derive(Debug, Clone)]
pub(in crate::fight) struct EnergyShield {
    pub(in crate::fight) id: u64,
    pub(in crate::fight) team: u32,
    pub(in crate::fight) x_q32: i64,
    pub(in crate::fight) z_q32: i64,
    pub(in crate::fight) radius_q32: i64,
    pub(in crate::fight) energy: i64,
    max_energy: i64,
    source_kind: ShieldSourceKind,
    /// The unit carrying it, `FightEnergyShield.GetOwner`: none for one that
    /// stands where it was placed.
    owner: Option<u64>,
    /// `FightEnergyShield.IsActive`: false once a hit empties a shield with
    /// an owner, or its owner's technologies are switched off.
    pub(in crate::fight) active: bool,
    /// `EnergyShieldBehaviour.isEnable` of its owner's
    /// `AdvancedEnergyShieldController`: false while the technologies that
    /// gave it are switched off.
    enabled: bool,
    /// `AdvancedEnergyShieldController.energyRecord`: the energy it had as
    /// it was switched off, which it gets back.
    energy_record: i64,
}

/// A shield a unit carries into the fight: its side, where it starts, how
/// large and how full, and its owner.
#[derive(Debug, Clone, Copy)]
pub(in crate::fight) struct CarriedShieldPlacement {
    pub(in crate::fight) team: u32,
    pub(in crate::fight) x_q32: i64,
    pub(in crate::fight) z_q32: i64,
    pub(in crate::fight) radius_q32: i64,
    pub(in crate::fight) energy: i64,
    pub(in crate::fight) owner: u64,
    /// Whether its owner opens the fight travelling in from a flank.
    pub(in crate::fight) travelling: bool,
}

/// `AdvancedEnergyShieldSystem`: every battlefield shield standing, and what
/// the tick did to them, which the recording reads at the tick's end.
pub(in crate::fight) struct ShieldSystem {
    /// Every battlefield shield still standing, each side's in its active
    /// order.
    pub(in crate::fight) standing: Vec<EnergyShield>,
    /// The `shield_destroyed` events of the tick, which the recording reads
    /// off the shields standing at its end, after everything else.
    pub(in crate::fight) destroyed: Vec<Event>,
    /// The `shield_created` events of the tick, which the recording reads
    /// off the shields standing at its end, before the destroyed.
    pub(in crate::fight) created: Vec<Event>,
    /// The identity the next shield made in the fight takes.
    pub(in crate::fight) next_id: u64,
    /// The shields broken in the fight, where they stood: a skill firing at
    /// one still aims at it until its next check.
    pub(in crate::fight) broken: Vec<EnergyShield>,
}

impl ShieldSystem {
    /// The shields a layout stands, before the first tick.
    pub(in crate::fight) fn new(
        placed: &[ShieldPlacement],
        carried: &[CarriedShieldPlacement],
    ) -> Self {
        Self {
            standing: initialize_shields(placed, carried),
            destroyed: Vec::new(),
            created: Vec::new(),
            next_id: u64::try_from(placed.len() + carried.len()).expect("shield count fits u64")
                + 1,
            broken: Vec::new(),
        }
    }
}

/// Every shield both sides release, each side's in its active order:
/// `GroupAdvancedEnergyShieldManager.OnFightStart` sorts a side's shields by
/// `CompareEnergyShield`, their centre's x, then its z, then their energy and
/// radius. They take their identities in that order, side by side. A shield
/// a unit carries is one of its side's, where its owner stands.
fn initialize_shields(
    placed: &[ShieldPlacement],
    carried: &[CarriedShieldPlacement],
) -> Vec<EnergyShield> {
    let mut shields = placed
        .iter()
        .map(|shield| EnergyShield {
            id: 0,
            team: shield.team,
            x_q32: space_to_q32(shield.x),
            z_q32: space_to_q32(shield.z),
            radius_q32: shield.radius_q32,
            energy: shield.energy,
            max_energy: shield.energy,
            source_kind: match shield.kind {
                ShieldKind::Contraption => ShieldSourceKind::Contraption,
                ShieldKind::CommanderSkill => ShieldSourceKind::CommanderSkill,
            },
            owner: None,
            active: true,
            enabled: true,
            energy_record: 0,
        })
        .chain(carried.iter().map(|shield| EnergyShield {
            id: 0,
            team: shield.team,
            x_q32: shield.x_q32,
            z_q32: shield.z_q32,
            radius_q32: shield.radius_q32,
            // A unit placed on a flank has its effects deactivated as it is
            // placed (`AdvancedEnergyShieldProvider.DoDeactive`,
            // `AdvancedEnergyShieldSystem.DeactiveEnergyShield`): its shield
            // holds nothing and shields nothing until it arrives.
            energy: if shield.travelling { 0 } else { shield.energy },
            max_energy: shield.energy,
            source_kind: ShieldSourceKind::OwnerAdvanced,
            owner: Some(shield.owner),
            active: !shield.travelling,
            enabled: true,
            energy_record: 0,
        }))
        .collect::<Vec<_>>();
    shields.sort_by_key(|shield| {
        (
            shield.team,
            shield.x_q32,
            shield.z_q32,
            shield.energy,
            shield.radius_q32,
        )
    });
    for (index, shield) in shields.iter_mut().enumerate() {
        shield.id = u64::try_from(index + 1).expect("shield count fits u64");
    }
    shields
}

impl EnergyShield {
    pub(in crate::fight) const fn object_ref(&self) -> ObjectRef {
        ObjectRef::new(ObjectKind::Shield, self.id)
    }

    /// `FightCalculator.IsInEnergyShield`: a point strictly inside the sphere
    /// of a shield that has energy left.
    pub(in crate::fight) fn contains(&self, x_q32: i64, y_q32: i64, z_q32: i64) -> bool {
        self.active
            && self.energy > 0
            && native_q32_magnitude_3d(
                x_q32.saturating_sub(self.x_q32),
                y_q32,
                z_q32.saturating_sub(self.z_q32),
            ) < self.radius_q32
    }

    /// `FightCalculator.IsInEnergyShield(position, shield, range)`: a circle
    /// of `range` about a point lies within the sphere of a shield that has
    /// energy left, the point's distance from its centre less than its
    /// radius less the range (`FPoint.op_LessThan`).
    pub(in crate::fight) fn holds_circle(
        &self,
        (x_q32, y_q32, z_q32): (i64, i64, i64),
        range_q32: i64,
    ) -> bool {
        self.active
            && self.energy > 0
            && super::rvo::fpoint_less_than(
                native_q32_magnitude_3d(
                    x_q32.saturating_sub(self.x_q32),
                    y_q32,
                    z_q32.saturating_sub(self.z_q32),
                ),
                self.radius_q32.saturating_sub(range_q32),
            )
    }
}

impl Simulation {
    /// The shields a unit carries stand where it stands: each is moved with
    /// it, as its owner's position is set.
    pub(in crate::fight) fn follow_owner(&mut self, actor_id: u64) {
        let Some(actor) = self.actors.get(&actor_id) else {
            return;
        };
        let (x_q32, z_q32) = (actor.x_q32, actor.z_q32);
        for shield in &mut self.shield.standing {
            if shield.owner == Some(actor_id) {
                shield.x_q32 = x_q32;
                shield.z_q32 = z_q32;
            }
        }
    }

    /// `EnergyShieldProvider.DisableEffect` and `EnableEffect`
    /// (`EnergyShieldController.Disable`, `Enable`): off, the unit's own
    /// shield is disabled and records its energy over its maximum, an
    /// `FPoint` quotient, rounded; on, it is enabled with that share of its
    /// maximum, the product truncated. Its energy stays as it was while it
    /// is off.
    pub(in crate::fight) fn switch_energy_shield(&mut self, actor_id: u64, on: bool) {
        let actor = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable");
        let Some(shield) = actor.shield.as_mut() else {
            return;
        };
        shield.enabled = on;
        if shield.maximum <= 0 {
            return;
        }
        let maximum = i128::from(shield.maximum);
        if on {
            shield.energy = i64::try_from((maximum * i128::from(shield.record_rate_q32)) >> 32)
                .expect("a share of the maximum fits");
        } else {
            shield.record_rate_q32 =
                i64::try_from(((i128::from(shield.energy) << 32) + maximum / 2) / maximum)
                    .expect("a share of the maximum fits");
        }
    }

    /// `AdvancedEnergyShieldProvider.DisableEffect` and `EnableEffect` of a
    /// unit's technologies, on the shield it carries.
    ///
    /// Switched off, a shield that is available (`IsAvaliable`: active,
    /// enabled and holding energy) is disabled with its energy recorded
    /// (`AdvancedEnergyShieldController.Disable`) and deactivated
    /// (`AdvancedEnergyShieldSystem.DeactiveEnergyShield`): it has no energy
    /// and leaves its side's active shields. One that is not available is
    /// left as it is.
    ///
    /// Switched on, a shield that was disabled gets its recorded energy back
    /// (`AdvancedEnergyShieldController.Enable`) and, while its owner lives,
    /// is activated without being refilled
    /// (`AdvancedEnergyShieldSystem.ActiveEnergyShield` with no reset),
    /// joining its side's active shields after every other.
    pub(in crate::fight) fn switch_carried_shield(&mut self, actor_id: u64, on: bool) {
        let alive = self.actors.get(&actor_id).is_some_and(Actor::alive);
        let Some(index) = self
            .shield
            .standing
            .iter()
            .position(|shield| shield.owner == Some(actor_id))
        else {
            return;
        };
        let shield = &mut self.shield.standing[index];
        if !on {
            if !(shield.active && shield.enabled && shield.energy > 0) {
                return;
            }
            shield.enabled = false;
            shield.energy_record = shield.energy;
            shield.active = false;
            shield.energy = 0;
            return;
        }
        if shield.enabled {
            return;
        }
        shield.enabled = true;
        shield.energy = shield.energy_record;
        if !alive {
            return;
        }
        shield.active = true;
        let shield = self.shield.standing.remove(index);
        let after = self
            .shield
            .standing
            .iter()
            .rposition(|standing| standing.team <= shield.team)
            .map_or(0, |index| index + 1);
        self.shield.standing.insert(after, shield);
    }

    /// `AdvancedEnergyShieldProvider.DoActive` as a unit's effects are
    /// activated: `AdvancedEnergyShieldSystem.ActiveEnergyShield` with a
    /// reset, which refills the shield it carries (`FightEnergyShield.Active`)
    /// and adds it to its side's active shields after every other. A unit
    /// arriving from a flank brings up the shield its placement deactivated.
    /// A shield already active is left as it is, and so is a technology's
    /// while a buff holds its owner's technologies off: `ActiveCheck` finds
    /// the source locked (`IsLockedEffect`), and `EnableEffect` later acts
    /// only on a shield its disabling found available, so it never comes up.
    pub(in crate::fight) fn activate_carried_shield(&mut self, actor_id: u64) {
        let locked = self.actors.get(&actor_id).is_some_and(|actor| {
            actor.technology_disabled()
                && actor
                    .placement
                    .effects
                    .carried_shield
                    .is_some_and(|carried| carried.priority == 0)
        });
        if locked {
            return;
        }
        let Some(index) = self
            .shield
            .standing
            .iter()
            .position(|shield| shield.owner == Some(actor_id) && !shield.active)
        else {
            return;
        };
        let mut shield = self.shield.standing.remove(index);
        shield.active = true;
        shield.energy = shield.max_energy;
        let after = self
            .shield
            .standing
            .iter()
            .rposition(|standing| standing.team <= shield.team)
            .map_or(0, |index| index + 1);
        self.shield.standing.insert(after, shield);
    }

    /// Every standing shield as the snapshot holds it.
    pub(in crate::fight) fn shield_states(&self) -> Vec<ShieldState> {
        let mut order = BTreeMap::<u32, u32>::new();
        self.shield
            .standing
            .iter()
            .map(|shield| {
                let active_order = order.entry(shield.team).or_insert(0);
                let state = ShieldState {
                    shield_id: shield.id,
                    team_id: shield.team,
                    source_kind: shield.source_kind,
                    owner: shield
                        .owner
                        .map(|owner| ObjectRef::new(ObjectKind::Unit, owner)),
                    position: QVec3 {
                        x: shield.x_q32,
                        y: 0,
                        z: shield.z_q32,
                    },
                    radius: shield.radius_q32,
                    energy: GaugeI32 {
                        current: i32::try_from(shield.energy).expect("shield energy fits i32"),
                        maximum: i32::try_from(shield.max_energy).expect("shield energy fits i32"),
                    },
                    // `FightEnergyShield.IsShortLifeTime`: a spawned shield's
                    // round ends with it; any other is full again the next.
                    round_policy: if shield.source_kind == ShieldSourceKind::SpawnedTemporary {
                        ShieldRoundPolicy::DestroyAtRoundEnd
                    } else {
                        ShieldRoundPolicy::ResetToMax
                    },
                    active: shield.active,
                    active_order: shield.active.then_some(*active_order),
                };
                if shield.active {
                    *active_order += 1;
                }
                state
            })
            .collect()
    }

    /// A Shield Airdrop that lands, where it lands, in space units.
    pub(in crate::fight) fn drop_shield(
        &mut self,
        team: u32,
        x: i64,
        z: i64,
        radius_q32: i64,
        energy: i64,
    ) {
        self.create_shield(
            (team, ShieldSourceKind::CommanderSkill),
            space_to_q32(x),
            space_to_q32(z),
            radius_q32,
            energy,
        );
    }

    /// `AdvancedEnergyShieldSystem.Create` for a Shield Airdrop that lands,
    /// or a shield a unit's technology spawns: a shield of no owner of the
    /// side, active and full, after every shield its side already holds,
    /// taking the next identity.
    pub(in crate::fight) fn create_shield(
        &mut self,
        (team, source_kind): (u32, ShieldSourceKind),
        x_q32: i64,
        z_q32: i64,
        radius_q32: i64,
        energy: i64,
    ) {
        let id = self.shield.next_id;
        self.shield.next_id += 1;
        let shield = EnergyShield {
            id,
            team,
            x_q32,
            z_q32,
            radius_q32,
            energy,
            max_energy: energy,
            source_kind,
            owner: None,
            active: true,
            enabled: true,
            energy_record: 0,
        };
        self.shield.created.push(event(
            Some(shield.object_ref()),
            None,
            None,
            None,
            EventPayload::ShieldCreated {
                team_id: team,
                source_kind,
                position: QVec3 {
                    x: shield.x_q32,
                    y: 0,
                    z: shield.z_q32,
                },
            },
        ));
        let after = self
            .shield
            .standing
            .iter()
            .rposition(|standing| standing.team <= team)
            .map_or(0, |index| index + 1);
        self.shield.standing.insert(after, shield);
    }

    /// Every shield of the other side that holds the point, a projectile's
    /// `inEnergyShields` as it is made.
    pub(in crate::fight) fn enemy_shields_at(
        &self,
        team: u32,
        x_q32: i64,
        y_q32: i64,
        z_q32: i64,
    ) -> Vec<u64> {
        self.shield
            .standing
            .iter()
            .filter(|shield| shield.team != team && shield.contains(x_q32, y_q32, z_q32))
            .map(|shield| shield.id)
            .collect()
    }

    /// `FightCalculator.IsActorInEnergyShield`: the first of the actor's own
    /// side's shields that holds its centre. A tower is a `FightActor` as a
    /// unit is, and a shield over it covers it.
    pub(in crate::fight) fn shield_around(&self, target: FightActorRef) -> Option<u64> {
        let team = self.fight_actor(target)?.team;
        let (x_q32, y_q32, z_q32) = self.position_3d(target)?;
        self.shield
            .standing
            .iter()
            .find(|shield| shield.team == team && shield.contains(x_q32, y_q32, z_q32))
            .map(|shield| shield.id)
    }

    /// Whether a shield holds an actor's centre.
    /// Where a standing shield's centre stands, Q32.32 metres:
    /// `FightEnergyShield.GetFightTransform`'s position.
    pub(in crate::fight) fn shield_centre(&self, shield_id: u64) -> Option<(i64, i64)> {
        self.shield
            .standing
            .iter()
            .find(|shield| shield.id == shield_id)
            .map(|shield| (shield.x_q32, shield.z_q32))
    }

    pub(in crate::fight) fn shield_holds(&self, shield_id: u64, actor: FightActorRef) -> bool {
        let (Some(shield), Some((x_q32, y_q32, z_q32))) = (
            self.shield
                .standing
                .iter()
                .find(|shield| shield.id == shield_id),
            self.position_3d(actor),
        ) else {
            return false;
        };
        shield.contains(x_q32, y_q32, z_q32)
    }

    /// `DamagePerformer.PerformHitAdvancedEndergyShieldEffect`: a hit on a
    /// shield takes as much energy as it has, and one that empties it
    /// destroys it, `AdvancedEnergyShieldSystem.Destroy`, for good. What it
    /// took is returned.
    pub(in crate::fight) fn hit_shield(
        &mut self,
        shield_id: u64,
        hit: &DamageHit,
        events: &mut Vec<Event>,
    ) -> Result<i64> {
        let index = self
            .shield
            .standing
            .iter()
            .position(|shield| shield.id == shield_id)
            .ok_or_else(|| Error::new("a hit shield is absent"))?;
        let shield = &mut self.shield.standing[index];
        // `CalculateHitEnergyShieldDamage`: what the hit deals a shield.
        let taken = hit
            .shield_damage
            .unwrap_or(hit.amount)
            .min(shield.energy)
            .max(0);
        shield.energy -= taken;
        // A hit that takes nothing records nothing, as a unit's does: a
        // Disintegration wave strikes a shield every firing for none.
        if taken > 0 {
            events.push(event(
                hit.projectile,
                hit.source,
                Some(hit.source_team),
                Some(shield.object_ref()),
                EventPayload::Damage {
                    amount: i32::try_from(taken).map_err(|_| Error::new("damage exceeds i32"))?,
                    skill_slot: hit.skill_slot,
                },
            ));
        }
        if shield.energy <= 0 && shield.owner.is_some() {
            // `AdvancedEnergyShieldSystem.DeactiveEnergyShield`.
            shield.active = false;
        } else if shield.energy <= 0 {
            let shield = self.shield.standing.remove(index);
            self.shield.broken.push(shield.clone());
            self.shield.destroyed.push(event(
                Some(shield.object_ref()),
                None,
                None,
                None,
                EventPayload::ShieldDestroyed {
                    position: QVec3 {
                        x: shield.x_q32,
                        y: 0,
                        z: shield.z_q32,
                    },
                    reason: ShieldDestroyedReason::EnergyDepleted,
                },
            ));
        }
        Ok(taken)
    }

    /// The shield a projectile flies into: none for a shot that crosses
    /// shields, and otherwise the first enemy shield that holds it now and
    /// did not as it was made. A missile's shot crosses none.
    pub(in crate::fight) fn absorbing_shield(&self, projectile: &Projectile) -> Option<u64> {
        if let Shooter::Actor(owner) = projectile.shooter
            && self
                .attacker(owner)
                .is_some_and(|attacker| attacker.attack.crosses_shields)
        {
            return None;
        }
        self.shield
            .standing
            .iter()
            .find(|shield| {
                shield.team != projectile.team
                    && !projectile.spawn_shields.contains(&shield.id)
                    && shield.contains(projectile.x_q32, projectile.y_q32, projectile.z_q32)
            })
            .map(|shield| shield.id)
    }

    /// Where a point of the fight stands, `FPoint` raw metres: a unit's
    /// centre at its height, a building's on the ground.
    pub(in crate::fight) fn position_3d(&self, object: FightActorRef) -> Option<(i64, i64, i64)> {
        let view = self.fight_actor(object)?;
        Some((view.x_q32, self.target_height_q32(object), view.z_q32))
    }

    /// `FightSkill.GetTargetEnergyShield`: the shield of the target's side
    /// that covers it and whose surface, seen from the owner, is within the
    /// skill's reach, `range` with its `extraShieldAttackRange` added
    /// (`IsInAttackRange` with `useExtraRange`); a nearer one that covers it
    /// too takes its place.
    pub(in crate::fight) fn target_energy_shield(
        &self,
        owner: FightActorRef,
        target: FightActorRef,
        range: i64,
    ) -> Option<u64> {
        // Any `FightActor`: a tower a shield covers is fired at through it as
        // a unit is.
        let target_actor = self.fight_actor(target)?;
        let attacker = self.attacker(owner)?;
        let (owner_x, owner_y, owner_z) = self.position_3d(owner)?;
        let (target_x, target_y, target_z) = self.position_3d(target)?;
        let mut best: Option<(u64, i64)> = None;
        for shield in self
            .shield
            .standing
            .iter()
            .filter(|shield| shield.team == target_actor.team)
        {
            if !shield.contains(target_x, target_y, target_z) {
                continue;
            }
            let distance = native_q32_magnitude_3d(
                shield.x_q32.saturating_sub(owner_x),
                owner_y,
                shield.z_q32.saturating_sub(owner_z),
            );
            match best {
                None => {
                    let gap = distance
                        .saturating_sub(space_to_q32(attacker.radius))
                        .saturating_sub(shield.radius_q32)
                        .saturating_sub(space_to_q32(target_actor.radius))
                        .max(0);
                    if gap >= space_to_q32(attacker.min_range) && gap <= space_to_q32(range) {
                        best = Some((shield.id, distance));
                    }
                }
                Some((_, nearest)) if distance < nearest => best = Some((shield.id, distance)),
                Some(_) => {}
            }
        }
        best.map(|(shield, _)| shield)
    }

    /// `SkillSearchTargetController.SearchTargetShield`: the shield a skill
    /// fires at in place of its lock, unless its hits cross shields or its
    /// owner stands inside that shield.
    pub(in crate::fight) fn search_target_shield(
        &self,
        owner: FightActorRef,
        target: FightActorRef,
    ) -> Option<u64> {
        let range = self.attacker(owner)?.shield_range();
        self.search_target_shield_in(owner, target, range)
    }

    /// The same for a skill of the given range: a grouped unit's slot reaches
    /// further than its core, and asks with its own.
    pub(in crate::fight) fn search_target_shield_in(
        &self,
        owner: FightActorRef,
        target: FightActorRef,
        range: i64,
    ) -> Option<u64> {
        if self
            .attacker(owner)
            .is_none_or(|attacker| attacker.attack.crosses_shields)
        {
            return None;
        }
        let shield = self.target_energy_shield(owner, target, range)?;
        (!self.shield_holds(shield, owner)).then_some(shield)
    }

    /// `GetAttackPositionOnEnergyShield`: the point of a shield's surface on
    /// the way from what it covers towards the owner, half a metre out.
    pub(in crate::fight) fn shield_attack_point(
        &self,
        shield_id: u64,
        skill_ref: SkillRef,
        target: FightActorRef,
    ) -> Option<(i64, i64)> {
        // A sweep is measured to the shield's centre or to its lock,
        // whichever is nearer in space
        // (`SkillAttackRangeChecker.IsAttackTargetInAttackRange`); to the
        // lock once the shield is gone, which a sweep under way, its checks
        // held, still names.
        if self.skill(skill_ref).kind == SkillKind::Sweep {
            let inside = self.position_3d(target)?;
            let outside = self.position_3d(skill_ref.owner)?;
            let center = self
                .shield
                .standing
                .iter()
                .chain(&self.shield.broken)
                .find(|shield| shield.id == shield_id)
                .map(|shield| (shield.x_q32, 0, shield.z_q32));
            let (x, _, z) = match center {
                Some(center)
                    if magnitude(sub(center, outside)) <= magnitude(sub(inside, outside)) =>
                {
                    center
                }
                _ => inside,
            };
            return Some((x, z));
        }
        // A broken shield is still the skill's target until its next check
        // (`FightSkill.SearchAttackTarget`): the Vortex whose blow breaks one
        // reads attacking on its lock that tick, and loses it the next, and
        // a Wasp, which checks once a blow, goes on attacking it, its lock
        // out of reach, until its next blow's check.
        if !self
            .shield
            .standing
            .iter()
            .chain(&self.shield.broken)
            .any(|shield| shield.id == shield_id)
        {
            return None;
        }
        let inside = self.position_3d(target)?;
        let outside = self.position_3d(skill_ref.owner)?;
        let (x, _, z) = self.shield_entry_point(shield_id, inside, outside);
        Some((x, z))
    }

    /// `CommanderSkillSubEffectAgent.IsHitEnergyShield`: the shield a falling
    /// sub-effect has come inside, of either side, the one whose surface its
    /// last point was nearest, and where it met that surface.
    pub(in crate::fight) fn falling_into_shield(
        &self,
        now: (i64, i64, i64),
        last: (i64, i64, i64),
    ) -> Option<(u64, (i64, i64, i64))> {
        let shield = self
            .shield
            .standing
            .iter()
            .filter(|shield| shield.contains(now.0, now.1, now.2))
            .min_by_key(|shield| {
                magnitude(sub(last, (shield.x_q32, 0, shield.z_q32)))
                    .saturating_sub(shield.radius_q32)
            })?;
        Some((
            shield.id,
            point_on_circle(
                (shield.x_q32, 0, shield.z_q32),
                shield.radius_q32,
                now,
                last,
                HALF_METRE,
            ),
        ))
    }

    /// `FightUtility.GetAttackPositionOnEnergyShieldOuter`: where the way from
    /// a point inside a shield towards one outside it leaves the shield,
    /// half a metre beyond its surface.
    pub(in crate::fight) fn shield_entry_point(
        &self,
        shield_id: u64,
        inside: (i64, i64, i64),
        outside: (i64, i64, i64),
    ) -> (i64, i64, i64) {
        let Some(shield) = self
            .shield
            .standing
            .iter()
            .chain(&self.shield.broken)
            .find(|shield| shield.id == shield_id)
        else {
            return inside;
        };
        point_on_circle(
            (shield.x_q32, 0, shield.z_q32),
            shield.radius_q32,
            inside,
            outside,
            HALF_METRE,
        )
    }
}

/// `FPoint.C0_5`, the offset every caller passes.
const HALF_METRE: i64 = 0x8000_0000;
/// `FightUtility.Angle180`, in degrees.
const ANGLE_180: i64 = 180 << 32;
/// `FPoint.Deg2Rad` and `FPoint.Rad2Deg`, as the build holds them.
const DEG_TO_RAD: i64 = 0x0477_D1A9;
const RAD_TO_DEG: i64 = 0x39_4BB8_34C8;
/// `FVector3.Normalize` leaves a vector shorter than `FPoint.C1em5` zero,
/// and `AngleRad` answers zero under `FPoint.C1em9`.
const NORMALIZE_EPSILON: i64 = 0xA7C5;
const ANGLE_EPSILON: i64 = 4;

pub(in crate::fight) type Vector = (i64, i64, i64);

const fn sub(left: Vector, right: Vector) -> Vector {
    (
        left.0.wrapping_sub(right.0),
        left.1.wrapping_sub(right.1),
        left.2.wrapping_sub(right.2),
    )
}

const fn add(left: Vector, right: Vector) -> Vector {
    (
        left.0.wrapping_add(right.0),
        left.1.wrapping_add(right.1),
        left.2.wrapping_add(right.2),
    )
}

pub(in crate::fight) fn scale(vector: Vector, factor: i64) -> Vector {
    (
        rvo::q32_mul(vector.0, factor),
        rvo::q32_mul(vector.1, factor),
        rvo::q32_mul(vector.2, factor),
    )
}

/// `FVector3.SqrMagnitude`: `(y² + x²) + z²`.
fn square_magnitude(vector: Vector) -> i64 {
    rvo::q32_mul(vector.1, vector.1)
        .wrapping_add(rvo::q32_mul(vector.0, vector.0))
        .wrapping_add(rvo::q32_mul(vector.2, vector.2))
}

fn magnitude(vector: Vector) -> i64 {
    native_q32_magnitude_3d(vector.0, vector.1, vector.2)
}

/// `FVector3.get_normalized`.
pub(in crate::fight) fn normalized(vector: Vector) -> Vector {
    let length = magnitude(vector);
    if length < NORMALIZE_EPSILON {
        return (0, 0, 0);
    }
    scale(vector, rvo::q32_div(Q32_ONE, length))
}

/// `FVector3.Angle`, in degrees: `AngleRad` through `FPCSMath.AcosFastest`.
fn angle(from: Vector, to: Vector) -> i64 {
    let (first, second) = (square_magnitude(from), square_magnitude(to));
    let denominator = if first.wrapping_add(second) < 0x1_6A09_0000_0001 {
        fpcs_sqrt_fastest(rvo::q32_mul(first, second))
    } else {
        rvo::q32_mul(fpcs_sqrt_fastest(first), fpcs_sqrt_fastest(second))
    };
    if denominator <= ANGLE_EPSILON {
        return 0;
    }
    let dot = rvo::q32_mul(from.1, to.1)
        .wrapping_add(rvo::q32_mul(from.0, to.0))
        .wrapping_add(rvo::q32_mul(from.2, to.2));
    let cosine = rvo::q32_div(dot, denominator).clamp(-Q32_ONE, Q32_ONE);
    rvo::q32_mul(fpcs_acos_fastest(cosine), RAD_TO_DEG)
}

/// `FVector3.ClampMagnitude`; a `NaN` length leaves the vector as it is.
fn clamp_magnitude(vector: Vector, length: Option<i64>) -> Vector {
    let Some(length) = length else {
        return vector;
    };
    let square = square_magnitude(vector);
    if !rvo::fpoint_greater_than(square, rvo::q32_mul(length, length)) {
        return vector;
    }
    let inverse = rvo::q32_div(Q32_ONE, fpcs_sqrt_fastest(square));
    scale(scale(vector, inverse), length)
}

/// `FightUtility.CalculatePointOnCricle`: from `inside`, towards `outside`,
/// to where the way leaves the sphere, `offset` beyond it. A step too short
/// to reach the surface is taken a hundred times over and no further.
fn point_on_circle(
    center: Vector,
    radius: i64,
    inside: Vector,
    outside: Vector,
    offset: i64,
) -> Vector {
    let to_center = sub(center, inside);
    let way = sub(outside, inside);
    let distance = magnitude(to_center);
    if rvo::fpoint_less_or_equal(distance, 0) {
        return add(center, scale(normalized(way), offset.wrapping_add(radius)));
    }
    let theta = angle(to_center, way);
    if rvo::fpoint_less_than(theta, ANGLE_180) {
        let sine = fpcs_sin_fastest(rvo::q32_mul(theta, DEG_TO_RAD));
        let across = rvo::q32_mul(sine, distance);
        let length = fpcs_asin_fastest(rvo::q32_div(across, radius)).map(|chord| {
            let chord = rvo::q32_mul(rvo::q32_mul(chord, RAD_TO_DEG), DEG_TO_RAD);
            let along = rvo::q32_mul(fpcs_cos_fastest(rvo::q32_mul(theta, DEG_TO_RAD)), distance);
            along
                .wrapping_add(rvo::q32_mul(radius, fpcs_cos_fastest(chord)))
                .wrapping_add(offset)
        });
        return add(inside, clamp_magnitude(scale(way, 100 << 32), length));
    }
    add(
        center,
        clamp_magnitude(
            scale(sub(outside, center), 100 << 32),
            Some(offset.wrapping_add(radius)),
        ),
    )
}
