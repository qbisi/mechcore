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
    /// The owners of the skills whose beams hold it, in the order they
    /// started.
    pub(in crate::fight) sources: Vec<u64>,
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
            .is_some_and(|actor| actor.placement.ignores_control_beam)
    }

    /// `NormalAttackPerformer.Enter`, `Exit` and the change of attack target
    /// between them, for a skill that fires a control beam: the effect it
    /// holds follows the attack state and its target.
    pub(in crate::fight) fn sync_beam(&mut self, actor_id: u64) {
        let actor = &self.actors[&actor_id];
        if actor.skills.main.kind != SkillKind::ControlBeam {
            return;
        }
        // `FightControllBeamSkill.GetAttackEffect`: the control effect for a
        // unit that may be turned, the skill's damage effect for anything
        // else, a shield the skill fires at in place of its lock among it.
        let wanted = (actor.alive() && matches!(actor.skills.main.state, SkillState::Attack(_)))
            .then(|| actor.skills.main.attack_target())
            .flatten()
            .map(|target| {
                let control = match target {
                    FightActorRef::Unit(id) => {
                        !self.ignores_control(id) && actor.skills.main.shield_target().is_none()
                    }
                    FightActorRef::Building(_) => false,
                };
                Beam { target, control }
            });
        if actor.beam == wanted {
            return;
        }
        self.stop_beam(actor_id);
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
                Some(entry) => entry.sources.push(actor_id),
                None => self.translations.push(Translation {
                    target: id,
                    progress: 0,
                    sources: vec![actor_id],
                }),
            }
        }
        self.actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .beam = Some(Beam { target, control });
    }

    /// `ControllEffect.Stop` and `TeamTranslationSystem.Remove`.
    fn stop_beam(&mut self, actor_id: u64) {
        let Some(beam) = self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
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
                entry.sources.retain(|&source| source != actor_id);
            }
            self.translations.retain(|entry| !entry.sources.is_empty());
        }
    }

    /// `ControlBeamDamageCalculator.GetAttackDamage`: the skill's damage,
    /// and for the hits up to `warmup_attack_count`, counted from zero, that
    /// damage times the warm-up multiplier, at least one.
    fn beam_damage(&self, actor_id: u64, attack_count: i32) -> i64 {
        let actor = &self.actors[&actor_id];
        let damage = self.main_attack_damage(actor_id);
        let AttackPath::ControlBeam {
            warmup_attack_count,
            warmup_damage_multiplier,
        } = actor.rules.attack.path
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

    /// What a recording reads as the beam's damage: its damage calculator
    /// with the default attack count, zero, which is the first warmup hit
    /// whatever the beam is on. The Hacker reads 1 against a unit it may
    /// turn, a unit wearing the Anti-Interference Module and a shield alike.
    pub(in crate::fight) fn beam_snapshot_damage(&self, actor: &Actor) -> Option<i32> {
        if actor.skills.main.kind != SkillKind::ControlBeam {
            return None;
        }
        let damage = self.beam_damage(actor.placement.unit_id, 0);
        Some(i32::try_from(damage).unwrap_or(i32::MAX))
    }

    /// `NormalAttackPerformer.TryPerformEffect` for a control beam:
    /// `ControllEffect.Perform` adds the hit's power to the lock's entry,
    /// and `DamageEffect.Perform` strikes with the damage the beam's damage
    /// effect deals.
    pub(in crate::fight) fn control_effect(
        &mut self,
        actor_id: u64,
        target: FightActorRef,
        events: &mut Vec<Event>,
    ) -> Result<()> {
        self.sync_beam(actor_id);
        let actor = &self.actors[&actor_id];
        let attack_count = actor.skills.main.attack_count;
        let damage = self.beam_damage(actor_id, attack_count);
        let control = actor.beam.is_some_and(|beam| beam.control);
        if !control {
            // A hit that deals nothing strikes nothing: the warmup hits on a
            // unit wearing the Anti-Interference Module write no damage.
            let amount = damage_effect(damage);
            if amount < 1 {
                return Ok(());
            }
            return self.direct_effect_dealing(actor_id, target, amount, events);
        }
        // `ControllEffect.Perform` turns the skill's lock.
        let Some(FightActorRef::Unit(lock)) = actor.skills.main.lock_target else {
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
        self.count_experience(Some(owner), team, FightActorRef::Unit(lock), false)
    }

    /// `TeamTranslationSystem.Update`: every unit whose progress has reached
    /// its life changes to the side of its first beam's owner. The keys are
    /// taken first and each is asked again as it comes: turning one unit
    /// stops the beams locked on it and its own, which may take another's
    /// entry away or change the side its owner stands on. Two Hackers that
    /// turn each other on one tick leave both on the side of the first one
    /// turned.
    pub(in crate::fight) fn update_translations(&mut self, step: u64) {
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
            let team = entry
                .sources
                .first()
                .and_then(|owner| self.actors.get(owner))
                .map(|owner| owner.placement.team);
            if let (true, Some(team)) = (due, team)
                && self.change_team(target, team, step)
            {
                turned.push(target);
            }
        }
        // A turned unit's `MechTeam` is made when it is first asked for
        // (`FightMech.GetMechTeam`), which the recorder does unit by unit:
        // the units turned on one tick take their formations in identity
        // order, whichever was turned first.
        let mut formations = turned
            .iter()
            .map(|id| self.actors[id].placement.formation_id)
            .collect::<Vec<_>>();
        formations.sort_unstable();
        turned.sort_unstable();
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
        if actor.placement.team == actor.original_team {
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
        let step = self.step_now;
        let locked = self
            .actors
            .iter()
            .filter(|(id, actor)| **id != unit_id && actor.skills.main.lock_target == Some(unit))
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        for id in locked {
            self.lock_changed_team(id, step);
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
    fn lock_changed_team(&mut self, actor_id: u64, cooling_from: u64) {
        let cooling_steps =
            native_time_units_to_steps(self.actors[&actor_id].rules.attack.cooling_time_units());
        let skill = &mut self
            .actors
            .get_mut(&actor_id)
            .expect("actor identity is stable")
            .skills
            .main;
        let fired_at = skill.attack_target();
        skill.drop_lock();
        skill.performer.stop();
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

    /// `TeamTranslationSystem.ChangeTeam` and `FightActor.ChangeTeam`: the
    /// unit leaves its side's lists for the other's, and every skill locked
    /// on it stops its attack (`FightSkill.OnChangeTeam`).
    fn change_team(&mut self, unit_id: u64, team: u32, step: u64) -> bool {
        let (old_team, x_q32, z_q32, radius) = {
            let actor = &self.actors[&unit_id];
            if actor.placement.team == team {
                return false;
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
        let unit = FightActorRef::Unit(unit_id);
        for trees in [&mut self.target_quadtrees, &mut self.mech_quadtrees] {
            if let Some(tree) = trees.get_mut(&old_team) {
                tree.remove(unit);
            }
            if let Some(tree) = trees.get_mut(&team) {
                tree.insert(unit, x_q32, z_q32, radius);
            }
        }
        // It leaves its `MechTeam` for one of its own on the new side, and
        // its skill lets go of what it was after and searches again.
        let formation_id = self.ids.next_formation;
        self.ids.next_formation += 1;
        let actor = self
            .actors
            .get_mut(&unit_id)
            .expect("actor identity is stable");
        actor.placement.team = team;
        actor.placement.formation_id = formation_id;
        // Its own attack ends as a skill's whose lock changed side: what it
        // was striking stands on its side now. A Hacker turned on its
        // Hacker reads cooling at it on the tick it turns, and a Crawler,
        // which has no cooling, idle and searched again. Which build method
        // ends it is not established.
        let mut locked = vec![unit_id];
        locked.extend(
            self.actors
                .iter()
                .filter(|(_, actor)| actor.skills.main.lock_target == Some(unit))
                .map(|(&id, _)| id),
        );
        // The change runs before any unit updates, so a skill it sends into
        // its cooling is updated in it on this tick, as if it had entered it
        // on the last.
        for id in locked {
            self.lock_changed_team(id, step.saturating_sub(1));
        }
        true
    }
}
