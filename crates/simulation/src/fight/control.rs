//! The Hacker's control beam: `ControllEffect` and `TeamTranslationSystem`.
//!
//! A control beam's skill attacks through `NormalAttackPerformer` as a blow
//! does, and the performer takes its effect from
//! `FightControllBeamSkill.GetAttackEffect` when the attack state is entered
//! (`Enter`) and again when the attack target changes: `ControllEffect` for a
//! mech the beam may turn, `DamageEffect` for anything else. The control
//! effect's `Start` adds the skill to its target's entry of
//! `TeamTranslationSystem.translatingDatas`, its `Stop`, on `Exit` or a change
//! of target, takes it away, and an entry with no skill left goes, its
//! progress with it.
//!
//! Each hit of a control effect adds the skill's damage to the entry of the
//! skill's lock (`TeamTranslationSystem.Translate`), and the damage of its
//! first hits is all but nothing (`ControlBeamDamageCalculator`). Before the
//! units update, `TeamTranslationSystem.Update` turns every unit whose entry
//! has reached its life to the side of its first skill's owner
//! (`ChangeTeam`), and every skill locked on it stops its attack
//! (`FightSkill.OnChangeTeam`). `docs/rules/control.md` says what the
//! recordings showed.

use super::damage::Stroke;
use super::skill::SkillState;
use super::*;

/// `FightControllBeamSkill.DAMAGE_MODIFIER`: what a beam's damage effect
/// deals of its damage, 0.3 in `FPoint`.
const DAMAGE_EFFECT_RATE_Q32: i64 = 0x4ccc_cccc;

/// `FightControllBeamSkill.GetDamage` for what the beam does not turn: its
/// damage times [`DAMAGE_EFFECT_RATE_Q32`], rounded down. A full hit of 600
/// deals 179, and a warmup hit of 1 deals nothing.
fn damage_effect(damage: i64) -> i64 {
    q32_mul(damage << 32, DAMAGE_EFFECT_RATE_Q32) >> 32
}

/// One entry of `TeamTranslationSystem.translatingDatas`.
#[derive(Debug, Clone)]
pub(in crate::fight) struct Translation {
    pub(in crate::fight) target: u64,
    /// `TranslationData.progress`: the power its beams have added.
    pub(in crate::fight) progress: i32,
    /// The skills whose beams hold it, in the order they started.
    pub(in crate::fight) sources: Vec<BeamSource>,
}

/// One skill of `TranslationData`'s list: a unit's skill, and the place in
/// its group of the one whose beam holds the entry. A unit whose several
/// beams hold one unit is listed once for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct BeamSource {
    pub(in crate::fight) owner: u64,
    pub(in crate::fight) slot: SkillSlot,
    pub(in crate::fight) offset: usize,
}

/// What a control beam's `NormalAttackPerformer` holds while its skill
/// attacks: the attack target its effect was taken for, and whether that
/// effect is `ControllEffect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::fight) struct Beam {
    pub(in crate::fight) target: FightActorRef,
    pub(in crate::fight) control: bool,
}

impl Simulation {
    /// `TeamTranslationSystem.IsIgnoredMech`: a unit the beam may not turn,
    /// which wears an item whose `IgnoreControllerBeam` is true.
    fn ignores_control(&self, unit_id: u64) -> bool {
        self.actors
            .get(&unit_id)
            .is_some_and(|actor| actor.placement.effects.ignores_control_beam)
    }

    /// `NormalAttackPerformer.Enter`, `Exit` and the change of attack target
    /// between them, for every skill of a unit that fires a control beam, in
    /// the order its skills update: the effect each holds follows its attack
    /// state and its target.
    pub(in crate::fight) fn sync_beam(&mut self, actor_id: u64) {
        for source in self.beam_skills(actor_id) {
            self.sync_skill_beam(source);
        }
    }

    /// A unit's skills that fire a control beam, each skill of a group its
    /// own, in the order `SkillManager.Update` runs them: by skill ID, the
    /// skills of a group after their core.
    fn beam_skills(&self, actor_id: u64) -> Vec<BeamSource> {
        let actor = &self.actors[&actor_id];
        let (before, after) = self.extra_skills_around_main(actor_id);
        before
            .into_iter()
            .map(|index| (SkillSlot::Extra(index), &actor.skills.extras[index].skill))
            .chain(std::iter::once((SkillSlot::Main, &actor.skills.main)))
            .chain(
                after
                    .into_iter()
                    .map(|index| (SkillSlot::Extra(index), &actor.skills.extras[index].skill)),
            )
            .filter(|(_, skill)| skill.kind == SkillKind::ControlBeam)
            .flat_map(|(slot, skill)| {
                (0..skill.group_size().max(1)).map(move |offset| BeamSource {
                    owner: actor_id,
                    slot,
                    offset,
                })
            })
            .collect()
    }

    fn sync_skill_beam(&mut self, source: BeamSource) {
        let skill_ref = SkillRef {
            owner: FightActorRef::Unit(source.owner),
            slot: source.slot,
        };
        let alive = self.actors[&source.owner].alive();
        let skill = self.skill(skill_ref).group_skill(source.offset);
        // `FightControllBeamSkill.GetAttackEffect`: the control effect for a
        // unit that may be turned, the skill's damage effect for anything
        // else, a shield the skill fires at in place of its lock among it.
        let wanted = (alive && matches!(skill.state, SkillState::Attack(_)))
            .then(|| skill.attack_target())
            .flatten()
            .map(|target| {
                let control = match target {
                    FightActorRef::Unit(id) => {
                        !self.ignores_control(id) && skill.shield_target().is_none()
                    }
                    FightActorRef::Building(_) => false,
                };
                Beam { target, control }
            });
        if skill.beam == wanted {
            return;
        }
        self.stop_beam(source);
        let Some(Beam { target, control }) = wanted else {
            return;
        };
        if let (true, FightActorRef::Unit(id)) = (control, target) {
            // `ControllEffect.Start` and `TeamTranslationSystem.Add`.
            match self
                .translations
                .iter_mut()
                .find(|entry| entry.target == id)
            {
                Some(entry) => entry.sources.push(source),
                None => self.translations.push(Translation {
                    target: id,
                    progress: 0,
                    sources: vec![source],
                }),
            }
        }
        self.skill_mut(skill_ref)
            .group_skill_mut(source.offset)
            .beam = Some(Beam { target, control });
    }

    /// `ControllEffect.Stop` and `TeamTranslationSystem.Remove`.
    fn stop_beam(&mut self, source: BeamSource) {
        let skill_ref = SkillRef {
            owner: FightActorRef::Unit(source.owner),
            slot: source.slot,
        };
        let Some(beam) = self
            .skill_mut(skill_ref)
            .group_skill_mut(source.offset)
            .beam
            .take()
        else {
            return;
        };
        if let (true, FightActorRef::Unit(id)) = (beam.control, beam.target) {
            if let Some(entry) = self
                .translations
                .iter_mut()
                .find(|entry| entry.target == id)
            {
                entry.sources.retain(|&held| held != source);
            }
            self.translations.retain(|entry| !entry.sources.is_empty());
        }
    }

    /// `ControlBeamDamageCalculator.GetAttackDamage`: the skill's damage,
    /// and for the hits up to `warmup_attack_count`, counted from zero, that
    /// damage times the warm-up multiplier, at least one. The damage is the
    /// skill's own, which a technology's `DamageReduceRateBase` has lowered
    /// already: Multi Control's beams turn by 102 a hit of a Hacker's 600,
    /// and by 1 a warm-up hit.
    fn beam_damage(&self, skill_ref: SkillRef, attack_count: i32) -> i64 {
        let attacker = self
            .skill_attacker(skill_ref)
            .expect("skill owner identity is stable");
        let damage = attacker.attack_damage;
        let AttackPath::ControlBeam {
            warmup_attack_count,
            warmup_damage_multiplier,
            ..
        } = attacker.attack.path
        else {
            unreachable!("a control beam's damage needs a control beam's path")
        };
        if attack_count > i32::try_from(warmup_attack_count).unwrap_or(i32::MAX) {
            return damage;
        }
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "the multiplier is the build's float, applied as the build applies it"
        )]
        let warm = ((damage as f64) * warmup_damage_multiplier) as i64;
        warm.max(1)
    }

    /// What a recording reads as a beam's damage: its damage calculator
    /// with the default attack count, zero, which is the first warmup hit
    /// whatever the beam is on. The Hacker reads 1 against a unit it may
    /// turn, a unit wearing the Anti-Interference Module and a shield alike.
    pub(in crate::fight) fn beam_snapshot_damage(&self, skill_ref: SkillRef) -> Option<i32> {
        if self.skill(skill_ref).kind != SkillKind::ControlBeam {
            return None;
        }
        let damage = self.beam_damage(skill_ref, 0);
        Some(i32::try_from(damage).unwrap_or(i32::MAX))
    }

    /// `NormalAttackPerformer.TryPerformEffect` for a control beam, the
    /// skill at `offset` of its group: `ControllEffect.Perform` adds the
    /// hit's power to the entry of that skill's lock, and
    /// `DamageEffect.Perform` strikes with the damage the beam's damage
    /// effect deals.
    pub(in crate::fight) fn control_effect(
        &mut self,
        skill_ref: SkillRef,
        offset: usize,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        let actor_id = skill_ref
            .owner
            .unit_id()
            .ok_or_else(|| Error::new("a construction's control beam is not supported"))?;
        self.sync_beam(actor_id);
        let skill = self.skill(skill_ref).group_skill(offset);
        let (attack_count, lock, beam) = (skill.attack_count, skill.lock_target, skill.beam);
        let damage = self.beam_damage(skill_ref, attack_count);
        if !beam.is_some_and(|beam| beam.control) {
            // A hit that deals nothing strikes nothing: the warmup hits on a
            // unit wearing the Anti-Interference Module write no damage.
            let amount = damage_effect(damage);
            if amount < 1 {
                return Ok(());
            }
            // `FightControllBeamSkill.GetAttackEffect` answers its
            // `DamageEffect` for any skill of the class, an extra row's
            // among them, which strikes as its own `SkillDamageProvider`.
            return self.direct_effect_dealing((skill_ref, offset), target, amount, events);
        }
        // `ControllEffect.Perform` turns the skill's lock.
        let Some(FightActorRef::Unit(lock)) = lock else {
            return Ok(());
        };
        let power = i32::try_from(damage).unwrap_or(i32::MAX);
        if power < 1 {
            return Ok(());
        }
        if let Some(entry) = self
            .translations
            .iter_mut()
            .find(|entry| entry.target == lock)
        {
            entry.progress = entry.progress.saturating_add(power);
        }
        // `Translate` lists the beam's owner among the unit's attackers
        // (`ExpSystem.AddAttackData`), as a hit does.
        let owner = self.actors[&actor_id].object_ref();
        let team = self.actors[&actor_id].placement.team;
        self.count_experience(Some(owner), team, FightActorRef::Unit(lock), false)?;
        // Then the unit it turns raises `OnMechBeHit` with the beam's owner
        // as the attacker, as a hit's `FightMech.OnHitted` does: a Void Eye
        // with Electromagnetic Armor switches the Hacker's technologies off.
        self.on_mech_be_hit(lock, owner, events)
    }

    /// `TeamTranslationSystem.Update`: every unit whose progress has reached
    /// its life changes to the side of its first beam's owner. The keys are
    /// taken first and each is asked again as it comes: turning one unit
    /// stops the beams locked on it and its own, which may take another's
    /// entry away or change the side its owner stands on. Two Hackers that
    /// turn each other on one tick leave both on the side of the first one
    /// turned.
    pub(in crate::fight) fn update_translations(
        &mut self,
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        // `TeamTranslationSystem.OnMechDead`: a dead unit's entry goes.
        let actors = &self.actors;
        self.translations
            .retain(|entry| actors.get(&entry.target).is_some_and(Actor::alive));
        let keys = self
            .translations
            .iter()
            .map(|entry| entry.target)
            .collect::<Vec<_>>();
        let mut turned = Vec::new();
        for target in keys {
            let Some(entry) = self
                .translations
                .iter()
                .find(|entry| entry.target == target)
            else {
                continue;
            };
            let due = self
                .actors
                .get(&target)
                .is_some_and(|actor| actor.alive() && actor.life <= i64::from(entry.progress));
            let turner = entry
                .sources
                .first()
                .and_then(|source| self.actors.get(&source.owner))
                .map(|owner| {
                    let kept = match &owner.rules.attack.path {
                        AttackPath::ControlBeam { keeps_buffs, .. } => keeps_buffs.clone(),
                        _ => Vec::new(),
                    };
                    (owner.placement.team, kept)
                });
            if let (true, Some((team, kept))) = (due, turner)
                && self.change_team(target, (team, &kept), step, events)?
                && !self.actors[&target].summoned
            {
                turned.push(target);
            }
        }
        self.turned_unnamed.append(&mut turned);
        Ok(())
    }

    /// The formation a turned unit is recorded under: `FightMech.GetMechTeam`
    /// answers none for it, so the recorder numbers one for the unit itself
    /// as it first records it, walking the units by side, then by where they
    /// stand, `z` before `x`. The units turned on one tick take their
    /// formations in that order as the tick ends, wherever they stood as they
    /// turned: six Crawlers Multi Control turned at once number by how far
    /// up the field each stands.
    pub(in crate::fight) fn name_turned_formations(&mut self) {
        let mut turned = std::mem::take(&mut self.turned_unnamed);
        if turned.is_empty() {
            return;
        }
        let mut formations = turned
            .iter()
            .map(|id| self.actors[id].placement.formation_id)
            .collect::<Vec<_>>();
        formations.sort_unstable();
        turned.sort_by_key(|id| {
            let actor = &self.actors[id];
            (actor.placement.team, actor.z_q32, actor.x_q32, *id)
        });
        for (id, formation_id) in turned.into_iter().zip(formations) {
            self.actors
                .get_mut(&id)
                .expect("actor identity is stable")
                .placement
                .formation_id = formation_id;
        }
    }

    /// A stroke that killed a turned unit: `DeadEffectSystem` queues it, and
    /// its `OnDead` waits until every unit and projectile has updated.
    pub(in crate::fight) fn turned_unit_fell(&mut self, target: FightActorRef, stroke: &Stroke) {
        if let (FightActorRef::Unit(unit_id), true) = (target, stroke.killed)
            && self
                .actors
                .get(&unit_id)
                .is_some_and(|actor| actor.placement.team != actor.original_team)
        {
            self.turned_fallen.push(unit_id);
        }
    }

    /// `TeamTranslationSystem.OnMechDead` for a unit a beam turned, from
    /// `DeadEffectSystem` after the tick's updates: it goes back to the side
    /// it was deployed on, and every skill still locked on it stops its
    /// attack (`FightSkill.OnChangeTeam`). A Crawler that updated after the
    /// blow has met the death in its own update already.
    pub(in crate::fight) fn turned_unit_died(&mut self, unit_id: u64) {
        let Some(actor) = self.actors.get(&unit_id) else {
            return;
        };
        // `TeamTranslationSystem.IsMechInFightGroup`: a summon stands in no
        // side's `FightTeam`, and dies on the side that turned it.
        if actor.placement.team == actor.original_team || actor.summoned {
            return;
        }
        let (team, x_q32, z_q32, radius) = (
            actor.original_team,
            actor.x_q32,
            actor.z_q32,
            actor.rules.collision_radius(),
        );
        let old_team = actor.placement.team;
        let unit = FightActorRef::Unit(unit_id);
        for trees in [&mut self.target_quadtrees, &mut self.mech_quadtrees] {
            if let Some(tree) = trees.get_mut(&old_team) {
                tree.remove(unit);
            }
            if let Some(tree) = trees.get_mut(&team) {
                tree.insert(unit, x_q32, z_q32, radius);
            }
        }
        let actor = self
            .actors
            .get_mut(&unit_id)
            .expect("actor identity is stable");
        actor.placement.team = team;
        actor.placement.formation_id = actor.original_formation;
        self.returned_dead.insert(unit_id);
        self.joins_side_last(unit_id);
        let step = self.step_now;
        for (skill_ref, offset) in self.skills_locked_on(unit, Some(unit_id)) {
            self.lock_changed_team(skill_ref, offset, step);
        }
    }

    /// `FightSkill.OnChangeTeam`, for a skill whose lock changed side:
    /// `StopAttack`, which drops the lock and keeps the attack target, and
    /// then, unless the skill is idle already or cooling, its cooling, or
    /// `SkillIdleState` with its targets cleared when it has none. So the
    /// weapons of a skill that was idle go on naming what they named until
    /// it takes another, and a cooling's until the idle state it ends in
    /// clears them, as the Hacker's do; a Crawler that was striking a turned
    /// Crawler names nothing from the tick it falls. The motion is left to
    /// its own update.
    ///
    /// Each skill of a group hears it as its own `FightSkill`, `offset` its
    /// place in the group; one of the main skill's group that searches for
    /// the unit (`FightSkillBase.IsMainSearcher`) hands the unit the dropped
    /// lock too, as `StopAttack` does.
    fn lock_changed_team(&mut self, skill_ref: SkillRef, offset: usize, cooling_from: u64) {
        let FightActorRef::Unit(actor_id) = skill_ref.owner else {
            unreachable!("only a unit's skill locks a unit that changes side")
        };
        let cooling_steps =
            native_time_units_to_steps(self.skill_rules(skill_ref).cooling_time_units());
        // `OnChangeTeam` returns after `StopAttack` for a permanent
        // preemptive skill not yet active, and for the main skill while one
        // is active: the main skill that one has locked stays locked. A main
        // skill a running preemptive skill has locked leaves its lock state.
        let permanent_holds = match skill_ref.slot {
            SkillSlot::Main => self.actors[&actor_id].skills.preemptive_active,
            SkillSlot::Extra(index) => {
                !self.actors[&actor_id].skills.preemptive_active
                    && self.actors[&actor_id].skills.extras[index]
                        .rules
                        .preemptive
                        .is_some()
            }
        };
        let core = self.skill_mut(skill_ref);
        if offset > 0 && skill_ref.slot == SkillSlot::Main && core.joined(offset).is_none() {
            core.set_mech_lock(None);
        }
        let skill = core.group_skill_mut(offset);
        // A skill firing at a shield has no attack target to go on naming,
        // as at the end of an attack (`SkillAttackState.Finish`).
        let fired_at = skill
            .attack_target()
            .filter(|_| skill.shield_target().is_none());
        skill.drop_lock();
        skill.performer.stop();
        if permanent_holds {
            self.sync_beam(actor_id);
            return;
        }
        match skill.state {
            SkillState::Idle { .. } => skill.attack_target_left = fired_at,
            SkillState::Cooling { .. } if cooling_steps > 0 => {}
            _ if cooling_steps > 0 => {
                skill.set_pending(None);
                skill.set_phase(FightSkillPhase::Idle);
                skill.set_backswing_finish_step(None);
                skill.set_cooling(Some((cooling_from, fired_at)));
            }
            _ => {
                skill.set_pending(None);
                skill.set_phase(FightSkillPhase::Idle);
                skill.set_backswing_finish_step(None);
                skill.attack_target_left = None;
                skill.search_target_time = 0;
            }
        }
        self.sync_beam(actor_id);
    }

    /// Every unit's skill locked on a unit, the main skill's and each extra
    /// skill's, each skill of a group its own, each a `FightSkill` that
    /// hears its lock change side (`FightSkill.OnChangeTeam`): a Tarantula's
    /// Spider Mine skill drops a turned Crawler that dies and goes back to
    /// its side. `except` leaves one unit's out.
    fn skills_locked_on(&self, unit: FightActorRef, except: Option<u64>) -> Vec<(SkillRef, usize)> {
        self.actors
            .iter()
            .filter(|(id, _)| Some(**id) != except)
            .flat_map(|(&id, actor)| {
                let owner = FightActorRef::Unit(id);
                std::iter::once((SkillSlot::Main, &actor.skills.main))
                    .chain(
                        actor
                            .skills
                            .extras
                            .iter()
                            .enumerate()
                            .map(|(index, extra)| (SkillSlot::Extra(index), &extra.skill)),
                    )
                    .flat_map(|(slot, skill)| {
                        (0..skill.group_size().max(1))
                            .filter(|&offset| skill.group_skill(offset).lock_target == Some(unit))
                            .map(move |offset| (SkillRef { owner, slot }, offset))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// `TeamTranslationSystem.ChangeTeam` and `FightActor.ChangeTeam`: the
    /// unit leaves its side's lists for the other's, and every skill locked
    /// on it stops its attack (`FightSkill.OnChangeTeam`).
    fn change_team(
        &mut self,
        unit_id: u64,
        (team, kept): (u32, &[u32]),
        step: u64,
        events: &mut Vec<Event>,
    ) -> Result<bool> {
        let (old_team, x_q32, z_q32, radius) = {
            let actor = &self.actors[&unit_id];
            if actor.placement.team == team {
                return Ok(false);
            }

            (
                actor.placement.team,
                actor.x_q32,
                actor.z_q32,
                actor.rules.collision_radius(),
            )
        };
        // The unit is handed out while it still stands on its old side, as a
        // kill with no killer: the beam's owner shares it as an attacker.
        let _ = self.hand_out_turned(team, unit_id);
        // `TeamTranslationSystem.ChangeTeam` first takes its buffs off.
        self.remove_buffs_on_turn(unit_id, kept, events)?;
        let unit = FightActorRef::Unit(unit_id);
        for trees in [&mut self.target_quadtrees, &mut self.mech_quadtrees] {
            if let Some(tree) = trees.get_mut(&old_team) {
                tree.remove(unit);
            }
            if let Some(tree) = trees.get_mut(&team) {
                tree.insert(unit, x_q32, z_q32, radius);
            }
        }
        // It leaves its `MechTeam` for one of its own on the new side
        // (`FightMech.GetMechTeam` answers none for a turned unit), and its
        // skill lets go of what it was after and searches again. A summon
        // has no `MechTeam` to leave, and keeps the formation it is recorded
        // under.
        let formation_id = if self.actors[&unit_id].summoned {
            self.actors[&unit_id].placement.formation_id
        } else {
            let formation_id = self.ids.next_formation;
            self.ids.next_formation += 1;
            formation_id
        };
        let actor = self
            .actors
            .get_mut(&unit_id)
            .expect("actor identity is stable");
        actor.placement.team = team;
        actor.placement.formation_id = formation_id;
        // `OnChangeTeam` reaches `MechGrounpSystem.ChangeMechGroup`.
        self.change_group_side(unit_id, old_team);
        let actor = self
            .actors
            .get_mut(&unit_id)
            .expect("actor identity is stable");
        if actor.summoned {
            self.summon_changes_side(unit_id, old_team, team);
        }
        self.interceptors_change_side(unit_id);
        self.joins_side_last(unit_id);
        // Its own attack ends as a skill's whose lock changed side: what it
        // was striking stands on its side now. A Hacker turned on its
        // Hacker reads cooling at it on the tick it turns, and a Crawler,
        // which has no cooling, idle and searched again. Which build method
        // ends it is not established.
        let mut locked = vec![(SkillRef::main(unit), 0)];
        locked.extend((0..self.actors[&unit_id].skills.extras.len()).map(|index| {
            (
                SkillRef {
                    owner: unit,
                    slot: SkillSlot::Extra(index),
                },
                0,
            )
        }));
        locked.extend(self.skills_locked_on(unit, None));
        let running_preemptive = self.actors[&unit_id].skills.running_preemptive;
        // The change runs before any unit updates, so a skill it sends into
        // its cooling is updated in it on this tick, as if it had entered it
        // on the last.
        for (skill_ref, offset) in locked {
            self.lock_changed_team(skill_ref, offset, step.saturating_sub(1));
        }
        // A running preemptive skill sent to its idle state hands the main
        // skill its place back (`PreemptiveSkillEnterIdleBehaviour`).
        if let Some(index) = running_preemptive
            && matches!(
                self.actors[&unit_id].skills.extras[index].skill.state,
                SkillState::Idle { .. }
            )
        {
            self.preemptive_enters_idle(unit_id);
        }
        Ok(true)
    }
}
